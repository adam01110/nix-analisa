use std::collections::{HashMap, HashSet};

use eframe::egui::{Vec2, vec2};

use crate::util::stable_pair;

use super::super::render_utils::node_radius;
use super::super::{PhysicsScratch, RenderGraph, RenderNode, ViewModel, ViewScratch};

impl ViewModel {
    fn filtered_node_ids(&self) -> Vec<String> {
        let threshold = (self.min_size_mb.max(0.0) * 1024.0 * 1024.0) as u64;

        let mut ranked = self
            .graph
            .nodes
            .iter()
            .filter_map(|(id, node)| {
                let metric = node.metric(self.metric);

                let always_include =
                    id == &self.graph.root_id || self.selected.as_deref() == Some(id.as_str());

                if metric >= threshold || always_include {
                    Some((metric, id.as_str()))
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();

        ranked.sort_by(|a, b| b.0.cmp(&a.0));

        let target_nodes = self.max_nodes.clamp(2, self.graph.node_count().max(2));
        let mut selected = HashSet::new();
        let mut ids = Vec::new();

        if self.graph.nodes.contains_key(&self.graph.root_id) {
            selected.insert(self.graph.root_id.as_str());
            ids.push(self.graph.root_id.clone());
        }

        if let Some(selected_id) = &self.selected
            && self.graph.nodes.contains_key(selected_id)
            && selected.insert(selected_id.as_str())
        {
            ids.push(selected_id.clone());
        }

        for (_metric, id) in ranked {
            if ids.len() >= target_nodes {
                break;
            }
            if selected.insert(id) {
                ids.push(id.to_string());
            }
        }

        ids
    }

    fn make_render_node(
        id: String,
        index: usize,
        metric_value: u64,
        base_radius: f32,
        is_root: bool,
    ) -> RenderNode {
        let (jx, jy) = stable_pair(&id);
        let mut direction = vec2(jx, jy);
        if direction.length_sq() <= 0.0001 {
            let angle = ((index as f32) * 0.618_034 + 0.11) * std::f32::consts::TAU;
            direction = vec2(angle.cos(), angle.sin());
        } else {
            direction = direction.normalized();
        }

        let initial_speed = if is_root {
            0.0
        } else {
            1.15 + (base_radius * 0.022)
        };

        RenderNode {
            id,
            world_pos: Vec2::ZERO,
            velocity: direction * initial_speed,
            metric_value,
            base_radius,
        }
    }

    fn collect_edges(
        &self,
        ids: &[String],
        index_by_id: &HashMap<String, usize>,
    ) -> Vec<(usize, usize)> {
        let mut edges = Vec::new();
        for (source_index, source_id) in ids.iter().enumerate() {
            let Some(node) = self.graph.nodes.get(source_id) else {
                continue;
            };

            for target_id in &node.references {
                if let Some(&target_index) = index_by_id.get(target_id)
                    && source_index != target_index
                {
                    edges.push((source_index, target_index));
                }
            }
        }
        edges.sort_unstable();
        edges.dedup();
        edges
    }

    pub(in crate::app) fn rebuild_render_graph(&mut self) {
        self.render_graph_revision = self.render_graph_revision.wrapping_add(1);
        self.search_match_cache = None;

        let ids = self.filtered_node_ids();

        if ids.is_empty() {
            self.graph_cache = None;
            self.visible_node_count = 0;
            self.visible_edge_count = 0;
            self.graph_dirty = false;
            return;
        }

        let mut min_metric = u64::MAX;
        let mut max_metric = 0u64;
        let mut metrics = Vec::with_capacity(ids.len());
        for id in &ids {
            let metric = self
                .graph
                .nodes
                .get(id)
                .map(|node| node.metric(self.metric).max(1))
                .unwrap_or(1);
            metrics.push(metric);
            min_metric = min_metric.min(metric);
            max_metric = max_metric.max(metric);
        }
        if min_metric == u64::MAX {
            min_metric = 1;
        }
        if max_metric < min_metric {
            max_metric = min_metric;
        }

        let node_radii = metrics
            .iter()
            .map(|metric| node_radius(*metric, min_metric, max_metric))
            .collect::<Vec<_>>();

        let mut index_by_id = HashMap::with_capacity(ids.len());
        for (index, id) in ids.iter().enumerate() {
            index_by_id.insert(id.clone(), index);
        }
        let root_index = index_by_id.get(&self.graph.root_id).copied();
        let edges = self.collect_edges(&ids, &index_by_id);

        if let Some(mut cache) = self.graph_cache.take() {
            let mut prior_nodes = cache
                .nodes
                .into_iter()
                .map(|node| (node.id.clone(), node))
                .collect::<HashMap<_, _>>();

            let mut next_nodes = Vec::with_capacity(ids.len());
            for (index, ((id, metric_value), base_radius)) in ids
                .iter()
                .zip(metrics.iter())
                .zip(node_radii.iter())
                .enumerate()
            {
                if let Some(mut node) = prior_nodes.remove(id) {
                    node.metric_value = *metric_value;
                    node.base_radius = *base_radius;
                    next_nodes.push(node);
                } else {
                    next_nodes.push(Self::make_render_node(
                        id.clone(),
                        index,
                        *metric_value,
                        *base_radius,
                        root_index.is_some_and(|root| root == index),
                    ));
                }
            }

            let mut outgoing = vec![Vec::new(); next_nodes.len()];
            let mut incoming = vec![Vec::new(); next_nodes.len()];
            for &(source, target) in &edges {
                if source < next_nodes.len() && target < next_nodes.len() {
                    outgoing[source].push(target);
                    incoming[target].push(source);
                }
            }

            cache.nodes = next_nodes;
            cache.edges = edges;
            cache.index_by_id = index_by_id;
            cache.outgoing = outgoing;
            cache.incoming = incoming;
            cache.root_index = root_index;
            cache.min_metric = min_metric;
            cache.max_metric = max_metric;
            cache.view_scratch.draw_order_dirty = true;
            self.graph_cache = Some(cache);
        } else {
            let nodes = ids
                .iter()
                .zip(metrics.iter())
                .zip(node_radii.iter())
                .enumerate()
                .map(|(index, ((id, metric_value), base_radius))| {
                    Self::make_render_node(
                        id.clone(),
                        index,
                        *metric_value,
                        *base_radius,
                        root_index.is_some_and(|root| root == index),
                    )
                })
                .collect::<Vec<_>>();

            let mut outgoing = vec![Vec::new(); nodes.len()];
            let mut incoming = vec![Vec::new(); nodes.len()];
            for &(source, target) in &edges {
                if source < nodes.len() && target < nodes.len() {
                    outgoing[source].push(target);
                    incoming[target].push(source);
                }
            }

            self.graph_cache = Some(RenderGraph {
                nodes,
                edges,
                index_by_id,
                outgoing,
                incoming,
                root_index,
                min_metric,
                max_metric,
                physics_scratch: PhysicsScratch {
                    forces: Vec::new(),
                    positions: Vec::new(),
                    radii: Vec::new(),
                },
                view_scratch: ViewScratch {
                    screen_positions: Vec::new(),
                    screen_radii: Vec::new(),
                    visible_indices: Vec::new(),
                    visible_mask: Vec::new(),
                    draw_order: Vec::new(),
                    draw_order_dirty: true,
                    quadtree_positions: Vec::new(),
                    quadtree_cells: Vec::new(),
                },
            });
        }

        if let Some(cache) = &self.graph_cache {
            self.visible_node_count = cache.nodes.len();
            self.visible_edge_count = cache.edges.len();
        }
        self.graph_dirty = false;
    }
}
