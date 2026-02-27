use std::collections::{HashMap, HashSet};

use eframe::egui::{self, RichText, Ui};

use crate::util::{format_bytes, short_name};

use super::super::highlight::build_highlight_state_for_selected_id;
use super::super::{DetailsPanelCache, DetailsPanelCacheKey, RelatedNodeEntry, ViewModel};

impl ViewModel {
    pub(in crate::app) fn draw_details(&mut self, ui: &mut Ui) {
        ui.heading("Selection Details");
        ui.separator();
        ui.add_space(6.0);

        let Some(selected_id) = self.selected.clone() else {
            ui.label("Select a node from the graph or rankings.");
            return;
        };

        let Some(node) = self.graph.nodes.get(&selected_id) else {
            ui.label("Selected node no longer exists in the graph state.");
            return;
        };

        let node_short = short_name(&node.id).to_string();
        let nar_size = node.nar_size;
        let closure_size = node.closure_size;
        let reference_count = node.references.len();
        let referrer_count = node.referrers.len();
        let deriver = node.deriver.as_deref();

        ui.label(RichText::new(node_short).strong());
        ui.small(node.id.as_str());
        ui.add_space(6.0);
        ui.separator();

        ui.label(format!("Full path: {}", node.full_path));
        ui.label(format!("Node size (narSize): {}", format_bytes(nar_size)));
        ui.label(format!("Closure size: {}", format_bytes(closure_size)));
        ui.label(format!("Direct dependencies: {reference_count}"));
        ui.label(format!("Reverse dependencies: {referrer_count}"));

        if let Some(deriver) = deriver {
            ui.label(format!("Deriver: {deriver}"));
        }

        let transitive_delta = closure_size.saturating_sub(nar_size);
        ui.label(format!(
            "Transitive-only weight: {}",
            format_bytes(transitive_delta)
        ));

        let (related_nodes, shortest_path_from_root) = self.details_panel_data(&selected_id, 32);

        ui.separator();
        ui.label(RichText::new("Why this can be large").strong());
        if nar_size > (1 << 30) {
            ui.label("- large direct payload (> 1 GiB)");
        }
        if transitive_delta > nar_size.saturating_mul(2) {
            ui.label("- large transitive dependency surface");
        }
        if referrer_count > 20 {
            ui.label("- reused by many other derivations");
        }
        if nar_size <= (1 << 30)
            && transitive_delta <= nar_size.saturating_mul(2)
            && referrer_count <= 20
        {
            ui.label("- moderate local and transitive contribution");
        }

        ui.separator();
        ui.label(RichText::new("Related nodes (in and out of view)").strong());
        if related_nodes.is_empty() {
            ui.label("No related nodes found for this selection.");
        } else {
            let row_count = related_nodes.len().min(self.related_rows_visible);
            let mut should_load_more = false;

            egui::ScrollArea::vertical()
                .id_salt("related_nodes_scroll")
                .max_height(320.0)
                .auto_shrink([false, false])
                .show_rows(ui, 22.0, row_count, |ui, row_range| {
                    if row_range.end + Self::RELATED_PREFETCH_MARGIN >= row_count {
                        should_load_more = true;
                    }

                    for index in row_range {
                        let Some(related) = related_nodes.get(index) else {
                            continue;
                        };
                        let mut flags = Vec::new();
                        if related.is_in_view {
                            flags.push("in-view");
                        } else {
                            flags.push("out-of-view");
                        }
                        if related.is_root_path {
                            flags.push("path");
                        }
                        if related.is_direct {
                            flags.push("direct");
                        }

                        let label = format!(
                            "{}  ({})  [{}]",
                            short_name(&related.id),
                            Self::format_metric_value(self.metric, related.metric_value),
                            flags.join(", ")
                        );

                        if ui.link(label).on_hover_text(related.id.as_str()).clicked() {
                            self.include_node_in_current_graph(&related.id);
                            self.set_selected(Some(related.id.clone()));
                        }
                    }
                });

            if should_load_more && row_count < related_nodes.len() {
                self.related_rows_visible =
                    (row_count + Self::RELATED_PAGE_ROWS).min(related_nodes.len());
            }
        }

        ui.separator();
        ui.label(RichText::new("Why installed").strong());
        if selected_id == self.graph.root_id {
            ui.label("This is the root closure target currently being analyzed.");
        } else if let Some(path) = shortest_path_from_root {
            if let Some(parent) = path.get(path.len().saturating_sub(2)) {
                ui.label(format!(
                    "Included because {} depends on {}.",
                    short_name(parent),
                    short_name(&selected_id)
                ));
            }

            ui.separator();
            ui.label(RichText::new("Shortest dependency path from root:").strong());

            enum PathSegment<'a> {
                Node(&'a str),
                Ellipsis,
            }

            let mut segments = Vec::new();
            if path.len() <= 14 {
                for id in path.iter() {
                    segments.push(PathSegment::Node(id.as_str()));
                }
            } else {
                for id in path.iter().take(8) {
                    segments.push(PathSegment::Node(id.as_str()));
                }
                segments.push(PathSegment::Ellipsis);
                for id in path.iter().skip(path.len().saturating_sub(4)) {
                    segments.push(PathSegment::Node(id.as_str()));
                }
            }

            ui.horizontal_wrapped(|ui| {
                for (index, segment) in segments.iter().enumerate() {
                    match segment {
                        PathSegment::Node(id) => {
                            if ui.link(short_name(id)).on_hover_text(*id).clicked() {
                                self.include_node_in_current_graph(id);
                                self.set_selected(Some((*id).to_owned()));
                            }
                        }
                        PathSegment::Ellipsis => {
                            ui.weak("...");
                        }
                    }

                    if index + 1 < segments.len() {
                        ui.weak("->");
                    }
                }
            });
        } else {
            ui.label("No root-reachable path found in the current closure graph.");
        }
    }

