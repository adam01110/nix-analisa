use std::collections::HashSet;

use crate::nix::SystemGraph;

mod collect;

use self::collect::collect_related_paths_by_id;
use super::{HighlightState, RenderGraph};

pub(super) fn build_highlight_state_for_selected_id(
    graph: &SystemGraph,
    cache: &RenderGraph,
    selected_id: &str,
) -> Option<HighlightState> {
    if !graph.nodes.contains_key(selected_id) {
        return None;
    }

    let mut related_nodes = HashSet::new();
    let mut related_edges = HashSet::new();

    if let Some(&selected_index) = cache.index_by_id.get(selected_id) {
        related_nodes.insert(selected_index);
    }

    collect_related_paths_by_id(
        graph,
        selected_id,
        true,
        &cache.index_by_id,
        &mut related_nodes,
        &mut related_edges,
    );
    collect_related_paths_by_id(
        graph,
        selected_id,
        false,
        &cache.index_by_id,
        &mut related_nodes,
        &mut related_edges,
    );

    let mut root_path_nodes = HashSet::new();
    let mut root_path_edges = HashSet::new();
    if let Some(path) = graph.shortest_path_from_root(selected_id) {
        for id in &path {
            if let Some(&index) = cache.index_by_id.get(id) {
                root_path_nodes.insert(index);
            }
        }

        for pair in path.windows(2) {
            if let [source_id, target_id] = pair
                && let (Some(&source), Some(&target)) = (
                    cache.index_by_id.get(source_id),
                    cache.index_by_id.get(target_id),
                )
            {
                root_path_edges.insert((source, target));
            }
        }
    }

    Some(HighlightState {
        related_nodes,
        related_edges,
        root_path_nodes,
        root_path_edges,
    })
}
