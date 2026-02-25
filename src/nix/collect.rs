use std::collections::{HashMap, HashSet};

use anyhow::{Context, Result, anyhow};

use super::graph::{NodeRecord, SystemGraph};
use super::nix_cmd::run_nix;
use super::parse::{normalize_store_key, parse_path_info_output};

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
