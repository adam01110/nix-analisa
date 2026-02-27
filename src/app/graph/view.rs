use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use eframe::egui::{self, Align2, Color32, FontId, Sense, Stroke, Ui, Vec2, vec2};
use fuzzy_matcher::FuzzyMatcher;
use fuzzy_matcher::skim::SkimMatcherV2;

use crate::util::short_name;

use super::super::highlight::build_highlight_state_for_selected_id;
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
    fn update_screen_space(
        rect: egui::Rect,
        pan: Vec2,
        zoom: f32,
        cache: &mut super::super::RenderGraph,
    ) {
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
                pan,
                zoom,
                render_node.world_pos,
            ));
            cache
                .view_scratch
                .screen_radii
                .push((render_node.base_radius * zoom.powf(0.40)).clamp(2.5, 46.0));
        }
    }

    fn ensure_draw_order(cache: &mut super::super::RenderGraph) {
        if !cache.view_scratch.draw_order_dirty
            && cache.view_scratch.draw_order.len() == cache.nodes.len()
        {
            return;
        }

        cache.view_scratch.draw_order.clear();
        cache.view_scratch.draw_order.extend(0..cache.nodes.len());
        cache.view_scratch.draw_order.sort_by(|a, b| {
            cache.nodes[*a]
                .metric_value
                .cmp(&cache.nodes[*b].metric_value)
        });
        cache.view_scratch.draw_order_dirty = false;
    }

    fn cached_pseudo_matches(&mut self) -> Option<Arc<HashSet<usize>>> {
        if self.selected.is_some() {
            return None;
        }

        let search_query = self.search.trim();
        if search_query.is_empty() {
            return None;
        }

        if let Some(cached) = &self.search_match_cache
            && cached.graph_revision == self.render_graph_revision
            && cached.query == search_query
        {
            return Some(Arc::clone(&cached.matches));
        }

        let cache = self.graph_cache.as_ref()?;
        let matcher = SkimMatcherV2::default();
        let matches = cache
            .nodes
            .iter()
            .enumerate()
            .filter_map(|(index, node)| {
                if fuzzy_match_score(&matcher, short_name(&node.id), search_query).is_some() {
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

        let pseudo_matches = self.cached_pseudo_matches();
        let pan = self.pan;
        let zoom = self.zoom;
        let show_quadtree_overlay = self.show_quadtree_overlay;
        let interaction_active = response.dragged();
        let frame_delta_seconds = ui
            .ctx()
            .input(|input| input.stable_dt)
            .clamp(1.0 / 240.0, 1.0 / 20.0);
        let physics = PhysicsConfig {
            intensity: self.physics_intensity,
            repulsion_scale: self.physics_repulsion,
            spring_scale: self.physics_spring,
            collision_scale: self.physics_collision,
            velocity_damping: self.physics_velocity_damping,
            target_spread: self.physics_target_spread,
            spread_force: self.physics_spread_force,
            delta_seconds: frame_delta_seconds,
        };

        let Some(cache) = self.graph_cache.as_mut() else {
            self.visible_node_count = 0;
            self.visible_edge_count = 0;
            ui.label("No nodes matched the current size/node filters.");
            return;
        };

        let mut physics_moving = false;
        if self.live_physics {
            physics_moving = step_physics(cache, physics);
        }

        if physics_moving || interaction_active {
            ui.ctx().request_repaint();
        }

        Self::update_screen_space(rect, pan, zoom, cache);
        Self::visible_indices_into(
            rect,
            &cache.view_scratch.screen_positions,
            &cache.view_scratch.screen_radii,
            &mut cache.view_scratch.visible_indices,
        );
        cache.view_scratch.visible_mask.clear();
        cache
            .view_scratch
            .visible_mask
            .resize(cache.nodes.len(), false);
        for &index in &cache.view_scratch.visible_indices {
            if let Some(entry) = cache.view_scratch.visible_mask.get_mut(index) {
                *entry = true;
            }
        }
        self.visible_node_count = cache.view_scratch.visible_indices.len();

        if show_quadtree_overlay {
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
                let line_width: f32 =
                    (1.4_f32 - (cell.depth as f32 * 0.09_f32)).clamp(0.45_f32, 1.4_f32);
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
        let highlight = self
            .selected
            .as_ref()
            .and_then(|id| build_highlight_state_for_selected_id(&self.graph, cache, id));
        let selection_active = highlight.as_ref().is_some_and(|state| {
            !state.related_nodes.is_empty()
                || !state.related_edges.is_empty()
                || !state.root_path_nodes.is_empty()
                || !state.root_path_edges.is_empty()
        });
        let pseudo_active = pseudo_matches
            .as_ref()
            .is_some_and(|matches| !matches.is_empty());

        let zoom_sqrt = self.zoom.sqrt();
        let edge_detail = ((self.zoom - 0.35) / 0.95).clamp(0.0, 1.0);
        let short_edge_min_length = 2.0 + (1.0 - edge_detail) * 3.0;
        let short_edge_min_length_sq = short_edge_min_length * short_edge_min_length;
        let low_zoom_edge_stride = if self.zoom < 0.35 {
            3usize
        } else if self.zoom < 0.55 {
            2usize
        } else {
            1usize
        };
        let density_cell_size = (28.0 + (1.0 - edge_detail) * 20.0).clamp(28.0, 52.0);
        let mut edge_density_by_cell: HashMap<u64, u16> = HashMap::new();
        for &(src, dst) in &cache.edges {
            if src >= cache.nodes.len() || dst >= cache.nodes.len() {
                continue;
            }

            let start = cache.view_scratch.screen_positions[src];
            let end = cache.view_scratch.screen_positions[dst];
            let src_visible = cache
                .view_scratch
                .visible_mask
                .get(src)
                .copied()
                .unwrap_or(false);
            let dst_visible = cache
                .view_scratch
                .visible_mask
                .get(dst)
                .copied()
                .unwrap_or(false);
            if !src_visible && !dst_visible && !edge_visible(rect, start, end, 2.5) {
                continue;
            }

            let mid = start + (end - start) * 0.5;
            let cell_x = ((mid.x - rect.left()) / density_cell_size).floor() as i32;
            let cell_y = ((mid.y - rect.top()) / density_cell_size).floor() as i32;
            let key = ((cell_x as u32 as u64) << 32) | (cell_y as u32 as u64);
            let entry = edge_density_by_cell.entry(key).or_insert(0);
            *entry = entry.saturating_add(1);
        }

        let mut visible_edge_count = 0usize;
        for &(src, dst) in &cache.edges {
            if src >= cache.nodes.len() || dst >= cache.nodes.len() {
                continue;
            }

            let start = cache.view_scratch.screen_positions[src];
            let end = cache.view_scratch.screen_positions[dst];
            let src_visible = cache
                .view_scratch
                .visible_mask
                .get(src)
                .copied()
                .unwrap_or(false);
            let dst_visible = cache
                .view_scratch
                .visible_mask
                .get(dst)
                .copied()
                .unwrap_or(false);
            if !src_visible && !dst_visible && !edge_visible(rect, start, end, 2.5) {
                continue;
            }

            let (is_root_path_edge, is_related_edge) = if let Some(state) = &highlight {
                (
                    state.root_path_edges.contains(&(src, dst)),
                    state.related_edges.contains(&(src, dst)),
                )
            } else {
                (false, false)
            };

            let highlighted_edge = is_root_path_edge || is_related_edge;
            if !highlighted_edge {
                let mid = start + (end - start) * 0.5;
                let cell_x = ((mid.x - rect.left()) / density_cell_size).floor() as i32;
                let cell_y = ((mid.y - rect.top()) / density_cell_size).floor() as i32;
                let key = ((cell_x as u32 as u64) << 32) | (cell_y as u32 as u64);
                let density = edge_density_by_cell.get(&key).copied().unwrap_or(0) as usize;

                let mut density_stride = if density > 110 {
                    4usize
                } else if density > 70 {
                    3usize
                } else if density > 40 {
                    2usize
                } else {
                    1usize
                };
                if edge_detail > 0.88 {
                    density_stride = density_stride.min(2);
                } else if edge_detail > 0.70 {
                    density_stride = density_stride.min(3);
                }

                let stride = density_stride.max(low_zoom_edge_stride);
                if stride > 1 {
                    let edge_hash = src.wrapping_mul(31) ^ dst.wrapping_mul(131);
                    if edge_hash % stride != 0 {
                        continue;
                    }
                }

                let apply_length_filter = density > 40 || self.zoom < 0.38;
                if apply_length_filter && (end - start).length_sq() < short_edge_min_length_sq {
                    continue;
                }
            }

            let (line_width, line_color) = if is_root_path_edge {
                (
                    (3.3 * zoom_sqrt).clamp(1.7, 5.8),
                    Color32::from_rgb(246, 206, 104),
                )
            } else if is_related_edge {
                (
                    (2.5 * zoom_sqrt).clamp(1.2, 4.4),
                    Color32::from_rgb(241, 146, 94),
                )
            } else if highlight.is_some() {
                let edge_alpha = (120.0 + edge_detail * 48.0) as u8;
                (
                    (0.82 * zoom_sqrt).clamp(0.45, 2.0),
                    Color32::from_rgba_unmultiplied(80, 90, 104, edge_alpha),
                )
            } else {
                let edge_alpha = (160.0 + edge_detail * 56.0) as u8;
                (
                    (1.18 * zoom_sqrt).clamp(0.60, 3.4),
                    Color32::from_rgba_unmultiplied(72, 72, 72, edge_alpha),
                )
            };

            painter.line_segment([start, end], Stroke::new(line_width, line_color));
            visible_edge_count += 1;
        }
        self.visible_edge_count = visible_edge_count;

        let selected_color = Color32::from_rgb(245, 206, 93);
        let mut selection_animating = false;

        Self::ensure_draw_order(cache);
        for index in cache.view_scratch.draw_order.iter().copied() {
            if !cache
                .view_scratch
                .visible_mask
                .get(index)
                .copied()
                .unwrap_or(false)
            {
                continue;
            }

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
            let unselected_color = if is_hovered {
                Color32::from_rgb(255, 164, 101)
            } else if is_root_path {
                blend_color(base_color, Color32::from_rgb(247, 194, 111), 0.72)
            } else if is_related {
                blend_color(base_color, Color32::from_rgb(246, 137, 92), 0.60)
            } else if is_pseudo_match {
                blend_color(base_color, Color32::from_rgb(103, 196, 255), 0.68)
            } else if selection_active {
                dim_color(base_color, 0.52)
            } else if pseudo_active {
                dim_color(base_color, 0.38)
            } else {
                base_color
            };

            let selection_mix = ui.ctx().animate_bool(
                ui.make_persistent_id(("node-selection", render_node.id.as_str())),
                is_selected,
            );
            if selection_mix > 0.0 && selection_mix < 1.0 {
                selection_animating = true;
            }

            let color = blend_color(unselected_color, selected_color, selection_mix);

            painter.circle_filled(position, radius, color);
            if selection_mix > 0.0 {
                let halo_strength = (selection_mix * (1.0 - selection_mix) * 4.0).clamp(0.0, 1.0);
                let halo_alpha = (30.0 + (halo_strength * 145.0)) as u8;
                painter.circle_stroke(
                    position,
                    radius + 4.0 + ((1.0 - selection_mix) * 6.0),
                    Stroke::new(
                        1.0 + (halo_strength * 1.6),
                        Color32::from_rgba_unmultiplied(245, 206, 93, halo_alpha),
                    ),
                );
            }

            let stroke_width = if is_root_path {
                1.8
            } else if is_pseudo_match {
                1.55
            } else {
                1.0
            } + (selection_mix * 1.2);
            painter.circle_stroke(
                position,
                radius,
                Stroke::new(
                    stroke_width,
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

        if selection_animating {
            ui.ctx().request_repaint();
        }

        if let Some((hovered_index, _)) = hovered
            && let Some(node) = self.graph.nodes.get(&cache.nodes[hovered_index].id)
        {
            let panel_text = format!(
                "{}  |  {}  |  refs {}",
                short_name(&node.id),
                Self::format_metric_value(self.metric, node.metric(self.metric)),
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

        if let Some(selected) = pending_selection {
            self.apply_graph_selection(selected);
        }
    }
}
