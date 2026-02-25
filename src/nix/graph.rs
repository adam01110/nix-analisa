use std::collections::{HashMap, HashSet, VecDeque};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SizeMetric {
    NarSize,
    ClosureSize,
}

impl SizeMetric {
    pub fn label(self) -> &'static str {
        match self {
            Self::NarSize => "narSize",
            Self::ClosureSize => "closureSize",
        }
    }
}

#[derive(Clone, Debug)]
pub struct NodeRecord {
    pub id: String,
    pub full_path: String,
    pub nar_size: u64,
    pub closure_size: u64,
    pub references: Vec<String>,
    pub referrers: Vec<String>,
    pub deriver: Option<String>,
}

impl NodeRecord {
    pub fn metric(&self, metric: SizeMetric) -> u64 {
        match metric {
            SizeMetric::NarSize => self.nar_size,
            SizeMetric::ClosureSize => self.closure_size,
        }
    }
}

#[derive(Clone, Debug)]
pub struct SystemGraph {
    pub store_dir: String,
    pub root_id: String,
    pub nodes: HashMap<String, NodeRecord>,
    pub edge_count: usize,
}

impl SystemGraph {
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    pub fn top_by_metric(&self, metric: SizeMetric, limit: usize) -> Vec<String> {
        let mut ids = self.nodes.keys().cloned().collect::<Vec<_>>();
        ids.sort_by(|a, b| {
            let a_node = self.nodes.get(a).expect("node exists");
            let b_node = self.nodes.get(b).expect("node exists");
            b_node
                .metric(metric)
                .cmp(&a_node.metric(metric))
                .then_with(|| b_node.references.len().cmp(&a_node.references.len()))
        });
        ids.truncate(limit);
        ids
    }

    pub fn top_by_referrers(&self, limit: usize) -> Vec<String> {
        let mut ids = self.nodes.keys().cloned().collect::<Vec<_>>();
        ids.sort_by(|a, b| {
            let a_node = self.nodes.get(a).expect("node exists");
            let b_node = self.nodes.get(b).expect("node exists");
            b_node
                .referrers
                .len()
                .cmp(&a_node.referrers.len())
                .then_with(|| b_node.nar_size.cmp(&a_node.nar_size))
        });
        ids.truncate(limit);
        ids
    }

    pub fn shortest_path_from_root(&self, target: &str) -> Option<Vec<String>> {
        if !self.nodes.contains_key(target) {
            return None;
        }

        if target == self.root_id {
            return Some(vec![self.root_id.clone()]);
        }

        let mut queue = VecDeque::new();
        let mut visited = HashSet::new();
        let mut parent: HashMap<String, String> = HashMap::new();

        queue.push_back(self.root_id.clone());
        visited.insert(self.root_id.clone());

        while let Some(current) = queue.pop_front() {
            if current == target {
                break;
            }

            let Some(node) = self.nodes.get(&current) else {
                continue;
            };

            for next in &node.references {
                if !self.nodes.contains_key(next) || visited.contains(next) {
                    continue;
                }

                visited.insert(next.clone());
                parent.insert(next.clone(), current.clone());
                queue.push_back(next.clone());
            }
        }

        if !visited.contains(target) {
            return None;
        }

        let mut path = Vec::new();
        let mut cursor = target.to_string();
        path.push(cursor.clone());

        while cursor != self.root_id {
            let prev = parent.get(&cursor)?;
            cursor = prev.clone();
            path.push(cursor.clone());
        }

        path.reverse();
        Some(path)
    }
}
