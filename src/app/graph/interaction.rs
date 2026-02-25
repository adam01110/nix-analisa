use eframe::egui::{self, Pos2, Rect, Ui};

use super::super::render_utils::{circle_visible, screen_to_world};
use super::super::ViewModel;

impl ViewModel {
    pub(in crate::app) fn handle_graph_zoom(
        &mut self,
        ui: &Ui,
        rect: Rect,
        response: &egui::Response,
    ) {
        if !response.hovered() {
            return;
        }

        let scroll = ui.input(|input| input.raw_scroll_delta.y);
        if scroll.abs() <= f32::EPSILON {
            return;
        }

        let pointer = ui
            .input(|input| input.pointer.hover_pos())
            .unwrap_or_else(|| rect.center());
        let world_before = screen_to_world(rect, self.pan, self.zoom, pointer);

        let zoom_factor = (1.0 + (scroll * 0.0018)).clamp(0.85, 1.15);
        self.zoom = (self.zoom * zoom_factor).clamp(0.05, 6.0);
        self.pan = pointer - rect.center() - (world_before * self.zoom);
    }

    pub(in crate::app) fn handle_graph_pan(&mut self, response: &egui::Response) {
        if response.dragged_by(egui::PointerButton::Secondary)
            || response.dragged_by(egui::PointerButton::Middle)
        {
            self.pan += response.drag_delta();
        }
    }

    pub(in crate::app) fn visible_indices(
        &self,
        rect: Rect,
        screen_positions: &[Pos2],
        screen_radii: &[f32],
    ) -> Vec<usize> {
        (0..screen_positions.len())
            .filter(|&index| circle_visible(rect, screen_positions[index], screen_radii[index]))
            .collect()
    }

    pub(in crate::app) fn hovered_index(
        &self,
        ui: &Ui,
        visible_indices: &[usize],
        screen_positions: &[Pos2],
        screen_radii: &[f32],
    ) -> Option<(usize, f32)> {
        let pointer_pos = ui.input(|input| input.pointer.hover_pos());
        pointer_pos.and_then(|pointer| {
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
        })
    }

    pub(in crate::app) fn apply_graph_selection(&mut self, selected: Option<String>) {
        self.set_selected(selected);
    }
}
