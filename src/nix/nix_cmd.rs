use std::process::Command;

use anyhow::{anyhow, Context, Result};

pub(super) fn run_nix(args: &[&str]) -> Result<String> {
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
