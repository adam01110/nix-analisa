use std::collections::{HashMap, HashSet};

use eframe::egui::{self, RichText, Ui};

use crate::util::{format_bytes, short_name};

use super::super::highlight::build_highlight_state;
use super::super::{RelatedNodeEntry, ViewModel};

impl ViewModel {
    pub(in crate::app) fn draw_details(&mut self, ui: &mut Ui) {
        ui.heading("Selection Details");
        ui.add_space(6.0);

        let Some(selected_id) = self.selected.clone() else {
            ui.label("Select a node from the graph or rankings.");
            return;
        };

        let Some(node) = self.graph.nodes.get(&selected_id) else {
            ui.label("Selected node no longer exists in the graph state.");
            return;
        };

        let node_id = node.id.clone();
        let node_short = short_name(&node.id).to_string();
        let full_path = node.full_path.clone();
        let nar_size = node.nar_size;
        let closure_size = node.closure_size;
        let reference_count = node.references.len();
        let referrer_count = node.referrers.len();
        let deriver = node.deriver.clone();

        let related_nodes = self.related_nodes_for_details(&selected_id, 32);

        ui.label(RichText::new(node_short).strong());
        ui.small(node_id.as_str());
        ui.add_space(6.0);

        ui.label(format!("Full path: {full_path}"));
        ui.label(format!("Node size (narSize): {}", format_bytes(nar_size)));
        ui.label(format!("Closure size: {}", format_bytes(closure_size)));
        ui.label(format!("Direct dependencies: {reference_count}"));
        ui.label(format!("Reverse dependencies: {referrer_count}"));

        if let Some(deriver) = &deriver {
            ui.label(format!("Deriver: {deriver}"));
        }

        let transitive_delta = closure_size.saturating_sub(nar_size);
        ui.label(format!(
            "Transitive-only weight: {}",
            format_bytes(transitive_delta)
        ));

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
                            format_bytes(related.metric_value),
                            flags.join(", ")
                        );

                        if ui.link(label).on_hover_text(related.id.as_str()).clicked() {
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
        ui.label(RichText::new("Shortest path from root").strong());
        if let Some(path) = self.graph.shortest_path_from_root(&selected_id) {
            let rendered = if path.len() <= 14 {
                path.iter()
                    .map(|id| short_name(id).to_string())
                    .collect::<Vec<_>>()
                    .join(" -> ")
            } else {
                let head = path
                    .iter()
                    .take(8)
                    .map(|id| short_name(id).to_string())
                    .collect::<Vec<_>>()
                    .join(" -> ");
                let tail = path
                    .iter()
                    .skip(path.len().saturating_sub(4))
                    .map(|id| short_name(id).to_string())
                    .collect::<Vec<_>>()
                    .join(" -> ");
                format!("{head} -> ... -> {tail}")
            };
            ui.label(rendered);
        } else {
            ui.label("No root-reachable path found in the current closure graph.");
        }
    }

    fn related_nodes_for_details(&self, selected_id: &str, limit: usize) -> Vec<RelatedNodeEntry> {
        if limit == 0 {
            return Vec::new();
        }

        let mut related_by_id: HashMap<String, RelatedNodeEntry> = HashMap::new();

        if let Some(node) = self.graph.nodes.get(selected_id) {
            for id in node.references.iter().chain(node.referrers.iter()) {
                if id == selected_id {
                    continue;
                }

                if let Some(related_node) = self.graph.nodes.get(id) {
                    let entry = related_by_id.entry(id.clone()).or_insert(RelatedNodeEntry {
                        id: id.clone(),
                        metric_value: related_node.metric(self.metric),
                        is_root_path: false,
                        is_direct: false,
                        is_in_view: false,
                    });
                    entry.is_direct = true;
                }
            }
        }

        if let Some(path) = self.graph.shortest_path_from_root(selected_id) {
            for id in path {
                if id == selected_id {
                    continue;
                }

                if let Some(related_node) = self.graph.nodes.get(&id) {
                    let entry = related_by_id.entry(id.clone()).or_insert(RelatedNodeEntry {
                        id: id.clone(),
                        metric_value: related_node.metric(self.metric),
                        is_root_path: false,
                        is_direct: false,
                        is_in_view: false,
                    });
                    entry.is_root_path = true;
                }
            }
        }

        if let Some(cache) = &self.graph_cache {
            if let Some(&selected_index) = cache.index_by_id.get(selected_id) {
                let highlight = build_highlight_state(cache, selected_index);
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
                        let entry = related_by_id.entry(render_node.id.clone()).or_insert(
                            RelatedNodeEntry {
                                id: render_node.id.clone(),
                                metric_value: self
                                    .graph
                                    .nodes
                                    .get(&render_node.id)
                                    .map(|node| node.metric(self.metric))
                                    .unwrap_or(render_node.metric_value),
                                is_root_path: false,
                                is_direct: false,
                                is_in_view: false,
                            },
                        );

                        entry.is_in_view = true;
                        entry.is_root_path |= highlight.root_path_nodes.contains(index);
                        entry.is_direct |=
                            direct_outgoing.contains(index) || direct_incoming.contains(index);
                    }
                }
            }
        }

        let mut related = related_by_id.into_values().collect::<Vec<_>>();

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
