use std::collections::HashMap;

use anyhow::{Context, Result, anyhow};
use serde::Deserialize;
use serde_json::Value;

#[derive(Clone, Debug, Deserialize)]
pub(super) struct RawPathInfo {
    #[serde(default, rename = "narSize")]
    pub(super) nar_size: u64,
    #[serde(default, rename = "closureSize")]
    pub(super) closure_size: u64,
    #[serde(default)]
    pub(super) references: Vec<String>,
    #[serde(default)]
    pub(super) deriver: Option<String>,
}

pub(super) fn parse_path_info_output(raw: &str) -> Result<(String, HashMap<String, RawPathInfo>)> {
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

pub(super) fn normalize_store_key(value: &str) -> String {
    value.rsplit('/').next().unwrap_or(value).to_string()
}
