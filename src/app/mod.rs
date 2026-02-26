use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;

use eframe::egui::{self, Context, Pos2, Vec2};

use crate::nix::{SizeMetric, SystemGraph, collect_system_graph};

mod graph;
mod highlight;
mod physics;
mod render_utils;
mod ui;

pub struct NixAnalyzeApp {
    system_path: String,
    state: AppState,
    reload_rx: Option<Receiver<Result<SystemGraph, String>>>,
}

enum AppState {
    Loading {
        rx: Receiver<Result<SystemGraph, String>>,
    },
    Ready(Box<ViewModel>),
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
    lazy_physics: bool,
    lazy_physics_update_interval_secs: f32,
    lazy_physics_offscreen_accumulator_secs: f32,
    lazy_physics_last_tick_secs: Option<f64>,
    physics_intensity: f32,
    physics_repulsion: f32,
    physics_spring: f32,
    physics_collision: f32,
    physics_velocity_damping: f32,
    physics_target_spread: f32,
    physics_spread_force: f32,
    show_quadtree_overlay: bool,
    graph_dirty: bool,
    render_graph_revision: u64,
    graph_cache: Option<RenderGraph>,
    search_match_cache: Option<SearchMatchCache>,
    details_panel_cache: Option<DetailsPanelCache>,
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

struct SearchMatchCache {
    query: String,
    graph_revision: u64,
    matches: Arc<HashSet<usize>>,
}

struct DetailsPanelCache {
    key: DetailsPanelCacheKey,
    related_nodes: Vec<RelatedNodeEntry>,
    shortest_path_from_root: Option<Vec<String>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct DetailsPanelCacheKey {
    selected_id: String,
    metric: SizeMetric,
    render_graph_revision: u64,
    related_limit: usize,
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
    physics_scratch: PhysicsScratch,
    view_scratch: ViewScratch,
}

struct PhysicsScratch {
    forces: Vec<Vec2>,
    positions: Vec<Vec2>,
    radii: Vec<f32>,
}

struct ViewScratch {
    screen_positions: Vec<Pos2>,
    screen_radii: Vec<f32>,
    visible_indices: Vec<usize>,
    draw_order: Vec<usize>,
    quadtree_positions: Vec<Vec2>,
    quadtree_cells: Vec<physics::QuadtreeCell>,
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

#[derive(Clone)]
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
        Self {
            system_path,
            state,
            reload_rx: None,
        }
    }

    fn spawn_load(system_path: String) -> Receiver<Result<SystemGraph, String>> {
        let (tx, rx) = mpsc::channel();

        thread::spawn(move || {
            let result = collect_system_graph(&system_path).map_err(|error| error.to_string());
            let _ = tx.send(result);
        });

        rx
    }

    fn start_load(system_path: String) -> AppState {
        AppState::Loading {
            rx: Self::spawn_load(system_path),
        }
    }
}

impl eframe::App for NixAnalyzeApp {
    fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        let mut transition = None;

        match &mut self.state {
            AppState::Loading { rx } => {
                if let Ok(result) = rx.try_recv() {
                    transition = Some(match result {
                        Ok(graph) => AppState::Ready(Box::new(ViewModel::new(graph))),
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
                let is_reloading = self.reload_rx.is_some();
                model.show(ctx, &self.system_path, &mut reload_requested, is_reloading);

                if reload_requested && self.reload_rx.is_none() {
                    self.reload_rx = Some(Self::spawn_load(self.system_path.clone()));
                }

                if let Some(rx) = self.reload_rx.take() {
                    match rx.try_recv() {
                        Ok(result) => {
                            transition = Some(match result {
                                Ok(graph) => AppState::Ready(Box::new(ViewModel::new(graph))),
                                Err(error) => AppState::Error(error),
                            });
                        }
                        Err(TryRecvError::Empty) => {
                            self.reload_rx = Some(rx);
                        }
                        Err(TryRecvError::Disconnected) => {
                            transition =
                                Some(AppState::Error("Background load worker disconnected".to_owned()));
                        }
                    }
                }
            }
        }

        if let Some(next_state) = transition {
            self.reload_rx = None;
            self.state = next_state;
        }
    }
}
