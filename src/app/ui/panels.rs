use std::collections::VecDeque;

use eframe::egui::{self, vec2, Align, Context, Layout, Vec2};

use crate::nix::{SizeMetric, SystemGraph};
use crate::util::{short_name, stable_pair};

use super::super::render_utils::node_radius;
use super::super::{RenderNode, ViewModel};

impl ViewModel {
    pub(in crate::app) const INITIAL_RANKING_ROWS: usize = 20;
    pub(in crate::app) const RANKING_PAGE_ROWS: usize = 20;
    pub(in crate::app) const RANKING_PREFETCH_MARGIN: usize = 4;
    pub(in crate::app) const INITIAL_RELATED_ROWS: usize = 24;
    pub(in crate::app) const RELATED_PAGE_ROWS: usize = 24;
    pub(in crate::app) const RELATED_PREFETCH_MARGIN: usize = 4;

    pub(in crate::app) fn new(graph: SystemGraph) -> Self {
        let ranking_limit = graph.node_count();
        let top_nar = graph.top_by_metric(SizeMetric::NarSize, ranking_limit);
        let top_closure = graph.top_by_metric(SizeMetric::ClosureSize, ranking_limit);
        let top_referrers = graph.top_by_referrers(ranking_limit);

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
            lazy_physics: false,
            lazy_physics_update_interval_secs: 0.0,
            lazy_physics_offscreen_accumulator_secs: 0.0,
            lazy_physics_last_tick_secs: None,
            physics_intensity: 1.0,
            physics_repulsion: 2.6,
            physics_spring: 0.2,
            physics_collision: 1.0,
            physics_velocity_damping: 0.9,
            physics_target_spread: 2.0,
            physics_spread_force: 0.08,
            show_quadtree_overlay: false,
            graph_dirty: true,
            render_graph_revision: 0,
            graph_cache: None,
            search_match_cache: None,
            details_panel_cache: None,
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

    pub(in crate::app) fn show(
        &mut self,
        ctx: &Context,
        system_path: &str,
        reload_requested: &mut bool,
        is_loading: bool,
    ) {
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
                    let reload_button =
                        ui.add_enabled(!is_loading, egui::Button::new("Reload closure"));
                    if reload_button.clicked() {
                        *reload_requested = true;
                    }
                    if ui.button("Rebuild graph").clicked() {
                        self.graph_dirty = true;
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

        egui::CentralPanel::default().show(ctx, |ui| {
            if is_loading {
                ui.vertical_centered(|ui| {
                    ui.add_space(120.0);
                    ui.heading("Loading NixOS closure graph...");
                    ui.add_space(8.0);
                    ui.spinner();
                });
            } else {
                self.draw_graph(ui);
            }
        });
    }

    pub(in crate::app) fn set_selected(&mut self, selected: Option<String>) {
        let changed = self.selected != selected;
        if !changed {
            return;
        }

        self.selected = selected;
        self.related_rows_visible = Self::INITIAL_RELATED_ROWS;
    }

    pub(in crate::app) fn include_node_in_current_graph(&mut self, node_id: &str) {
        let Some(node) = self.graph.nodes.get(node_id) else {
            return;
        };

        let Some(cache) = self.graph_cache.as_mut() else {
            return;
        };

        if cache.index_by_id.contains_key(node_id) {
            return;
        }

        let metric_value = node.metric(self.metric).max(1);
        let base_radius = node_radius(metric_value, cache.min_metric, cache.max_metric);
        let (jx, jy) = stable_pair(node_id);
        let mut direction = vec2(jx, jy);
        if direction.length_sq() <= 0.0001 {
            let angle = ((cache.nodes.len() as f32) * 0.618_034 + 0.11) * std::f32::consts::TAU;
            direction = vec2(angle.cos(), angle.sin());
        } else {
            direction = direction.normalized();
        }

        let initial_speed = 1.15 + (base_radius * 0.022);
        let new_index = cache.nodes.len();

        cache.nodes.push(RenderNode {
            id: node_id.to_owned(),
            world_pos: Vec2::ZERO,
            velocity: direction * initial_speed,
            metric_value,
            base_radius,
        });
        cache.index_by_id.insert(node_id.to_owned(), new_index);
        cache.outgoing.push(Vec::new());
        cache.incoming.push(Vec::new());

        if self.graph.root_id == node_id {
            cache.root_index = Some(new_index);
        }

        let mut add_edge = |source: usize, target: usize| {
            if source == target {
                return;
            }
            if cache
                .outgoing
                .get(source)
                .is_some_and(|neighbors| neighbors.contains(&target))
            {
                return;
            }

            cache.edges.push((source, target));
            cache.outgoing[source].push(target);
            cache.incoming[target].push(source);
        };

        for target_id in &node.references {
            if let Some(&target_index) = cache.index_by_id.get(target_id) {
                add_edge(new_index, target_index);
            }
        }

        for (source_id, source_node) in &self.graph.nodes {
            if source_id == node_id {
                continue;
            }

            let Some(&source_index) = cache.index_by_id.get(source_id) else {
                continue;
            };

            if source_node
                .references
                .iter()
                .any(|target| target == node_id)
            {
                add_edge(source_index, new_index);
            }
        }

        self.visible_node_count = cache.nodes.len();
        self.visible_edge_count = cache.edges.len();
    }
}
