use eframe::egui::{self, Align, Key, Layout, Response, Ui};

use crate::nix::SizeMetric;
use crate::util::short_name;

use super::super::{DependencyRankingMode, SizeRankingMode, ViewModel};

const SLIDER_KEY_BASE_RATE: f32 = 10.0;
const SLIDER_KEY_ACCEL_PER_SEC: f32 = 9.0;
const SLIDER_KEY_ACCEL_MAX: f32 = 40.0;

#[derive(Clone, Copy, Default)]
struct SliderKeyHoldState {
    positive_secs: f32,
    negative_secs: f32,
    integer_carry: f32,
}

fn slider_key_accel_multiplier(hold_secs: f32) -> f32 {
    let ramp = hold_secs * SLIDER_KEY_ACCEL_PER_SEC;
    (1.0 + ramp + ramp * ramp * 0.15).min(SLIDER_KEY_ACCEL_MAX)
}

fn default_slider_key_step(min: f32, max: f32) -> f32 {
    ((max - min) / 200.0).max(0.0005)
}

fn apply_slider_arrow_acceleration_f32(
    ui: &Ui,
    response: &Response,
    value: &mut f32,
    min: f32,
    max: f32,
    step: f32,
) -> bool {
    let state_id = response.id.with("arrow_key_hold_state");
    let mut hold_state = ui.ctx().data(|data| {
        data.get_temp::<SliderKeyHoldState>(state_id)
            .unwrap_or_default()
    });

    if !response.has_focus() {
        hold_state = SliderKeyHoldState::default();
        ui.ctx()
            .data_mut(|data| data.insert_temp(state_id, hold_state));
        return false;
    }

    let (delta_time, increase_down, decrease_down) = ui.input(|input| {
        (
            input.stable_dt.min(0.1),
            input.key_down(Key::ArrowRight) || input.key_down(Key::ArrowUp),
            input.key_down(Key::ArrowLeft) || input.key_down(Key::ArrowDown),
        )
    });

    if increase_down {
        hold_state.positive_secs += delta_time;
    } else {
        hold_state.positive_secs = 0.0;
    }

    if decrease_down {
        hold_state.negative_secs += delta_time;
    } else {
        hold_state.negative_secs = 0.0;
    }

    let direction = (increase_down as i8) - (decrease_down as i8);
    if direction == 0 {
        ui.ctx()
            .data_mut(|data| data.insert_temp(state_id, hold_state));
        return false;
    }

    let hold_secs = if direction > 0 {
        hold_state.positive_secs
    } else {
        hold_state.negative_secs
    };
    let speed = SLIDER_KEY_BASE_RATE * slider_key_accel_multiplier(hold_secs);
    let delta = direction as f32 * step * speed * delta_time;

    let old_value = *value;
    *value = (*value + delta).clamp(min, max);
    let changed = (*value - old_value).abs() > f32::EPSILON;

    if increase_down || decrease_down {
        ui.ctx().request_repaint();
    }

    ui.ctx()
        .data_mut(|data| data.insert_temp(state_id, hold_state));
    changed
}

