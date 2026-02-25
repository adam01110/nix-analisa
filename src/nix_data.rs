use std::collections::{HashMap, HashSet, VecDeque};
use std::process::Command;

use anyhow::{anyhow, Context, Result};
use serde::Deserialize;
use serde_json::Value;

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

#[derive(Clone, Debug, Deserialize)]
struct RawPathInfo {
    #[serde(default, rename = "narSize")]
    nar_size: u64,
    #[serde(default, rename = "closureSize")]
    closure_size: u64,
    #[serde(default)]
    references: Vec<String>,
    #[serde(default)]
    deriver: Option<String>,
}

pub fn collect_system_graph(system_path: &str) -> Result<SystemGraph> {
    let root_raw = run_nix(&["path-info", "--json", "--json-format", "2", system_path])
        .with_context(|| format!("failed to resolve root path for {system_path}"))?;

    let (_root_store_dir, root_info) =
        parse_path_info_output(&root_raw).context("failed to parse root nix path-info output")?;

    let root_key = root_info
        .keys()
        .next()
        .cloned()
        .ok_or_else(|| anyhow!("nix path-info returned empty info for {system_path}"))?;
    let root_id = normalize_store_key(&root_key);

    let closure_raw = run_nix(&[
        "path-info",
        "--recursive",
        "--closure-size",
        "--json",
        "--json-format",
        "2",
        system_path,
    ])
    .with_context(|| format!("failed to collect recursive closure for {system_path}"))?;

    let (store_dir, closure_info) = parse_path_info_output(&closure_raw)
        .context("failed to parse recursive closure nix path-info output")?;

    let mut nodes = HashMap::with_capacity(closure_info.len());

    for (raw_key, raw_entry) in closure_info {
        let id = normalize_store_key(&raw_key);
        if id.is_empty() {
            continue;
        }

        let full_path = if raw_key.starts_with('/') {
            raw_key.clone()
        } else {
            format!("{store_dir}/{id}")
        };

        let mut references = raw_entry
            .references
            .into_iter()
            .map(|reference| normalize_store_key(&reference))
            .filter(|reference| !reference.is_empty() && reference != &id)
            .collect::<Vec<_>>();
        references.sort();
        references.dedup();

        let closure_size = if raw_entry.closure_size == 0 {
            raw_entry.nar_size
        } else {
            raw_entry.closure_size
        };

        let deriver = raw_entry
            .deriver
            .map(|value| normalize_store_key(&value))
            .filter(|value| !value.is_empty());

        nodes.insert(
            id.clone(),
            NodeRecord {
                id,
                full_path,
                nar_size: raw_entry.nar_size,
                closure_size,
                references,
                referrers: Vec::new(),
                deriver,
            },
        );
    }

    if nodes.is_empty() {
        return Err(anyhow!(
            "no closure nodes were returned by nix path-info for {system_path}"
        ));
    }

    let root_id = if nodes.contains_key(&root_id) {
        root_id
    } else {
        nodes
            .keys()
            .next()
            .cloned()
            .ok_or_else(|| anyhow!("closure nodes are unexpectedly empty"))?
    };

    let known_ids = nodes.keys().cloned().collect::<HashSet<_>>();
    let mut reverse_refs: HashMap<String, Vec<String>> = HashMap::new();
    let mut edge_count = 0usize;

    for (id, node) in &mut nodes {
        node.references
            .retain(|reference| known_ids.contains(reference));
        node.references.sort();
        node.references.dedup();

        edge_count += node.references.len();
        for reference in &node.references {
            reverse_refs
                .entry(reference.clone())
                .or_default()
                .push(id.clone());
        }
    }

    for (id, node) in &mut nodes {
        if let Some(mut referrers) = reverse_refs.remove(id) {
            referrers.sort();
            node.referrers = referrers;
        }
    }

    Ok(SystemGraph {
        store_dir,
        root_id,
        nodes,
        edge_count,
    })
}

fn run_nix(args: &[&str]) -> Result<String> {
    let output = Command::new("nix")
        .args(args)
        .output()
        .with_context(|| format!("failed to spawn nix with args: {args:?}"))?;

    if output.status.success() {
        String::from_utf8(output.stdout).context("nix output was not valid UTF-8")
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(anyhow!("nix command failed for args {args:?}: {stderr}"))
    }
}

fn parse_path_info_output(raw: &str) -> Result<(String, HashMap<String, RawPathInfo>)> {
    let parsed: Value = serde_json::from_str(raw).context("invalid JSON from nix")?;
    let object = parsed
        .as_object()
        .ok_or_else(|| anyhow!("unexpected JSON type from nix path-info"))?;

    if let Some(info_value) = object.get("info") {
        let store_dir = object
            .get("storeDir")
            .and_then(Value::as_str)
            .unwrap_or("/nix/store")
            .to_string();
        let info: HashMap<String, RawPathInfo> =
            serde_json::from_value(info_value.clone()).context("invalid info map in JSON")?;
        return Ok((store_dir, info));
    }

    let store_dir = object
        .get("storeDir")
        .and_then(Value::as_str)
        .unwrap_or("/nix/store")
        .to_string();

    let mut info = HashMap::new();
    for (key, value) in object {
        if key == "storeDir" || key == "version" {
            continue;
        }

        if let Ok(entry) = serde_json::from_value::<RawPathInfo>(value.clone()) {
            info.insert(key.clone(), entry);
        }
    }

    if info.is_empty() {
        Err(anyhow!(
            "could not parse nix path-info JSON; no entries found"
        ))
    } else {
        Ok((store_dir, info))
    }
}

fn normalize_store_key(value: &str) -> String {
    value.rsplit('/').next().unwrap_or(value).to_string()
}
