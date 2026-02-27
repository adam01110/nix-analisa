use std::collections::{HashMap, HashSet, VecDeque};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SizeMetric {
    NarSize,
    ClosureSize,
    Dependencies,
    ReverseDependencies,
}

impl SizeMetric {
    pub fn is_byte_metric(self) -> bool {
        matches!(self, Self::NarSize | Self::ClosureSize)
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
            SizeMetric::Dependencies => self.references.len() as u64,
            SizeMetric::ReverseDependencies => self.referrers.len() as u64,
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

    pub fn ranked_by_metric(&self, metric: SizeMetric, limit: usize) -> Vec<String> {
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

    pub fn ranked_by_referrers(&self, limit: usize) -> Vec<String> {
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

    pub fn ranked_by_dependencies(&self, limit: usize) -> Vec<String> {
        let mut ids = self.nodes.keys().cloned().collect::<Vec<_>>();
        ids.sort_by(|a, b| {
            let a_node = self.nodes.get(a).expect("node exists");
            let b_node = self.nodes.get(b).expect("node exists");
            b_node
                .references
                .len()
                .cmp(&a_node.references.len())
                .then_with(|| b_node.nar_size.cmp(&a_node.nar_size))
        });
        ids.truncate(limit);
        ids
    }

    pub fn shortest_path_from_root(&self, target: &str) -> Option<Vec<String>> {
        let target = self.nodes.get_key_value(target)?.0.as_str();
        let root = self.root_id.as_str();

        if target == root {
            return Some(vec![self.root_id.clone()]);
        }

        let mut queue: VecDeque<&str> = VecDeque::new();
        let mut visited: HashSet<&str> = HashSet::new();
        let mut parent: HashMap<&str, &str> = HashMap::new();

        queue.push_back(root);
        visited.insert(root);

        while let Some(current) = queue.pop_front() {
            if current == target {
                break;
            }

            let Some(node) = self.nodes.get(current) else {
                continue;
            };

            for next in &node.references {
                let Some((next_key, _)) = self.nodes.get_key_value(next.as_str()) else {
                    continue;
                };
                let next_key = next_key.as_str();

                if visited.insert(next_key) {
                    parent.insert(next_key, current);
                    queue.push_back(next_key);
                }
            }
        }

        if !visited.contains(target) {
            return None;
        }

        let mut path = Vec::new();
        let mut cursor = target;
        path.push(cursor.to_owned());

        while cursor != root {
            cursor = *parent.get(cursor)?;
            path.push(cursor.to_owned());
        }

        path.reverse();
        Some(path)
    }
}
