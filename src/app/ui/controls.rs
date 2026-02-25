use eframe::egui::{self, Align, Layout, RichText, Ui};

use crate::nix::SizeMetric;
use crate::util::{format_bytes, short_name};

use super::super::ViewModel;

impl ViewModel {
    pub(in crate::app) fn draw_controls(&mut self, ui: &mut Ui) {
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
        ui.checkbox(&mut self.lazy_physics, "Lazy physics (in-view only)")
            .on_hover_text(
                "Only simulate nodes that are currently visible on screen; off-screen nodes remain fixed.",
            );
        ui.checkbox(&mut self.show_quadtree_overlay, "Show quadtree overlay")
            .on_hover_text("Draw the active quadtree partitions over the graph canvas.");

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

        ui.label(RichText::new(format!("Top by {}", self.metric.label())).strong());
        self.draw_metric_ranking(ui, self.metric);

        ui.add_space(8.0);
        ui.label(RichText::new("Top by reverse dependencies").strong());
        self.draw_referrer_ranking(ui);
    }

    fn draw_metric_ranking(&mut self, ui: &mut Ui, metric: SizeMetric) {
        let ids_len = self.metric_ids(metric).len();
        let row_count = ids_len.min(self.metric_rows_visible);
        let mut should_load_more = false;
        let mut selected_id = None;

        egui::ScrollArea::vertical()
            .id_salt("metric_ranking_scroll")
            .max_height(240.0)
            .auto_shrink([false, false])
            .show_rows(ui, 22.0, row_count, |ui, row_range| {
                if row_range.end + Self::RANKING_PREFETCH_MARGIN >= row_count {
                    should_load_more = true;
                }

                for index in row_range {
                    let Some(id) = self.metric_ids(metric).get(index) else {
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
                        selected_id = Some(id.clone());
                    }
                }
            });

        if let Some(id) = selected_id {
            self.set_selected(Some(id));
        }

        if should_load_more && row_count < ids_len {
            self.metric_rows_visible = (row_count + Self::RANKING_PAGE_ROWS).min(ids_len);
        }
    }

    fn draw_referrer_ranking(&mut self, ui: &mut Ui) {
        let ids_len = self.top_referrers.len();
        let row_count = ids_len.min(self.referrer_rows_visible);
        let mut should_load_more = false;
        let mut selected_id = None;

        egui::ScrollArea::vertical()
            .id_salt("referrer_ranking_scroll")
            .max_height(180.0)
            .auto_shrink([false, false])
            .show_rows(ui, 22.0, row_count, |ui, row_range| {
                if row_range.end + Self::RANKING_PREFETCH_MARGIN >= row_count {
                    should_load_more = true;
                }

                for index in row_range {
                    let Some(id) = self.top_referrers.get(index) else {
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
                        selected_id = Some(id.clone());
                    }
                }
            });

        if let Some(id) = selected_id {
            self.set_selected(Some(id));
        }

        if should_load_more && row_count < ids_len {
            self.referrer_rows_visible = (row_count + Self::RANKING_PAGE_ROWS).min(ids_len);
        }
    }

    fn metric_ids(&self, metric: SizeMetric) -> &[String] {
        match metric {
            SizeMetric::NarSize => &self.top_nar,
            SizeMetric::ClosureSize => &self.top_closure,
        }
    }
}