    fn details_panel_data(
        &mut self,
        selected_id: &str,
        related_limit: usize,
    ) -> (Vec<RelatedNodeEntry>, Option<Vec<String>>) {
        let key = DetailsPanelCacheKey {
            selected_id: selected_id.to_string(),
            metric: self.metric,
            render_graph_revision: self.render_graph_revision,
            related_limit,
        };

        if let Some(cache) = &self.details_panel_cache
            && cache.key == key
        {
            return (
                cache.related_nodes.clone(),
                cache.shortest_path_from_root.clone(),
            );
        }

        let shortest_path_from_root = self.graph.shortest_path_from_root(selected_id);
        let related_nodes = self.related_nodes_for_details(
            selected_id,
            related_limit,
            shortest_path_from_root.as_deref(),
        );

        self.details_panel_cache = Some(DetailsPanelCache {
            key,
            related_nodes,
            shortest_path_from_root,
        });

        let cache = self
            .details_panel_cache
            .as_ref()
            .expect("details panel cache is initialized");
        (
            cache.related_nodes.clone(),
            cache.shortest_path_from_root.clone(),
        )
    }

    fn related_nodes_for_details(
        &self,
        selected_id: &str,
        limit: usize,
        shortest_path_from_root: Option<&[String]>,
    ) -> Vec<RelatedNodeEntry> {
        #[derive(Default)]
        struct RelatedNodeFlags {
            metric_value: u64,
            is_root_path: bool,
            is_direct: bool,
            is_in_view: bool,
        }

        if limit == 0 {
            return Vec::new();
        }

        let mut related_by_id: HashMap<String, RelatedNodeFlags> = HashMap::new();

        if let Some(node) = self.graph.nodes.get(selected_id) {
            for id in node.references.iter().chain(node.referrers.iter()) {
                if id == selected_id {
                    continue;
                }

                if let Some(related_node) = self.graph.nodes.get(id) {
                    let entry = related_by_id.entry(id.clone()).or_insert(RelatedNodeFlags {
                        metric_value: related_node.metric(self.metric),
                        ..Default::default()
                    });
                    entry.is_direct = true;
                }
            }
        }

        if let Some(path) = shortest_path_from_root {
            for id in path {
                if id == selected_id {
                    continue;
                }

                if let Some(related_node) = self.graph.nodes.get(id) {
                    let entry = related_by_id.entry(id.clone()).or_insert(RelatedNodeFlags {
                        metric_value: related_node.metric(self.metric),
                        ..Default::default()
                    });
                    entry.is_root_path = true;
                }
            }
        }

        if let Some(cache) = &self.graph_cache
            && let Some(&selected_index) = cache.index_by_id.get(selected_id)
            && let Some(highlight) =
                build_highlight_state_for_selected_id(&self.graph, cache, selected_id)
        {
            let direct_outgoing = cache
                .outgoing
                .get(selected_index)
                .into_iter()
                .flatten()
                .copied()
                .collect::<HashSet<_>>();
            let direct_incoming = cache
                .incoming
                .get(selected_index)
                .into_iter()
                .flatten()
                .copied()
                .collect::<HashSet<_>>();

            for index in highlight
                .root_path_nodes
                .iter()
                .chain(highlight.related_nodes.iter())
            {
                if *index == selected_index {
                    continue;
                }

                if let Some(render_node) = cache.nodes.get(*index) {
                    let entry =
                        related_by_id
                            .entry(render_node.id.clone())
                            .or_insert(RelatedNodeFlags {
                                metric_value: self
                                    .graph
                                    .nodes
                                    .get(&render_node.id)
                                    .map(|node| node.metric(self.metric))
                                    .unwrap_or(render_node.metric_value),
                                ..Default::default()
                            });

                    entry.is_in_view = true;
                    entry.is_root_path |= highlight.root_path_nodes.contains(index);
                    entry.is_direct |=
                        direct_outgoing.contains(index) || direct_incoming.contains(index);
                }
            }
        }

        let mut related = related_by_id
            .into_iter()
            .map(|(id, flags)| RelatedNodeEntry {
                id,
                metric_value: flags.metric_value,
                is_root_path: flags.is_root_path,
                is_direct: flags.is_direct,
                is_in_view: flags.is_in_view,
            })
            .collect::<Vec<_>>();

        related.sort_by(|a, b| {
            b.is_in_view
                .cmp(&a.is_in_view)
                .then_with(|| b.is_root_path.cmp(&a.is_root_path))
                .then_with(|| b.is_direct.cmp(&a.is_direct))
                .then_with(|| b.metric_value.cmp(&a.metric_value))
                .then_with(|| a.id.cmp(&b.id))
        });
        related.truncate(limit);
        related
    }
}
