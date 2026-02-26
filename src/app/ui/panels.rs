use std::collections::VecDeque;

use eframe::egui::{self, Align, Context, Layout, Vec2};

use crate::nix::{SizeMetric, SystemGraph};
use crate::util::short_name;

use super::super::ViewModel;

impl ViewModel {
    pub(in crate::app) const INITIAL_RANKING_ROWS: usize = 20;
    pub(in crate::app) const RANKING_PAGE_ROWS: usize = 20;
    pub(in crate::app) const RANKING_PREFETCH_MARGIN: usize = 4;
    pub(in crate::app) const INITIAL_RELATED_ROWS: usize = 24;
    pub(in crate::app) const RELATED_PAGE_ROWS: usize = 24;
    pub(in crate::app) const RELATED_PREFETCH_MARGIN: usize = 4;

    pub(in crate::app) fn new(graph: SystemGraph) -> Self {
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
            lazy_physics: false,
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
}
