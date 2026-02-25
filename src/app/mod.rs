use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::mpsc::{self, Receiver};
use std::thread;

use eframe::egui::{self, Context, Vec2};

use crate::nix::{SizeMetric, SystemGraph, collect_system_graph};

mod graph;
mod highlight;
mod physics;
mod render_utils;
mod ui;

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
