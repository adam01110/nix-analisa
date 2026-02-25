use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::mpsc::{self, Receiver};
use std::thread;

use eframe::egui::{
    self, vec2, Align, Align2, Color32, Context, FontId, Layout, Painter, Pos2, Rect, RichText,
    Sense, Stroke, Ui, Vec2,
};
use fuzzy_matcher::skim::SkimMatcherV2;
use fuzzy_matcher::FuzzyMatcher;

use crate::nix_data::{collect_system_graph, SizeMetric, SystemGraph};
use crate::util::{format_bytes, short_name, stable_pair};

pub struct NixAnalyzeApp {
    system_path: String,
    state: AppState,
}

enum AppState {
    Loading {
        rx: Receiver<Result<SystemGraph, String>>,
    },
    Ready(ViewModel),
    Error(String),
}

struct ViewModel {
    graph: SystemGraph,
    metric: SizeMetric,
    min_size_mb: f32,
    max_nodes: usize,
    search: String,
    selected: Option<String>,
    pan: Vec2,
    zoom: f32,
    live_physics: bool,
    physics_intensity: f32,
    physics_repulsion: f32,
    physics_spring: f32,
    physics_collision: f32,
    physics_velocity_damping: f32,
    physics_target_spread: f32,
    physics_spread_force: f32,
    graph_dirty: bool,
    graph_cache: Option<RenderGraph>,
    top_nar: Vec<String>,
    top_closure: Vec<String>,
    top_referrers: Vec<String>,
    metric_rows_visible: usize,
    referrer_rows_visible: usize,
    related_rows_visible: usize,
    show_fps_bar: bool,
    fps_show_current: bool,
    fps_show_average: bool,
    fps_show_low: bool,
    fps_show_high: bool,
    fps_show_frame_time: bool,
    fps_current: f32,
    fps_samples: VecDeque<f32>,
    visible_node_count: usize,
    visible_edge_count: usize,
}

struct RenderGraph {
    nodes: Vec<RenderNode>,
    edges: Vec<(usize, usize)>,
    index_by_id: HashMap<String, usize>,
    outgoing: Vec<Vec<usize>>,
    incoming: Vec<Vec<usize>>,
    root_index: Option<usize>,
    min_metric: u64,
    max_metric: u64,
}

struct RenderNode {
    id: String,
    world_pos: Vec2,
    velocity: Vec2,
    metric_value: u64,
    base_radius: f32,
}

struct HighlightState {
    related_nodes: HashSet<usize>,
    related_edges: HashSet<(usize, usize)>,
    root_path_nodes: HashSet<usize>,
    root_path_edges: HashSet<(usize, usize)>,
}

struct RelatedNodeEntry {
    id: String,
    metric_value: u64,
    is_root_path: bool,
    is_direct: bool,
    is_in_view: bool,
}

#[derive(Clone, Copy)]
struct PhysicsConfig {
    intensity: f32,
    repulsion_scale: f32,
    spring_scale: f32,
    collision_scale: f32,
    velocity_damping: f32,
    target_spread: f32,
    spread_force: f32,
}

impl NixAnalyzeApp {
    pub fn new(_cc: &eframe::CreationContext<'_>, system_path: String) -> Self {
        let state = Self::start_load(system_path.clone());
        Self { system_path, state }
    }

    fn start_load(system_path: String) -> AppState {
        let (tx, rx) = mpsc::channel();

        thread::spawn(move || {
            let result = collect_system_graph(&system_path).map_err(|error| error.to_string());
            let _ = tx.send(result);
        });

        AppState::Loading { rx }
    }
}

impl eframe::App for NixAnalyzeApp {
    fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        let mut transition = None;

        match &mut self.state {
            AppState::Loading { rx } => {
                if let Ok(result) = rx.try_recv() {
                    transition = Some(match result {
                        Ok(graph) => AppState::Ready(ViewModel::new(graph)),
                        Err(error) => AppState::Error(error),
                    });
                }

                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.vertical_centered(|ui| {
                        ui.add_space(120.0);
                        ui.heading("Loading NixOS closure graph...");
                        ui.add_space(8.0);
                        ui.spinner();
                    });
                });
            }
            AppState::Error(error) => {
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.heading("Failed to collect NixOS closure graph");
                    ui.add_space(6.0);
                    ui.label(error.as_str());
                    ui.add_space(10.0);
                    if ui.button("Retry").clicked() {
                        transition = Some(Self::start_load(self.system_path.clone()));
                    }
                });
            }
            AppState::Ready(model) => {
                let mut reload_requested = false;
                model.show(ctx, &self.system_path, &mut reload_requested);
                if reload_requested {
                    transition = Some(Self::start_load(self.system_path.clone()));
                }
            }
        }

        if let Some(next_state) = transition {
            self.state = next_state;
        }
    }
}

impl ViewModel {
    const INITIAL_RANKING_ROWS: usize = 20;
    const RANKING_PAGE_ROWS: usize = 20;
    const RANKING_PREFETCH_MARGIN: usize = 4;
    const INITIAL_RELATED_ROWS: usize = 24;
    const RELATED_PAGE_ROWS: usize = 24;
    const RELATED_PREFETCH_MARGIN: usize = 4;

    fn new(graph: SystemGraph) -> Self {
        let top_nar = graph.top_by_metric(SizeMetric::NarSize, 128);
        let top_closure = graph.top_by_metric(SizeMetric::ClosureSize, 128);
        let top_referrers = graph.top_by_referrers(128);

        Self {
            selected: None,
            max_nodes: 450,
            graph,
            metric: SizeMetric::NarSize,
            min_size_mb: 64.0,
            search: String::new(),
            pan: Vec2::ZERO,
            zoom: 1.0,
            live_physics: true,
            physics_intensity: 1.0,
            physics_repulsion: 2.6,
            physics_spring: 0.2,
            physics_collision: 1.0,
            physics_velocity_damping: 0.9,
            physics_target_spread: 2.0,
            physics_spread_force: 0.08,
            graph_dirty: true,
            graph_cache: None,
            top_nar,
            top_closure,
            top_referrers,
            metric_rows_visible: Self::INITIAL_RANKING_ROWS,
            referrer_rows_visible: Self::INITIAL_RANKING_ROWS,
            related_rows_visible: Self::INITIAL_RELATED_ROWS,
            show_fps_bar: true,
            fps_show_current: true,
            fps_show_average: true,
            fps_show_low: false,
            fps_show_high: false,
            fps_show_frame_time: true,
            fps_current: 0.0,
            fps_samples: VecDeque::new(),
            visible_node_count: 0,
            visible_edge_count: 0,
        }
    }

