use std::collections::{HashSet, VecDeque};

use super::super::RenderGraph;

pub(super) fn shortest_root_path(
    cache: &RenderGraph,
    root_index: usize,
    target_index: usize,
) -> (HashSet<usize>, HashSet<(usize, usize)>) {
    if root_index >= cache.nodes.len() || target_index >= cache.nodes.len() {
        return (HashSet::new(), HashSet::new());
    }

    let mut path_nodes = HashSet::new();
    let mut path_edges = HashSet::new();

    if root_index == target_index {
        path_nodes.insert(root_index);
        return (path_nodes, path_edges);
    }

    let mut queue = VecDeque::from([root_index]);
    let mut visited = vec![false; cache.nodes.len()];
    let mut parent = vec![usize::MAX; cache.nodes.len()];
    visited[root_index] = true;

    while let Some(node) = queue.pop_front() {
        if node == target_index {
            break;
        }

        for &next in &cache.outgoing[node] {
            if !visited[next] {
                visited[next] = true;
                parent[next] = node;
                queue.push_back(next);
            }
        }
    }

    if !visited[target_index] {
        return (path_nodes, path_edges);
    }

    let mut cursor = target_index;
    path_nodes.insert(cursor);

    while cursor != root_index {
        let prev = parent[cursor];
        if prev == usize::MAX {
            break;
        }

        path_edges.insert((prev, cursor));
        path_nodes.insert(prev);
        cursor = prev;
    }

    (path_nodes, path_edges)
}
