use std::collections::HashSet;
use std::sync::Arc;

use eframe::egui::{self, vec2, Align2, Color32, FontId, Sense, Stroke, Ui};
use fuzzy_matcher::skim::SkimMatcherV2;
use fuzzy_matcher::FuzzyMatcher;

use crate::util::{format_bytes, short_name};

use super::super::highlight::{build_highlight_state, build_highlight_state_for_selected_id};
use super::super::physics::{quadtree_cells, step_physics};
use super::super::render_utils::{
    blend_color, dim_color, draw_background, edge_visible, metric_color, world_to_screen,
};
use super::super::{PhysicsConfig, ViewModel};

fn fuzzy_match_score(matcher: &SkimMatcherV2, text: &str, query: &str) -> Option<i64> {
    matcher
        .fuzzy_match(text, query)
        .or_else(|| matcher.fuzzy_match(&text.to_ascii_lowercase(), &query.to_ascii_lowercase()))
}

impl ViewModel {
    fn cached_pseudo_matches(&mut self) -> Option<Arc<HashSet<usize>>> {
        if self.selected.is_some() {
            return None;
        }

        let search_query = self.search.trim();
        if search_query.is_empty() {
            return None;
        }

        if let Some(cached) = &self.search_match_cache {
            if cached.graph_revision == self.render_graph_revision && cached.query == search_query {
                return Some(Arc::clone(&cached.matches));
            }
        }

        let cache = self.graph_cache.as_ref()?;
        let matcher = SkimMatcherV2::default();
        let matches = cache
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
            .collect::<HashSet<_>>();
        let matches = Arc::new(matches);

        self.search_match_cache = Some(super::super::SearchMatchCache {
            query: search_query.to_owned(),
            graph_revision: self.render_graph_revision,
            matches: Arc::clone(&matches),
        });

        Some(matches)
    }

    pub(in crate::app) fn draw_graph(&mut self, ui: &mut Ui) {
        if self.graph_dirty {
            self.rebuild_render_graph();
        }

        let (rect, response) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
        let painter = ui.painter_at(rect);

        draw_background(&painter, rect, self.pan, self.zoom);

        self.handle_graph_zoom(ui, rect, &response);
        self.handle_graph_pan(&response);

        let interaction_active = response.dragged();

        let mut physics_moving = false;
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
                physics_moving = step_physics(cache, physics);
            }
        }

        if physics_moving || interaction_active {
            ui.ctx().request_repaint();
        }

        let pseudo_matches = self.cached_pseudo_matches();

        let Some(cache) = self.graph_cache.as_mut() else {
            self.visible_node_count = 0;
            self.visible_edge_count = 0;
            ui.label("No nodes matched the current size/node filters.");
            return;
        };

        cache.view_scratch.screen_positions.clear();
        cache.view_scratch.screen_positions.reserve(
            cache
                .nodes
                .len()
                .saturating_sub(cache.view_scratch.screen_positions.capacity()),
        );
        cache.view_scratch.screen_radii.clear();
        cache.view_scratch.screen_radii.reserve(
            cache
                .nodes
                .len()
                .saturating_sub(cache.view_scratch.screen_radii.capacity()),
        );
        for render_node in &cache.nodes {
            cache.view_scratch.screen_positions.push(world_to_screen(
                rect,
                self.pan,
                self.zoom,
                render_node.world_pos,
            ));
            cache
                .view_scratch
                .screen_radii
                .push((render_node.base_radius * self.zoom.powf(0.40)).clamp(2.5, 46.0));
        }

        if self.show_quadtree_overlay {
            quadtree_cells(
                &cache.nodes,
                &mut cache.view_scratch.quadtree_positions,
                &mut cache.view_scratch.quadtree_cells,
            );
            for cell in &cache.view_scratch.quadtree_cells {
                let min = cell.center - vec2(cell.half_extent, cell.half_extent);
                let max = cell.center + vec2(cell.half_extent, cell.half_extent);
                let top_left = world_to_screen(rect, self.pan, self.zoom, vec2(min.x, min.y));
                let top_right = world_to_screen(rect, self.pan, self.zoom, vec2(max.x, min.y));
                let bottom_right = world_to_screen(rect, self.pan, self.zoom, vec2(max.x, max.y));
                let bottom_left = world_to_screen(rect, self.pan, self.zoom, vec2(min.x, max.y));

                let alpha = if cell.is_leaf { 110 } else { 55 };
                let line_width = (1.4 - (cell.depth as f32 * 0.09)).clamp(0.45, 1.4);
                let stroke = Stroke::new(
                    line_width,
                    Color32::from_rgba_unmultiplied(106, 198, 255, alpha),
                );

                painter.line_segment([top_left, top_right], stroke);
                painter.line_segment([top_right, bottom_right], stroke);
                painter.line_segment([bottom_right, bottom_left], stroke);
                painter.line_segment([bottom_left, top_left], stroke);
            }
        }

        Self::visible_indices_into(
            rect,
            &cache.view_scratch.screen_positions,
            &cache.view_scratch.screen_radii,
            &mut cache.view_scratch.visible_indices,
        );
        self.visible_node_count = cache.view_scratch.visible_indices.len();

        let hovered = Self::hovered_index(
            ui,
            &cache.view_scratch.visible_indices,
            &cache.view_scratch.screen_positions,
            &cache.view_scratch.screen_radii,
        );

        if hovered.is_some() {
            ui.output_mut(|output| {
                output.cursor_icon = egui::CursorIcon::PointingHand;
            });
        }

        let pending_selection =
            if response.clicked_by(egui::PointerButton::Primary) {
                Some(hovered.and_then(|(index, _distance)| {
                    cache.nodes.get(index).map(|node| node.id.clone())
                }))
            } else {
                None
            };

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
        let pseudo_active = pseudo_matches
            .as_ref()
            .is_some_and(|matches| !matches.is_empty());

        let mut visible_edge_count = 0usize;
        for &(src, dst) in &cache.edges {
            if src >= cache.nodes.len() || dst >= cache.nodes.len() {
                continue;
            }

            let start = cache.view_scratch.screen_positions[src];
            let end = cache.view_scratch.screen_positions[dst];
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

        cache.view_scratch.draw_order.clear();
        cache
            .view_scratch
            .draw_order
            .extend(cache.view_scratch.visible_indices.iter().copied());
        cache.view_scratch.draw_order.sort_by(|a, b| {
            cache.nodes[*a]
                .metric_value
                .cmp(&cache.nodes[*b].metric_value)
        });

        for index in cache.view_scratch.draw_order.iter().copied() {
            let render_node = &cache.nodes[index];
            let position = cache.view_scratch.screen_positions[index];
            let radius = cache.view_scratch.screen_radii[index];

            let is_selected = self.selected.as_deref() == Some(render_node.id.as_str());
            let is_hovered = hovered_index == Some(index);
            let is_root_path = highlight
                .as_ref()
                .is_some_and(|state| state.root_path_nodes.contains(&index));
            let is_related = highlight
                .as_ref()
                .is_some_and(|state| state.related_nodes.contains(&index));
            let is_pseudo_match = pseudo_matches
                .as_ref()
                .is_some_and(|matches| matches.contains(&index));

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
            let should_draw_label = highlighted
                || is_hovered
                || (is_pseudo_match && self.zoom > 0.35)
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

        if let Some(selected) = pending_selection {
            self.apply_graph_selection(selected);
        }
    }
}