fn apply_slider_arrow_acceleration_usize(
    ui: &Ui,
    response: &Response,
    value: &mut usize,
    min: usize,
    max: usize,
    step: usize,
) -> bool {
    let state_id = response.id.with("arrow_key_hold_state");
    let mut hold_state = ui.ctx().data(|data| {
        data.get_temp::<SliderKeyHoldState>(state_id)
            .unwrap_or_default()
    });

    if !response.has_focus() {
        hold_state = SliderKeyHoldState::default();
        ui.ctx()
            .data_mut(|data| data.insert_temp(state_id, hold_state));
        return false;
    }

    let (delta_time, increase_down, decrease_down) = ui.input(|input| {
        (
            input.stable_dt.min(0.1),
            input.key_down(Key::ArrowRight) || input.key_down(Key::ArrowUp),
            input.key_down(Key::ArrowLeft) || input.key_down(Key::ArrowDown),
        )
    });

    if increase_down {
        hold_state.positive_secs += delta_time;
    } else {
        hold_state.positive_secs = 0.0;
    }

    if decrease_down {
        hold_state.negative_secs += delta_time;
    } else {
        hold_state.negative_secs = 0.0;
    }

    let direction = (increase_down as i8) - (decrease_down as i8);
    if direction == 0 {
        hold_state.integer_carry = 0.0;
        ui.ctx()
            .data_mut(|data| data.insert_temp(state_id, hold_state));
        return false;
    }

    let hold_secs = if direction > 0 {
        hold_state.positive_secs
    } else {
        hold_state.negative_secs
    };
    let speed = SLIDER_KEY_BASE_RATE * slider_key_accel_multiplier(hold_secs);
    hold_state.integer_carry += direction as f32 * step as f32 * speed * delta_time;

    let whole_delta = hold_state.integer_carry.trunc() as isize;
    hold_state.integer_carry -= whole_delta as f32;

    let old_value = *value;
    if whole_delta != 0 {
        let next = (*value as isize + whole_delta).clamp(min as isize, max as isize) as usize;
        *value = next;
    }
    let changed = *value != old_value;

    if increase_down || decrease_down {
        ui.ctx().request_repaint();
    }

    ui.ctx()
        .data_mut(|data| data.insert_temp(state_id, hold_state));
    changed
}

