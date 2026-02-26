use std::collections::{HashMap, HashSet, VecDeque};

use crate::nix::SystemGraph;

pub(super) fn collect_related_paths_by_id(
    graph: &SystemGraph,
    selected_id: &str,
    forward: bool,
    index_by_id: &HashMap<String, usize>,
    related_nodes: &mut HashSet<usize>,
    related_edges: &mut HashSet<(usize, usize)>,
) {
    const RELATED_DEPTH: usize = 1;
    const RELATED_NODE_LIMIT: usize = 280;

    let mut queue = VecDeque::from([(selected_id, 0usize)]);
    let mut visited = HashSet::from([selected_id]);

    while let Some((node_id, depth)) = queue.pop_front() {
        if depth >= RELATED_DEPTH {
            continue;
        }

        let Some(node) = graph.nodes.get(node_id) else {
            continue;
        };
        let neighbors = if forward {
            &node.references
        } else {
            &node.referrers
        };

        for next_id in neighbors.iter().take(160) {
            let (source_id, target_id) = if forward {
                (node_id, next_id.as_str())
            } else {
                (next_id.as_str(), node_id)
            };

            let source_index = index_by_id.get(source_id).copied();
            let target_index = index_by_id.get(target_id).copied();

            if let Some(source) = source_index {
                related_nodes.insert(source);
            }
            if let Some(target) = target_index {
                related_nodes.insert(target);
            }

            if let (Some(source), Some(target)) = (source_index, target_index) {
                related_edges.insert((source, target));
            }

            if related_nodes.len() >= RELATED_NODE_LIMIT {
                return;
            }

            let next_id = next_id.as_str();
            if visited.insert(next_id) {
                queue.push_back((next_id, depth + 1));
            }
        }
    }
}
