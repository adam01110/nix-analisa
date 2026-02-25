use eframe::egui::Context;

use super::super::ViewModel;

impl ViewModel {
    pub(in crate::app) fn update_fps_counter(&mut self, ctx: &Context) {
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

    pub(in crate::app) fn fps_display_text(&self) -> Option<String> {
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

    pub(in crate::app) fn visible_graph_text(&self) -> Option<String> {
        self.graph_cache.as_ref().map(|cache| {
            format!(
                "visible graph: {} nodes / {} edges",
                self.visible_node_count.min(cache.nodes.len()),
                self.visible_edge_count.min(cache.edges.len())
            )
        })
    }
}