impl ViewModel {
    pub(in crate::app) fn draw_controls(&mut self, ui: &mut Ui) {
        ui.heading("Graph Controls");
        ui.separator();
        ui.add_space(4.0);

        let mut changed = false;
        let mut metric_changed = false;

        ui.label("Search (hash or derivation name)")
            .on_hover_text("Fuzzy-highlight matching nodes without changing the rendered graph.");
        let search_response = ui.text_edit_singleline(&mut self.search);
        search_response
            .on_hover_text("Type to pseudo-highlight matching nodes, then click one to select it.");

        ui.separator();

        ui.horizontal_wrapped(|ui| {
            let nar_changed = ui
                .selectable_value(&mut self.metric, SizeMetric::NarSize, "NAR size")
                .on_hover_text("Scale nodes and ranking by NAR size.")
                .changed();
            changed |= nar_changed;
            metric_changed |= nar_changed;

            let closure_changed = ui
                .selectable_value(&mut self.metric, SizeMetric::ClosureSize, "Closure size")
                .on_hover_text("Scale nodes and ranking by transitive closure size.")
                .changed();
            changed |= closure_changed;
            metric_changed |= closure_changed;

            let deps_changed = ui
                .selectable_value(&mut self.metric, SizeMetric::Dependencies, "Dependencies")
                .on_hover_text("Scale nodes and ranking by direct dependency count.")
                .changed();
            changed |= deps_changed;
            metric_changed |= deps_changed;

            let reverse_deps_changed = ui
                .selectable_value(
                    &mut self.metric,
                    SizeMetric::ReverseDependencies,
                    "Reverse deps",
                )
                .on_hover_text("Scale nodes and ranking by reverse dependency count.")
                .changed();
            changed |= reverse_deps_changed;
            metric_changed |= reverse_deps_changed;
        });

        ui.separator();

        let threshold_max = self.min_threshold_max();
        let threshold_label = self.min_threshold_label();
        let min_threshold_slider = ui
            .add(
                egui::Slider::new(&mut self.min_size_mb, 0.0..=threshold_max)
                    .step_by(5.0)
                    .text(threshold_label),
            )
            .on_hover_text("Hide nodes below this metric value before rendering.");
        if min_threshold_slider.hovered() {
            min_threshold_slider.request_focus();
        }
        changed |= min_threshold_slider.changed();
        changed |= apply_slider_arrow_acceleration_f32(
            ui,
            &min_threshold_slider,
            &mut self.min_size_mb,
            0.0,
            threshold_max,
            5.0,
        );

        let max_render_nodes_limit = self.graph.node_count().max(2);
        let max_nodes_slider = ui
            .add(
                egui::Slider::new(&mut self.max_nodes, 2..=max_render_nodes_limit)
                    .step_by(5.0)
                    .text("Max rendered nodes"),
            )
            .on_hover_text("Cap the number of nodes shown to keep rendering responsive.");
        if max_nodes_slider.hovered() {
            max_nodes_slider.request_focus();
        }
        changed |= max_nodes_slider.changed();
        changed |= apply_slider_arrow_acceleration_usize(
            ui,
            &max_nodes_slider,
            &mut self.max_nodes,
            2,
            max_render_nodes_limit,
            5,
        );

        ui.separator();

        ui.checkbox(&mut self.live_physics, "Live physics simulation")
            .on_hover_text("Continuously simulate layout forces while viewing the graph.");

        ui.checkbox(&mut self.show_fps_bar, "FPS Display")
            .on_hover_text("Show a live FPS readout in the header.");

        ui.checkbox(&mut self.show_quadtree_overlay, "Show quadtree overlay")
            .on_hover_text("Draw the active quadtree partitions over the graph canvas.");

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

        ui.collapsing("Physics tuning", |ui| {
            let physics_intensity_slider = ui
                .add(
                    egui::Slider::new(&mut self.physics_intensity, 0.2..=2.5)
                        .text("Intensity")
                        .clamping(egui::SliderClamping::Always),
                )
                .on_hover_text("Overall strength applied to all physics forces.");
            if physics_intensity_slider.hovered() {
                physics_intensity_slider.request_focus();
            }
            changed |= apply_slider_arrow_acceleration_f32(
                ui,
                &physics_intensity_slider,
                &mut self.physics_intensity,
                0.2,
                2.5,
                default_slider_key_step(0.2, 2.5),
            );

            let physics_repulsion_slider = ui
                .add(
                    egui::Slider::new(&mut self.physics_repulsion, 0.25..=2.6)
                        .text("Repulsion")
                        .clamping(egui::SliderClamping::Always),
                )
                .on_hover_text("How strongly nodes push away from each other.");
            if physics_repulsion_slider.hovered() {
                physics_repulsion_slider.request_focus();
            }
            changed |= apply_slider_arrow_acceleration_f32(
                ui,
                &physics_repulsion_slider,
                &mut self.physics_repulsion,
                0.25,
                2.6,
                default_slider_key_step(0.25, 2.6),
            );

            let physics_spring_slider = ui
                .add(
                    egui::Slider::new(&mut self.physics_spring, 0.2..=2.2)
                        .text("Edge spring")
                        .clamping(egui::SliderClamping::Always),
                )
                .on_hover_text("How strongly connected nodes pull toward their target distance.");
            if physics_spring_slider.hovered() {
                physics_spring_slider.request_focus();
            }
            changed |= apply_slider_arrow_acceleration_f32(
                ui,
                &physics_spring_slider,
                &mut self.physics_spring,
                0.2,
                2.2,
                default_slider_key_step(0.2, 2.2),
            );

            let physics_collision_slider = ui
                .add(
                    egui::Slider::new(&mut self.physics_collision, 0.2..=2.0)
                        .text("Collision")
                        .clamping(egui::SliderClamping::Always),
                )
                .on_hover_text("Extra separation force to prevent overlap between nearby nodes.");
            if physics_collision_slider.hovered() {
                physics_collision_slider.request_focus();
            }
            changed |= apply_slider_arrow_acceleration_f32(
                ui,
                &physics_collision_slider,
                &mut self.physics_collision,
                0.2,
                2.0,
                default_slider_key_step(0.2, 2.0),
            );

            let physics_velocity_damping_slider = ui
                .add(
                    egui::Slider::new(&mut self.physics_velocity_damping, 0.78..=0.97)
                        .text("Velocity damping")
                        .clamping(egui::SliderClamping::Always),
                )
                .on_hover_text("How quickly node movement slows each frame.");
            if physics_velocity_damping_slider.hovered() {
                physics_velocity_damping_slider.request_focus();
            }
            changed |= apply_slider_arrow_acceleration_f32(
                ui,
                &physics_velocity_damping_slider,
                &mut self.physics_velocity_damping,
                0.78,
                0.97,
                default_slider_key_step(0.78, 0.97),
            );

            let physics_target_spread_slider = ui
                .add(
                    egui::Slider::new(&mut self.physics_target_spread, 0.6..=2.0)
                        .text("Target spread")
                        .clamping(egui::SliderClamping::Always),
                )
                .on_hover_text("Preferred spacing between connected regions of the graph.");
            if physics_target_spread_slider.hovered() {
                physics_target_spread_slider.request_focus();
            }
            changed |= apply_slider_arrow_acceleration_f32(
                ui,
                &physics_target_spread_slider,
                &mut self.physics_target_spread,
                0.6,
                2.0,
                default_slider_key_step(0.6, 2.0),
            );

            let physics_spread_force_slider = ui
                .add(
                    egui::Slider::new(&mut self.physics_spread_force, 0.0..=0.08)
                        .text("Spread correction")
                        .clamping(egui::SliderClamping::Always),
                )
                .on_hover_text("How aggressively layout drift is corrected over time.");
            if physics_spread_force_slider.hovered() {
                physics_spread_force_slider.request_focus();
            }
            changed |= apply_slider_arrow_acceleration_f32(
                ui,
                &physics_spread_force_slider,
                &mut self.physics_spread_force,
                0.0,
                0.08,
                default_slider_key_step(0.0, 0.08),
            );
        });

        if changed {
            if metric_changed {
                if !self.metric.is_byte_metric() {
                    self.min_size_mb = self.min_size_mb.min(threshold_max);
                }
                self.graph_cache = None;
            }
            self.graph_dirty = true;
        }

        ui.separator();

        egui::CollapsingHeader::new("Size rankings")
            .default_open(true)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.selectable_value(
                        &mut self.size_ranking_mode,
                        SizeRankingMode::NarSize,
                        "NAR size",
                    )
                    .on_hover_text("Derivations with the highest NAR size.");
                    ui.selectable_value(
                        &mut self.size_ranking_mode,
                        SizeRankingMode::ClosureSize,
                        "Closure size",
                    )
                    .on_hover_text("Derivations with the highest transitive closure size.");
                });

                ui.add_space(6.0);

                match self.size_ranking_mode {
                    SizeRankingMode::NarSize => self.draw_metric_ranking(ui, SizeMetric::NarSize),
                    SizeRankingMode::ClosureSize => {
                        self.draw_metric_ranking(ui, SizeMetric::ClosureSize)
                    }
                }
            });

        ui.add_space(8.0);
        egui::CollapsingHeader::new("Dependency rankings")
            .default_open(true)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.selectable_value(
                        &mut self.dependency_ranking_mode,
                        DependencyRankingMode::Dependencies,
                        "Dependencies",
                    )
                    .on_hover_text("Derivations with the highest number of direct dependencies.");
                    ui.selectable_value(
                        &mut self.dependency_ranking_mode,
                        DependencyRankingMode::ReverseDependencies,
                        "Reverse dependencies",
                    )
                    .on_hover_text("Derivations referenced by the highest number of others.");
                });

                ui.add_space(6.0);

                match self.dependency_ranking_mode {
                    DependencyRankingMode::Dependencies => self.draw_dependency_ranking(ui),
                    DependencyRankingMode::ReverseDependencies => self.draw_referrer_ranking(ui),
                }
            });
    }

    fn draw_metric_ranking(&mut self, ui: &mut Ui, metric: SizeMetric) {
        let rows_visible = self.metric_rows_visible(metric);
        let ids_len = self.metric_ids(metric).len();
        let row_count = ids_len.min(rows_visible);
        let mut should_load_more = false;
        let mut selected_id = None;

        egui::ScrollArea::vertical()
            .id_salt(self.metric_ranking_scroll_id(metric))
            .max_height(180.0)
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
                    let value_label = Self::format_metric_value(metric, node.metric(metric));

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
            self.include_node_in_current_graph(&id);
            self.set_selected(Some(id));
        }

        if should_load_more && row_count < ids_len {
            self.set_metric_rows_visible(
                metric,
                (row_count + Self::RANKING_PAGE_ROWS).min(ids_len),
            );
        }
    }

    fn draw_referrer_ranking(&mut self, ui: &mut Ui) {
        let ids_len = self.reverse_dependency_ranking.len();
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
                    let Some(id) = self.reverse_dependency_ranking.get(index) else {
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
            self.include_node_in_current_graph(&id);
            self.set_selected(Some(id));
        }

        if should_load_more && row_count < ids_len {
            self.referrer_rows_visible = (row_count + Self::RANKING_PAGE_ROWS).min(ids_len);
        }
    }

    fn draw_dependency_ranking(&mut self, ui: &mut Ui) {
        let ids_len = self.dependency_ranking.len();
        let row_count = ids_len.min(self.dependency_rows_visible);
        let mut should_load_more = false;
        let mut selected_id = None;

        egui::ScrollArea::vertical()
            .id_salt("dependency_ranking_scroll")
            .max_height(180.0)
            .auto_shrink([false, false])
            .show_rows(ui, 22.0, row_count, |ui, row_range| {
                if row_range.end + Self::RANKING_PREFETCH_MARGIN >= row_count {
                    should_load_more = true;
                }

                for index in row_range {
                    let Some(id) = self.dependency_ranking.get(index) else {
                        continue;
                    };
                    let Some(node) = self.graph.nodes.get(id) else {
                        continue;
                    };

                    let is_selected = self.selected.as_deref() == Some(id.as_str());
                    let value_label = format!("{} deps", node.references.len());

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
            self.include_node_in_current_graph(&id);
            self.set_selected(Some(id));
        }

        if should_load_more && row_count < ids_len {
            self.dependency_rows_visible = (row_count + Self::RANKING_PAGE_ROWS).min(ids_len);
        }
    }

    fn metric_ids(&self, metric: SizeMetric) -> &[String] {
        match metric {
            SizeMetric::NarSize => &self.nar_ranking,
            SizeMetric::ClosureSize => &self.closure_ranking,
            SizeMetric::Dependencies => &self.dependency_ranking,
            SizeMetric::ReverseDependencies => &self.reverse_dependency_ranking,
        }
    }

    fn metric_rows_visible(&self, metric: SizeMetric) -> usize {
        match metric {
            SizeMetric::NarSize => self.nar_rows_visible,
            SizeMetric::ClosureSize => self.closure_rows_visible,
            SizeMetric::Dependencies => self.dependency_rows_visible,
            SizeMetric::ReverseDependencies => self.referrer_rows_visible,
        }
    }

    fn set_metric_rows_visible(&mut self, metric: SizeMetric, rows: usize) {
        match metric {
            SizeMetric::NarSize => self.nar_rows_visible = rows,
            SizeMetric::ClosureSize => self.closure_rows_visible = rows,
            SizeMetric::Dependencies => self.dependency_rows_visible = rows,
            SizeMetric::ReverseDependencies => self.referrer_rows_visible = rows,
        }
    }

    fn metric_ranking_scroll_id(&self, metric: SizeMetric) -> &'static str {
        match metric {
            SizeMetric::NarSize => "nar_ranking_scroll",
            SizeMetric::ClosureSize => "closure_ranking_scroll",
            SizeMetric::Dependencies => "dependency_count_ranking_scroll",
            SizeMetric::ReverseDependencies => "reverse_dependency_count_ranking_scroll",
        }
    }
}