    fn show(&mut self, ctx: &Context, system_path: &str, reload_requested: &mut bool) {
        self.update_fps_counter(ctx);
        if self.graph_dirty {
            self.rebuild_render_graph();
        }

        egui::TopBottomPanel::top("top_bar")
            .resizable(false)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.heading("nix-analisá");
                    ui.separator();
                    ui.label(format!("root: {}", short_name(&self.graph.root_id)));
                    ui.label(format!("store: {}", self.graph.store_dir));
                    ui.label(format!("system path: {system_path}"));
                    ui.label(format!("nodes: {}", self.graph.node_count()));
                    ui.label(format!("edges: {}", self.graph.edge_count));
                    if ui.button("Reload closure").clicked() {
                        *reload_requested = true;
                    }
                    if ui.button("Reset view").clicked() {
                        self.pan = Vec2::ZERO;
                        self.zoom = 1.0;
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if let Some(visible_graph_text) = self.visible_graph_text() {
                            ui.label(visible_graph_text);
                        }
                        if let Some(fps_text) = self.fps_display_text() {
                            ui.label(fps_text);
                        }
                    });
                });
            });

        egui::SidePanel::left("controls")
            .resizable(true)
            .default_width(350.0)
            .show(ctx, |ui| self.draw_controls(ui));

        egui::SidePanel::right("details")
            .resizable(true)
            .default_width(360.0)
            .show(ctx, |ui| self.draw_details(ui));

        egui::CentralPanel::default().show(ctx, |ui| self.draw_graph(ui));
    }

    fn set_selected(&mut self, selected: Option<String>) {
        let changed = self.selected != selected;
        if !changed {
            return;
        }

        self.selected = selected;
        self.related_rows_visible = Self::INITIAL_RELATED_ROWS;
    }

    fn draw_controls(&mut self, ui: &mut Ui) {
        ui.heading("Graph Controls");
        ui.add_space(4.0);

        let mut changed = false;
        let current_metric = self.metric;

        ui.horizontal(|ui| {
            changed |= ui
                .selectable_value(&mut self.metric, SizeMetric::NarSize, "Node size")
                .on_hover_text("Scale nodes and ranking by NAR size.")
                .changed();
            changed |= ui
                .selectable_value(&mut self.metric, SizeMetric::ClosureSize, "Closure size")
                .on_hover_text("Scale nodes and ranking by transitive closure size.")
                .changed();
        });

        if self.metric != current_metric {
            self.metric_rows_visible = Self::INITIAL_RANKING_ROWS;
        }

        changed |= ui
            .add(egui::Slider::new(&mut self.min_size_mb, 0.0..=4096.0).text("Min node size (MiB)"))
            .on_hover_text("Hide nodes smaller than this size before rendering.")
            .changed();

        let max_render_nodes_limit = self.graph.node_count().max(2);
        changed |= ui
            .add(
                egui::Slider::new(&mut self.max_nodes, 2..=max_render_nodes_limit)
                    .text("Max rendered nodes"),
            )
            .on_hover_text("Cap the number of nodes shown to keep rendering responsive.")
            .changed();

        ui.checkbox(&mut self.live_physics, "Live physics simulation")
            .on_hover_text("Continuously simulate layout forces while viewing the graph.");

        ui.collapsing("Physics tuning", |ui| {
            ui.add(
                egui::Slider::new(&mut self.physics_intensity, 0.2..=2.5)
                    .text("Intensity")
                    .clamping(egui::SliderClamping::Always),
            )
            .on_hover_text("Overall strength applied to all physics forces.");
            ui.add(
                egui::Slider::new(&mut self.physics_repulsion, 0.25..=2.6)
                    .text("Repulsion")
                    .clamping(egui::SliderClamping::Always),
            )
            .on_hover_text("How strongly nodes push away from each other.");
            ui.add(
                egui::Slider::new(&mut self.physics_spring, 0.2..=2.2)
                    .text("Edge spring")
                    .clamping(egui::SliderClamping::Always),
            )
            .on_hover_text("How strongly connected nodes pull toward their target distance.");
            ui.add(
                egui::Slider::new(&mut self.physics_collision, 0.2..=2.0)
                    .text("Collision")
                    .clamping(egui::SliderClamping::Always),
            )
            .on_hover_text("Extra separation force to prevent overlap between nearby nodes.");
            ui.add(
                egui::Slider::new(&mut self.physics_velocity_damping, 0.78..=0.97)
                    .text("Velocity damping")
                    .clamping(egui::SliderClamping::Always),
            )
            .on_hover_text("How quickly node movement slows each frame.");
            ui.add(
                egui::Slider::new(&mut self.physics_target_spread, 0.6..=2.0)
                    .text("Target spread")
                    .clamping(egui::SliderClamping::Always),
            )
            .on_hover_text("Preferred spacing between connected regions of the graph.");
            ui.add(
                egui::Slider::new(&mut self.physics_spread_force, 0.0..=0.08)
                    .text("Spread correction")
                    .clamping(egui::SliderClamping::Always),
            )
            .on_hover_text("How aggressively layout drift is corrected over time.");
        });

        ui.checkbox(&mut self.show_fps_bar, "FPS Display")
            .on_hover_text("Show a live FPS readout in the header.");

        ui.collapsing("FPS Display tuning", |ui| {
            ui.add_enabled_ui(self.show_fps_bar, |ui| {
                ui.checkbox(&mut self.fps_show_current, "Show current FPS")
                    .on_hover_text("Display the most recent frame rate sample.");
                ui.checkbox(&mut self.fps_show_average, "Show average FPS")
                    .on_hover_text("Display the running average FPS over recent samples.");
                ui.checkbox(&mut self.fps_show_low, "Show low FPS")
                    .on_hover_text("Display the minimum FPS from the recent sample window.");
                ui.checkbox(&mut self.fps_show_high, "Show high FPS")
                    .on_hover_text("Display the maximum FPS from the recent sample window.");
                ui.checkbox(&mut self.fps_show_frame_time, "Show frame time")
                    .on_hover_text("Display frame duration in milliseconds.");
            });
        });

        ui.label("Search (hash or derivation name)")
            .on_hover_text("Fuzzy-highlight matching nodes without changing the rendered graph.");
        let search_response = ui.text_edit_singleline(&mut self.search);
        search_response
            .on_hover_text("Type to pseudo-highlight matching nodes, then click one to select it.");

        if changed {
            self.graph_dirty = true;
        }

        ui.separator();

        let primary_ranking = if self.metric == SizeMetric::NarSize {
            self.top_nar.clone()
        } else {
            self.top_closure.clone()
        };

        ui.label(RichText::new(format!("Top by {}", self.metric.label())).strong());
        self.draw_metric_ranking(ui, &primary_ranking, self.metric);

        ui.add_space(8.0);
        ui.label(RichText::new("Top by reverse dependencies").strong());
        let top_referrers = self.top_referrers.clone();
        self.draw_referrer_ranking(ui, &top_referrers);
    }

    fn update_fps_counter(&mut self, ctx: &Context) {
        const FPS_SAMPLE_WINDOW: usize = 180;

        let dt = ctx.input(|input| input.stable_dt);
        if dt <= f32::EPSILON {
            return;
        }

        self.fps_current = (1.0 / dt).clamp(0.0, 1000.0);
        self.fps_samples.push_back(self.fps_current);
        while self.fps_samples.len() > FPS_SAMPLE_WINDOW {
            self.fps_samples.pop_front();
        }
    }

    fn fps_display_text(&self) -> Option<String> {
        if !self.show_fps_bar {
            return None;
        }

        let mut parts = Vec::new();

        if self.fps_show_current {
            parts.push(format!("FPS {:.0}", self.fps_current));
        }

        if self.fps_show_average && !self.fps_samples.is_empty() {
            let avg = self.fps_samples.iter().sum::<f32>() / self.fps_samples.len() as f32;
            parts.push(format!("avg {:.1}", avg));
        }

        if self.fps_show_low {
            if let Some(low) = self.fps_samples.iter().copied().reduce(f32::min) {
                parts.push(format!("low {:.0}", low));
            }
        }

        if self.fps_show_high {
            if let Some(high) = self.fps_samples.iter().copied().reduce(f32::max) {
                parts.push(format!("high {:.0}", high));
            }
        }

        if self.fps_show_frame_time && self.fps_current > f32::EPSILON {
            parts.push(format!("{:.1} ms", 1000.0 / self.fps_current));
        }

        if parts.is_empty() {
            None
        } else {
            Some(parts.join(" | "))
        }
    }

    fn visible_graph_text(&self) -> Option<String> {
        self.graph_cache.as_ref().map(|cache| {
            format!(
                "visible graph: {} nodes / {} edges",
                self.visible_node_count.min(cache.nodes.len()),
                self.visible_edge_count.min(cache.edges.len())
            )
        })
    }

    fn draw_metric_ranking(&mut self, ui: &mut Ui, ids: &[String], metric: SizeMetric) {
        let row_count = ids.len().min(self.metric_rows_visible);
        let mut should_load_more = false;

        egui::ScrollArea::vertical()
            .id_salt("metric_ranking_scroll")
            .max_height(240.0)
            .auto_shrink([false, false])
            .show_rows(ui, 22.0, row_count, |ui, row_range| {
                if row_range.end + Self::RANKING_PREFETCH_MARGIN >= row_count {
                    should_load_more = true;
                }

                for index in row_range {
                    let Some(id) = ids.get(index) else {
                        continue;
                    };
                    let Some(node) = self.graph.nodes.get(id) else {
                        continue;
                    };

                    let is_selected = self.selected.as_deref() == Some(id.as_str());
                    let value_label = format_bytes(node.metric(metric));

                    let row_response = ui
                        .horizontal(|ui| {
                            let clicked =
                                ui.selectable_label(is_selected, short_name(id)).clicked();
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                ui.label(value_label);
                            });
                            clicked
                        })
                        .inner;

                    if row_response {
                        self.set_selected(Some(id.clone()));
                    }
                }
            });

        if should_load_more && row_count < ids.len() {
            self.metric_rows_visible = (row_count + Self::RANKING_PAGE_ROWS).min(ids.len());
        }
    }

    fn draw_referrer_ranking(&mut self, ui: &mut Ui, ids: &[String]) {
        let row_count = ids.len().min(self.referrer_rows_visible);
        let mut should_load_more = false;

        egui::ScrollArea::vertical()
            .id_salt("referrer_ranking_scroll")
            .max_height(180.0)
            .auto_shrink([false, false])
            .show_rows(ui, 22.0, row_count, |ui, row_range| {
                if row_range.end + Self::RANKING_PREFETCH_MARGIN >= row_count {
                    should_load_more = true;
                }

                for index in row_range {
                    let Some(id) = ids.get(index) else {
                        continue;
                    };
                    let Some(node) = self.graph.nodes.get(id) else {
                        continue;
                    };

                    let is_selected = self.selected.as_deref() == Some(id.as_str());
                    let value_label = format!("{} refs", node.referrers.len());

                    let row_response = ui
                        .horizontal(|ui| {
                            let clicked =
                                ui.selectable_label(is_selected, short_name(id)).clicked();
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                ui.label(value_label);
                            });
                            clicked
                        })
                        .inner;

                    if row_response {
                        self.set_selected(Some(id.clone()));
                    }
                }
            });

        if should_load_more && row_count < ids.len() {
            self.referrer_rows_visible = (row_count + Self::RANKING_PAGE_ROWS).min(ids.len());
        }
    }

    fn draw_details(&mut self, ui: &mut Ui) {
        ui.heading("Selection Details");
        ui.add_space(6.0);

        let Some(selected_id) = self.selected.clone() else {
            ui.label("Select a node from the graph or rankings.");
            return;
        };

        let Some(node) = self.graph.nodes.get(&selected_id) else {
            ui.label("Selected node no longer exists in the graph state.");
            return;
        };

        let node_id = node.id.clone();
        let node_short = short_name(&node.id).to_string();
        let full_path = node.full_path.clone();
        let nar_size = node.nar_size;
        let closure_size = node.closure_size;
        let reference_count = node.references.len();
        let referrer_count = node.referrers.len();
        let deriver = node.deriver.clone();

        let related_nodes = self.related_nodes_for_details(&selected_id, 32);

        ui.label(RichText::new(node_short).strong());
        ui.small(node_id.as_str());
        ui.add_space(6.0);

        ui.label(format!("Full path: {full_path}"));
        ui.label(format!("Node size (narSize): {}", format_bytes(nar_size)));
        ui.label(format!("Closure size: {}", format_bytes(closure_size)));
        ui.label(format!("Direct dependencies: {reference_count}"));
        ui.label(format!("Reverse dependencies: {referrer_count}"));

        if let Some(deriver) = &deriver {
            ui.label(format!("Deriver: {deriver}"));
        }

        let transitive_delta = closure_size.saturating_sub(nar_size);
        ui.label(format!(
            "Transitive-only weight: {}",
            format_bytes(transitive_delta)
        ));

        ui.separator();
        ui.label(RichText::new("Why this can be large").strong());
        if nar_size > (1 << 30) {
            ui.label("- large direct payload (> 1 GiB)");
        }
        if transitive_delta > nar_size.saturating_mul(2) {
            ui.label("- large transitive dependency surface");
        }
        if referrer_count > 20 {
            ui.label("- reused by many other derivations");
        }
        if nar_size <= (1 << 30)
            && transitive_delta <= nar_size.saturating_mul(2)
            && referrer_count <= 20
        {
            ui.label("- moderate local and transitive contribution");
        }

        ui.separator();
        ui.label(RichText::new("Related nodes (in and out of view)").strong());
        if related_nodes.is_empty() {
            ui.label("No related nodes found for this selection.");
        } else {
            let row_count = related_nodes.len().min(self.related_rows_visible);
            let mut should_load_more = false;

            egui::ScrollArea::vertical()
                .id_salt("related_nodes_scroll")
                .max_height(320.0)
                .auto_shrink([false, false])
                .show_rows(ui, 22.0, row_count, |ui, row_range| {
                    if row_range.end + Self::RELATED_PREFETCH_MARGIN >= row_count {
                        should_load_more = true;
                    }

                    for index in row_range {
                        let Some(related) = related_nodes.get(index) else {
                            continue;
                        };
                        let mut flags = Vec::new();
                        if related.is_in_view {
                            flags.push("in-view");
                        } else {
                            flags.push("out-of-view");
                        }
                        if related.is_root_path {
                            flags.push("path");
                        }
                        if related.is_direct {
                            flags.push("direct");
                        }

                        let label = format!(
                            "{}  ({})  [{}]",
                            short_name(&related.id),
                            format_bytes(related.metric_value),
                            flags.join(", ")
                        );

                        if ui.link(label).on_hover_text(related.id.as_str()).clicked() {
                            self.set_selected(Some(related.id.clone()));
                        }
                    }
                });

            if should_load_more && row_count < related_nodes.len() {
                self.related_rows_visible =
                    (row_count + Self::RELATED_PAGE_ROWS).min(related_nodes.len());
            }
        }

        ui.separator();
        ui.label(RichText::new("Shortest path from root").strong());
        if let Some(path) = self.graph.shortest_path_from_root(&selected_id) {
            let rendered = if path.len() <= 14 {
                path.iter()
                    .map(|id| short_name(id).to_string())
                    .collect::<Vec<_>>()
                    .join(" -> ")
            } else {
                let head = path
                    .iter()
                    .take(8)
                    .map(|id| short_name(id).to_string())
                    .collect::<Vec<_>>()
                    .join(" -> ");
                let tail = path
                    .iter()
                    .skip(path.len().saturating_sub(4))
                    .map(|id| short_name(id).to_string())
                    .collect::<Vec<_>>()
                    .join(" -> ");
                format!("{head} -> ... -> {tail}")
            };
            ui.label(rendered);
        } else {
            ui.label("No root-reachable path found in the current closure graph.");
        }
    }

    fn related_nodes_for_details(&self, selected_id: &str, limit: usize) -> Vec<RelatedNodeEntry> {
        if limit == 0 {
            return Vec::new();
        }

        let mut related_by_id: HashMap<String, RelatedNodeEntry> = HashMap::new();

        if let Some(node) = self.graph.nodes.get(selected_id) {
            for id in node.references.iter().chain(node.referrers.iter()) {
                if id == selected_id {
                    continue;
                }

                if let Some(related_node) = self.graph.nodes.get(id) {
                    let entry = related_by_id.entry(id.clone()).or_insert(RelatedNodeEntry {
                        id: id.clone(),
                        metric_value: related_node.metric(self.metric),
                        is_root_path: false,
                        is_direct: false,
                        is_in_view: false,
                    });
                    entry.is_direct = true;
                }
            }
        }

        if let Some(path) = self.graph.shortest_path_from_root(selected_id) {
            for id in path {
                if id == selected_id {
                    continue;
                }

                if let Some(related_node) = self.graph.nodes.get(&id) {
                    let entry = related_by_id.entry(id.clone()).or_insert(RelatedNodeEntry {
                        id: id.clone(),
                        metric_value: related_node.metric(self.metric),
                        is_root_path: false,
                        is_direct: false,
                        is_in_view: false,
                    });
                    entry.is_root_path = true;
                }
            }
        }

        if let Some(cache) = &self.graph_cache {
            if let Some(&selected_index) = cache.index_by_id.get(selected_id) {
                let highlight = build_highlight_state(cache, selected_index);
                let direct_outgoing = cache
                    .outgoing
                    .get(selected_index)
                    .into_iter()
                    .flatten()
                    .copied()
                    .collect::<HashSet<_>>();
                let direct_incoming = cache
                    .incoming
                    .get(selected_index)
                    .into_iter()
                    .flatten()
                    .copied()
                    .collect::<HashSet<_>>();

                for index in highlight
                    .root_path_nodes
                    .iter()
                    .chain(highlight.related_nodes.iter())
                {
                    if *index == selected_index {
                        continue;
                    }

                    if let Some(render_node) = cache.nodes.get(*index) {
                        let entry = related_by_id.entry(render_node.id.clone()).or_insert(
                            RelatedNodeEntry {
                                id: render_node.id.clone(),
                                metric_value: self
                                    .graph
                                    .nodes
                                    .get(&render_node.id)
                                    .map(|node| node.metric(self.metric))
                                    .unwrap_or(render_node.metric_value),
                                is_root_path: false,
                                is_direct: false,
                                is_in_view: false,
                            },
                        );

                        entry.is_in_view = true;
                        entry.is_root_path |= highlight.root_path_nodes.contains(index);
                        entry.is_direct |=
                            direct_outgoing.contains(index) || direct_incoming.contains(index);
                    }
                }
            }
        }

        let mut related = related_by_id.into_values().collect::<Vec<_>>();

        related.sort_by(|a, b| {
            b.is_in_view
                .cmp(&a.is_in_view)
                .then_with(|| b.is_root_path.cmp(&a.is_root_path))
                .then_with(|| b.is_direct.cmp(&a.is_direct))
                .then_with(|| b.metric_value.cmp(&a.metric_value))
                .then_with(|| a.id.cmp(&b.id))
        });
        related.truncate(limit);
        related
    }

    fn draw_graph(&mut self, ui: &mut Ui) {
        if self.graph_dirty {
            self.rebuild_render_graph();
        }

        if self.live_physics {
            let physics = PhysicsConfig {
                intensity: self.physics_intensity,
                repulsion_scale: self.physics_repulsion,
                spring_scale: self.physics_spring,
                collision_scale: self.physics_collision,
                velocity_damping: self.physics_velocity_damping,
                target_spread: self.physics_target_spread,
                spread_force: self.physics_spread_force,
            };
            if let Some(cache) = self.graph_cache.as_mut() {
                step_physics(cache, physics);
                ui.ctx().request_repaint();
            }
        }

        let (rect, response) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
        let painter = ui.painter_at(rect);

        draw_background(&painter, rect, self.pan, self.zoom);

        if response.hovered() {
            let scroll = ui.input(|input| input.raw_scroll_delta.y);
            if scroll.abs() > f32::EPSILON {
                let pointer = ui
                    .input(|input| input.pointer.hover_pos())
                    .unwrap_or_else(|| rect.center());
                let world_before = screen_to_world(rect, self.pan, self.zoom, pointer);

                let zoom_factor = (1.0 + (scroll * 0.0018)).clamp(0.85, 1.15);
                self.zoom = (self.zoom * zoom_factor).clamp(0.05, 6.0);

                self.pan = pointer - rect.center() - (world_before * self.zoom);
            }
        }

        if response.dragged_by(egui::PointerButton::Secondary)
            || response.dragged_by(egui::PointerButton::Middle)
        {
            self.pan += response.drag_delta();
        }

        let Some(cache) = &self.graph_cache else {
            self.visible_node_count = 0;
            self.visible_edge_count = 0;
            ui.label("No nodes matched the current size/node filters.");
            return;
        };

        let mut screen_positions = Vec::with_capacity(cache.nodes.len());
        let mut screen_radii = Vec::with_capacity(cache.nodes.len());
        for render_node in &cache.nodes {
            screen_positions.push(world_to_screen(
                rect,
                self.pan,
                self.zoom,
                render_node.world_pos,
            ));
            screen_radii.push((render_node.base_radius * self.zoom.powf(0.40)).clamp(2.5, 46.0));
        }

        let mut visible_indices = Vec::new();
        for index in 0..cache.nodes.len() {
            if circle_visible(rect, screen_positions[index], screen_radii[index]) {
                visible_indices.push(index);
            }
        }
        self.visible_node_count = visible_indices.len();

        let pointer_pos = ui.input(|input| input.pointer.hover_pos());
        let hovered = pointer_pos.and_then(|pointer| {
            visible_indices
                .iter()
                .filter_map(|index| {
                    let distance = screen_positions[*index].distance(pointer);
                    if distance <= screen_radii[*index] {
                        Some((*index, distance))
                    } else {
                        None
                    }
                })
                .min_by(|a, b| a.1.total_cmp(&b.1))
        });

        if hovered.is_some() {
            ui.output_mut(|output| {
                output.cursor_icon = egui::CursorIcon::PointingHand;
            });
        }

        if response.clicked_by(egui::PointerButton::Primary) {
            if let Some((index, _distance)) = hovered {
                let selected = self
                    .graph_cache
                    .as_ref()
                    .and_then(|cache| cache.nodes.get(index).map(|node| node.id.clone()));
                if self.selected != selected {
                    self.selected = selected;
                    self.related_rows_visible = Self::INITIAL_RELATED_ROWS;
                }
            } else {
                if self.selected.is_some() {
                    self.selected = None;
                    self.related_rows_visible = Self::INITIAL_RELATED_ROWS;
                }
            }
        }

        let hovered_index = hovered.map(|(index, _)| index);
        let highlight = self.selected.as_ref().and_then(|id| {
            if let Some(selected_index) = cache.index_by_id.get(id).copied() {
                Some(build_highlight_state(cache, selected_index))
            } else {
                build_highlight_state_for_selected_id(&self.graph, cache, id)
            }
        });
        let selection_active = highlight.as_ref().is_some_and(|state| {
            !state.related_nodes.is_empty()
                || !state.related_edges.is_empty()
                || !state.root_path_nodes.is_empty()
                || !state.root_path_edges.is_empty()
        });
        let search_query = self.search.trim();
        let matcher = SkimMatcherV2::default();
        let pseudo_matches = if self.selected.is_none() && !search_query.is_empty() {
            cache
                .nodes
                .iter()
                .enumerate()
                .filter_map(|(index, node)| {
                    if fuzzy_match_score(&matcher, &node.id, search_query).is_some()
                        || fuzzy_match_score(&matcher, short_name(&node.id), search_query).is_some()
                    {
                        Some(index)
                    } else {
                        None
                    }
                })
                .collect::<HashSet<_>>()
        } else {
            HashSet::new()
        };
        let pseudo_active = !pseudo_matches.is_empty();

        let mut visible_edge_count = 0usize;
        for &(src, dst) in &cache.edges {
            if src >= cache.nodes.len() || dst >= cache.nodes.len() {
                continue;
            }

            let start = screen_positions[src];
            let end = screen_positions[dst];
            if !edge_visible(rect, start, end, 2.5) {
                continue;
            }
            visible_edge_count += 1;

            let (line_width, line_color) = if let Some(state) = &highlight {
                if state.root_path_edges.contains(&(src, dst)) {
                    (
                        (2.4 * self.zoom.sqrt()).clamp(1.2, 4.4),
                        Color32::from_rgb(246, 206, 104),
                    )
                } else if state.related_edges.contains(&(src, dst)) {
                    (
                        (1.7 * self.zoom.sqrt()).clamp(0.9, 3.3),
                        Color32::from_rgb(241, 146, 94),
                    )
                } else {
                    (
                        (0.45 * self.zoom.sqrt()).clamp(0.2, 1.2),
                        Color32::from_rgba_unmultiplied(80, 90, 104, 48),
                    )
                }
            } else {
                (
                    (0.7 * self.zoom.sqrt()).clamp(0.45, 2.2),
                    Color32::from_gray(72),
                )
            };

            painter.line_segment([start, end], Stroke::new(line_width, line_color));
        }
        self.visible_edge_count = visible_edge_count;

        let mut indices = visible_indices;
        indices.sort_by(|a, b| {
            cache.nodes[*a]
                .metric_value
                .cmp(&cache.nodes[*b].metric_value)
        });

        for index in indices {
            let render_node = &cache.nodes[index];
            let position = screen_positions[index];
            let radius = screen_radii[index];

            let is_selected = self.selected.as_deref() == Some(render_node.id.as_str());
            let is_hovered = hovered_index == Some(index);
            let is_root_path = highlight
                .as_ref()
                .is_some_and(|state| state.root_path_nodes.contains(&index));
            let is_related = highlight
                .as_ref()
                .is_some_and(|state| state.related_nodes.contains(&index));
            let is_pseudo_match = pseudo_matches.contains(&index);

            let base_color =
                metric_color(render_node.metric_value, cache.min_metric, cache.max_metric);
            let color = if is_selected {
                Color32::from_rgb(245, 206, 93)
            } else if is_hovered {
                Color32::from_rgb(255, 164, 101)
            } else if is_root_path {
                blend_color(base_color, Color32::from_rgb(247, 194, 111), 0.72)
            } else if is_related {
                blend_color(base_color, Color32::from_rgb(246, 137, 92), 0.60)
            } else if is_pseudo_match {
                blend_color(base_color, Color32::from_rgb(103, 196, 255), 0.68)
            } else if selection_active {
                dim_color(base_color, 0.30)
            } else if pseudo_active {
                dim_color(base_color, 0.22)
            } else {
                base_color
            };

            painter.circle_filled(position, radius, color);
            painter.circle_stroke(
                position,
                radius,
                Stroke::new(
                    if is_selected {
                        2.2
                    } else if is_root_path {
                        1.8
                    } else if is_pseudo_match {
                        1.55
                    } else {
                        1.0
                    },
                    Color32::from_rgba_unmultiplied(15, 15, 15, 190),
                ),
            );

            let highlighted = is_selected || is_root_path || is_related;
            let should_draw_label = is_selected
                || is_hovered
                || (highlighted && self.zoom > 0.45)
                || (is_pseudo_match && self.zoom > 0.35)
                || (selection_active && radius > 10.5)
                || radius > 17.0
                || self.zoom > 1.35;
            if should_draw_label {
                painter.text(
                    position + vec2(radius + 5.0, 0.0),
                    Align2::LEFT_CENTER,
                    short_name(&render_node.id),
                    FontId::proportional(12.0),
                    Color32::from_gray(238),
                );
            }
        }

        if let Some((hovered_index, _)) = hovered {
            if let Some(node) = self.graph.nodes.get(&cache.nodes[hovered_index].id) {
                let panel_text = format!(
                    "{}  |  {}  |  refs {}",
                    short_name(&node.id),
                    format_bytes(node.metric(self.metric)),
                    node.references.len()
                );
                painter.text(
                    rect.left_top() + vec2(10.0, 10.0),
                    Align2::LEFT_TOP,
                    panel_text,
                    FontId::proportional(13.0),
                    Color32::from_gray(240),
                );
            }
        }
    }

    fn rebuild_render_graph(&mut self) {
        let threshold = (self.min_size_mb.max(0.0) * 1024.0 * 1024.0) as u64;

        let mut ranked = self
            .graph
            .nodes
            .iter()
            .filter_map(|(id, node)| {
                let metric = node.metric(self.metric);

                let always_include =
                    id == &self.graph.root_id || self.selected.as_deref() == Some(id.as_str());

                if metric >= threshold || always_include {
                    Some((metric, id.clone()))
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();

        ranked.sort_by(|a, b| b.0.cmp(&a.0));

        let target_nodes = self.max_nodes.clamp(2, self.graph.node_count().max(2));
        let mut selected = HashSet::new();
        let mut ids = Vec::new();

        if self.graph.nodes.contains_key(&self.graph.root_id) {
            selected.insert(self.graph.root_id.clone());
            ids.push(self.graph.root_id.clone());
        }

        if let Some(selected_id) = &self.selected {
            if self.graph.nodes.contains_key(selected_id) && selected.insert(selected_id.clone()) {
                ids.push(selected_id.clone());
            }
        }

        for (_metric, id) in ranked {
            if ids.len() >= target_nodes {
                break;
            }
            if selected.insert(id.clone()) {
                ids.push(id);
            }
        }

        if ids.is_empty() {
            self.graph_cache = None;
            self.visible_node_count = 0;
            self.visible_edge_count = 0;
            self.graph_dirty = false;
            return;
        }

        let mut index_by_id = HashMap::with_capacity(ids.len());
        for (index, id) in ids.iter().enumerate() {
            index_by_id.insert(id.clone(), index);
        }

        let mut edges = Vec::new();
        for (source_index, source_id) in ids.iter().enumerate() {
            let Some(node) = self.graph.nodes.get(source_id) else {
                continue;
            };

            for target_id in &node.references {
                if let Some(&target_index) = index_by_id.get(target_id) {
                    if source_index != target_index {
                        edges.push((source_index, target_index));
                    }
                }
            }
        }
        edges.sort_unstable();
        edges.dedup();

        let mut min_metric = u64::MAX;
        let mut max_metric = 0u64;
        let mut metrics = Vec::with_capacity(ids.len());
        for id in &ids {
            let metric = self
                .graph
                .nodes
                .get(id)
                .map(|node| node.metric(self.metric).max(1))
                .unwrap_or(1);
            metrics.push(metric);
            min_metric = min_metric.min(metric);
            max_metric = max_metric.max(metric);
        }
        if min_metric == u64::MAX {
            min_metric = 1;
        }
        if max_metric < min_metric {
            max_metric = min_metric;
        }

        let node_radii = metrics
            .iter()
            .map(|metric| node_radius(*metric, min_metric, max_metric))
            .collect::<Vec<_>>();

        let root_index = index_by_id.get(&self.graph.root_id).copied();

        let nodes = ids
            .into_iter()
            .zip(metrics.into_iter().zip(node_radii))
            .enumerate()
            .map(|(index, (id, (metric_value, base_radius)))| {
                let (jx, jy) = stable_pair(&id);
                let mut direction = vec2(jx, jy);
                if direction.length_sq() <= 0.0001 {
                    let angle = ((index as f32) * 0.618_034 + 0.11) * std::f32::consts::TAU;
                    direction = vec2(angle.cos(), angle.sin());
                } else {
                    direction = direction.normalized();
                }

                let initial_speed = if root_index.is_some_and(|root| root == index) {
                    0.0
                } else {
                    1.15 + (base_radius * 0.022)
                };

                RenderNode {
                    id,
                    world_pos: Vec2::ZERO,
                    velocity: direction * initial_speed,
                    metric_value,
                    base_radius,
                }
            })
            .collect::<Vec<_>>();

        let mut outgoing = vec![Vec::new(); nodes.len()];
        let mut incoming = vec![Vec::new(); nodes.len()];
        for &(source, target) in &edges {
            if source < nodes.len() && target < nodes.len() {
                outgoing[source].push(target);
                incoming[target].push(source);
            }
        }

        self.graph_cache = Some(RenderGraph {
            nodes,
            edges,
            index_by_id,
            outgoing,
            incoming,
            root_index,
            min_metric,
            max_metric,
        });
        if let Some(cache) = &self.graph_cache {
            self.visible_node_count = cache.nodes.len();
            self.visible_edge_count = cache.edges.len();
        }
        self.graph_dirty = false;
    }
}

fn fuzzy_match_score(matcher: &SkimMatcherV2, text: &str, query: &str) -> Option<i64> {
    matcher
        .fuzzy_match(text, query)
        .or_else(|| matcher.fuzzy_match(&text.to_ascii_lowercase(), &query.to_ascii_lowercase()))
}

fn build_highlight_state(cache: &RenderGraph, selected_index: usize) -> HighlightState {
    let mut related_nodes = HashSet::new();
    let mut related_edges = HashSet::new();

    related_nodes.insert(selected_index);

    collect_related_paths(
        &cache.outgoing,
        selected_index,
        true,
        &mut related_nodes,
        &mut related_edges,
    );
    collect_related_paths(
        &cache.incoming,
        selected_index,
        false,
        &mut related_nodes,
        &mut related_edges,
    );

    let (root_path_nodes, root_path_edges) = if let Some(root_index) = cache.root_index {
        shortest_root_path(cache, root_index, selected_index)
    } else {
        (HashSet::new(), HashSet::new())
    };

    HighlightState {
        related_nodes,
        related_edges,
        root_path_nodes,
        root_path_edges,
    }
}

fn build_highlight_state_for_selected_id(
    graph: &SystemGraph,
    cache: &RenderGraph,
    selected_id: &str,
) -> Option<HighlightState> {
    if !graph.nodes.contains_key(selected_id) {
        return None;
    }

    let mut related_nodes = HashSet::new();
    let mut related_edges = HashSet::new();

    collect_related_paths_by_id(
        graph,
        selected_id,
        true,
        &cache.index_by_id,
        &mut related_nodes,
        &mut related_edges,
    );
    collect_related_paths_by_id(
        graph,
        selected_id,
        false,
        &cache.index_by_id,
        &mut related_nodes,
        &mut related_edges,
    );

    let mut root_path_nodes = HashSet::new();
    let mut root_path_edges = HashSet::new();
    if let Some(path) = graph.shortest_path_from_root(selected_id) {
        for id in &path {
            if let Some(&index) = cache.index_by_id.get(id) {
                root_path_nodes.insert(index);
            }
        }

        for pair in path.windows(2) {
            if let [source_id, target_id] = pair {
                if let (Some(&source), Some(&target)) = (
                    cache.index_by_id.get(source_id),
                    cache.index_by_id.get(target_id),
                ) {
                    root_path_edges.insert((source, target));
                }
            }
        }
    }

    Some(HighlightState {
        related_nodes,
        related_edges,
        root_path_nodes,
        root_path_edges,
    })
}

fn collect_related_paths_by_id(
    graph: &SystemGraph,
    selected_id: &str,
    forward: bool,
    index_by_id: &HashMap<String, usize>,
    related_nodes: &mut HashSet<usize>,
    related_edges: &mut HashSet<(usize, usize)>,
) {
    const RELATED_DEPTH: usize = 1;
    const RELATED_NODE_LIMIT: usize = 280;

    let mut queue = VecDeque::from([(selected_id.to_string(), 0usize)]);
    let mut visited = HashSet::from([selected_id.to_string()]);

    while let Some((node_id, depth)) = queue.pop_front() {
        if depth >= RELATED_DEPTH {
            continue;
        }

        let Some(node) = graph.nodes.get(&node_id) else {
            continue;
        };
        let neighbors = if forward {
            &node.references
        } else {
            &node.referrers
        };

        for next_id in neighbors.iter().take(160) {
            let (source_id, target_id) = if forward {
                (node_id.as_str(), next_id.as_str())
            } else {
                (next_id.as_str(), node_id.as_str())
            };

            let source_index = index_by_id.get(source_id).copied();
            let target_index = index_by_id.get(target_id).copied();

            if let Some(source) = source_index {
                related_nodes.insert(source);
            }
            if let Some(target) = target_index {
                related_nodes.insert(target);
            }

            if let (Some(source), Some(target)) = (source_index, target_index) {
                related_edges.insert((source, target));
            }

            if related_nodes.len() >= RELATED_NODE_LIMIT {
                return;
            }

            if visited.insert(next_id.clone()) {
                queue.push_back((next_id.clone(), depth + 1));
            }
        }
    }
}

fn collect_related_paths(
    adjacency: &[Vec<usize>],
    selected_index: usize,
    forward: bool,
    related_nodes: &mut HashSet<usize>,
    related_edges: &mut HashSet<(usize, usize)>,
) {
    const RELATED_DEPTH: usize = 1;
    const RELATED_NODE_LIMIT: usize = 280;

    let mut queue = VecDeque::from([(selected_index, 0usize)]);
    let mut visited = HashSet::from([selected_index]);

    while let Some((node, depth)) = queue.pop_front() {
        if depth >= RELATED_DEPTH {
            continue;
        }

        for &next in adjacency[node].iter().take(160) {
            let edge = if forward { (node, next) } else { (next, node) };
            related_edges.insert(edge);

            let inserted = related_nodes.insert(next);
            if inserted && related_nodes.len() >= RELATED_NODE_LIMIT {
                return;
            }

            if visited.insert(next) {
                queue.push_back((next, depth + 1));
            }
        }
    }
}

fn shortest_root_path(
    cache: &RenderGraph,
    root_index: usize,
    target_index: usize,
) -> (HashSet<usize>, HashSet<(usize, usize)>) {
    if root_index >= cache.nodes.len() || target_index >= cache.nodes.len() {
        return (HashSet::new(), HashSet::new());
    }

    let mut path_nodes = HashSet::new();
    let mut path_edges = HashSet::new();

    if root_index == target_index {
        path_nodes.insert(root_index);
        return (path_nodes, path_edges);
    }

    let mut queue = VecDeque::from([root_index]);
    let mut visited = vec![false; cache.nodes.len()];
    let mut parent = vec![usize::MAX; cache.nodes.len()];
    visited[root_index] = true;

    while let Some(node) = queue.pop_front() {
        if node == target_index {
            break;
        }

        for &next in &cache.outgoing[node] {
            if !visited[next] {
                visited[next] = true;
                parent[next] = node;
                queue.push_back(next);
            }
        }
    }

    if !visited[target_index] {
        return (path_nodes, path_edges);
    }

    let mut cursor = target_index;
    path_nodes.insert(cursor);

    while cursor != root_index {
        let prev = parent[cursor];
        if prev == usize::MAX {
            break;
        }

        path_edges.insert((prev, cursor));
        path_nodes.insert(prev);
        cursor = prev;
    }

    (path_nodes, path_edges)
}

fn blend_color(base: Color32, overlay: Color32, amount: f32) -> Color32 {
    let amount = amount.clamp(0.0, 1.0);
    let inverse = 1.0 - amount;

    Color32::from_rgba_unmultiplied(
        ((base.r() as f32 * inverse) + (overlay.r() as f32 * amount)) as u8,
        ((base.g() as f32 * inverse) + (overlay.g() as f32 * amount)) as u8,
        ((base.b() as f32 * inverse) + (overlay.b() as f32 * amount)) as u8,
        ((base.a() as f32 * inverse) + (overlay.a() as f32 * amount)) as u8,
    )
}

fn dim_color(color: Color32, factor: f32) -> Color32 {
    let factor = factor.clamp(0.0, 1.0);
    Color32::from_rgba_unmultiplied(
        (color.r() as f32 * factor) as u8,
        (color.g() as f32 * factor) as u8,
        (color.b() as f32 * factor) as u8,
        (color.a() as f32 * (0.45 + (factor * 0.55))) as u8,
    )
}

fn step_physics(cache: &mut RenderGraph, config: PhysicsConfig) {
    let node_count = cache.nodes.len();
    if node_count < 2 {
        return;
    }

    let mut forces = vec![Vec2::ZERO; node_count];
    let intensity = config.intensity.clamp(0.2, 2.5);
    let repulsion_strength = 78_000.0 * intensity * config.repulsion_scale.clamp(0.25, 2.6);
    let spring_strength = 0.016 * intensity * config.spring_scale.clamp(0.2, 2.2);
    let spring_damping = 0.22;
    let collision_strength = 1.9 * intensity * config.collision_scale.clamp(0.2, 2.0);
    let center_pull = 0.00055;
    let root_pull = 0.022;
    let damping = (config.velocity_damping - (intensity * 0.015)).clamp(0.78, 0.97);
    let softening = 620.0;

    for i in 0..node_count {
        for j in (i + 1)..node_count {
            let delta = cache.nodes[i].world_pos - cache.nodes[j].world_pos;
            let distance_sq = delta.length_sq();
            let distance = distance_sq.sqrt();
            let direction = if distance > 0.0001 {
                delta / distance
            } else {
                let angle =
                    ((i as f32) * 0.618_034 + (j as f32) * 0.414_214) * std::f32::consts::TAU;
                vec2(angle.cos(), angle.sin())
            };

            let min_distance = (cache.nodes[i].base_radius + cache.nodes[j].base_radius) * 4.2;
            let repulsion = repulsion_strength / (distance_sq + softening);

            forces[i] += direction * repulsion;
            forces[j] -= direction * repulsion;

            if distance < min_distance {
                let overlap_push = (min_distance - distance) * collision_strength;
                forces[i] += direction * overlap_push;
                forces[j] -= direction * overlap_push;
            }
        }
    }

    for &(from, to) in &cache.edges {
        if from >= node_count || to >= node_count || from == to {
            continue;
        }

        let delta = cache.nodes[from].world_pos - cache.nodes[to].world_pos;
        let distance = delta.length();
        if distance <= 0.0001 {
            continue;
        }
        let direction = delta / distance;

        let preferred = 96.0 + (cache.nodes[from].base_radius + cache.nodes[to].base_radius) * 4.0;
        let spring = (distance - preferred) * spring_strength;
        let relative_velocity = cache.nodes[from].velocity - cache.nodes[to].velocity;
        let damping_force = relative_velocity.dot(direction) * spring_damping;
        let correction = direction * (spring + damping_force);

        forces[from] -= correction;
        forces[to] += correction;
    }

    for index in 0..node_count {
        forces[index] -= cache.nodes[index].world_pos * center_pull;
    }

    let target_radius = (node_count as f32).sqrt() * 42.0 * config.target_spread.clamp(0.6, 2.0);
    let spread_force = config.spread_force.clamp(0.0, 0.08) * intensity;
    if spread_force > 0.0 {
        let root_index = cache.root_index.filter(|index| *index < node_count);
        let mut radius_sum = 0.0;
        let mut radius_count = 0usize;
        for (index, node) in cache.nodes.iter().enumerate() {
            if root_index.is_some_and(|root| root == index) {
                continue;
            }
            radius_sum += node.world_pos.length();
            radius_count += 1;
        }

        if radius_count > 0 {
            let average_radius = radius_sum / radius_count as f32;
            let radius_error = average_radius - target_radius;
            let hard_limit = target_radius * 1.55;
            for index in 0..node_count {
                if root_index.is_some_and(|root| root == index) {
                    continue;
                }

                let position = cache.nodes[index].world_pos;
                let radius = position.length();
                let direction = if radius > 0.0001 {
                    position / radius
                } else {
                    let angle = ((index as f32) * 0.618_034 + 0.37) * std::f32::consts::TAU;
                    vec2(angle.cos(), angle.sin())
                };

                forces[index] -= direction * radius_error * spread_force;
                if radius > hard_limit {
                    forces[index] -=
                        direction * (radius - hard_limit) * ((spread_force * 2.6) + 0.02);
                }
            }
        }
    }

    if let Some(root_index) = cache.root_index {
        if root_index < node_count {
            forces[root_index] -= cache.nodes[root_index].world_pos * root_pull;
        }
    }

    let max_force = 165.0 + (intensity * 90.0);
    let max_speed = 11.0 + (intensity * 15.0);
    for index in 0..node_count {
        let mut force = forces[index];
        let mut force_magnitude = force.length();
        if force_magnitude > max_force {
            force = force / force_magnitude * max_force;
            force_magnitude = max_force;
        }

        let mut velocity = (cache.nodes[index].velocity + (force * 0.055)) * damping;
        let mut speed = velocity.length();
        if speed > max_speed {
            velocity = velocity / speed * max_speed;
            speed = max_speed;
        }

        if speed < 0.02 && force_magnitude < 0.08 {
            velocity = Vec2::ZERO;
        }

        cache.nodes[index].velocity = velocity;
        cache.nodes[index].world_pos += velocity;
    }
}

fn draw_background(painter: &Painter, rect: Rect, pan: Vec2, zoom: f32) {
    painter.rect_filled(rect, 0.0, Color32::from_rgb(19, 23, 29));

    let step = (56.0 * zoom.clamp(0.6, 1.8)).max(20.0);
    let origin = rect.center() + pan;

    let mut x = origin.x.rem_euclid(step);
    while x < rect.right() {
        painter.line_segment(
            [Pos2::new(x, rect.top()), Pos2::new(x, rect.bottom())],
            Stroke::new(1.0, Color32::from_rgba_unmultiplied(60, 70, 80, 70)),
        );
        x += step;
    }

    let mut y = origin.y.rem_euclid(step);
    while y < rect.bottom() {
        painter.line_segment(
            [Pos2::new(rect.left(), y), Pos2::new(rect.right(), y)],
            Stroke::new(1.0, Color32::from_rgba_unmultiplied(60, 70, 80, 70)),
        );
        y += step;
    }
}

fn circle_visible(rect: Rect, position: Pos2, radius: f32) -> bool {
    !(position.x + radius < rect.left()
        || position.x - radius > rect.right()
        || position.y + radius < rect.top()
        || position.y - radius > rect.bottom())
}

fn edge_visible(rect: Rect, start: Pos2, end: Pos2, padding: f32) -> bool {
    let min_x = start.x.min(end.x) - padding;
    let max_x = start.x.max(end.x) + padding;
    let min_y = start.y.min(end.y) - padding;
    let max_y = start.y.max(end.y) + padding;

    if max_x < rect.left() || min_x > rect.right() || max_y < rect.top() || min_y > rect.bottom() {
        return false;
    }

    if rect.contains(start) || rect.contains(end) {
        return true;
    }

    let top_left = rect.left_top();
    let top_right = rect.right_top();
    let bottom_left = rect.left_bottom();
    let bottom_right = rect.right_bottom();

    segments_intersect(start, end, top_left, top_right)
        || segments_intersect(start, end, top_right, bottom_right)
        || segments_intersect(start, end, bottom_right, bottom_left)
        || segments_intersect(start, end, bottom_left, top_left)
}

fn segments_intersect(a1: Pos2, a2: Pos2, b1: Pos2, b2: Pos2) -> bool {
    fn cross(o: Pos2, a: Pos2, b: Pos2) -> f32 {
        let oa = a - o;
        let ob = b - o;
        (oa.x * ob.y) - (oa.y * ob.x)
    }

    let a_min_x = a1.x.min(a2.x);
    let a_max_x = a1.x.max(a2.x);
    let a_min_y = a1.y.min(a2.y);
    let a_max_y = a1.y.max(a2.y);
    let b_min_x = b1.x.min(b2.x);
    let b_max_x = b1.x.max(b2.x);
    let b_min_y = b1.y.min(b2.y);
    let b_max_y = b1.y.max(b2.y);

    if a_max_x < b_min_x || b_max_x < a_min_x || a_max_y < b_min_y || b_max_y < a_min_y {
        return false;
    }

    let c1 = cross(a1, a2, b1);
    let c2 = cross(a1, a2, b2);
    let c3 = cross(b1, b2, a1);
    let c4 = cross(b1, b2, a2);

    (c1 <= 0.0 && c2 >= 0.0 || c1 >= 0.0 && c2 <= 0.0)
        && (c3 <= 0.0 && c4 >= 0.0 || c3 >= 0.0 && c4 <= 0.0)
}

fn world_to_screen(rect: Rect, pan: Vec2, zoom: f32, world: Vec2) -> Pos2 {
    rect.center() + pan + world * zoom
}

fn screen_to_world(rect: Rect, pan: Vec2, zoom: f32, screen: Pos2) -> Vec2 {
    (screen - rect.center() - pan) / zoom
}

fn normalize_log(value: u64, min: u64, max: u64) -> f32 {
    let min = min.max(1) as f64;
    let max = max.max(min as u64) as f64;
    let value = value.max(1) as f64;

    if (max - min).abs() < f64::EPSILON {
        return 0.5;
    }

    let denominator = max.ln() - min.ln();
    if denominator.abs() < f64::EPSILON {
        return 0.5;
    }

    ((value.ln() - min.ln()) / denominator).clamp(0.0, 1.0) as f32
}

fn node_radius(metric: u64, min: u64, max: u64) -> f32 {
    6.0 + (normalize_log(metric, min, max) * 26.0)
}

fn metric_color(metric: u64, min: u64, max: u64) -> Color32 {
    let t = normalize_log(metric, min, max);
    let r = (55.0 + (190.0 * t)) as u8;
    let g = (150.0 - (70.0 * t)) as u8;
    let b = (215.0 - (155.0 * t)) as u8;
    Color32::from_rgb(r, g, b)
}
