#!/usr/bin/env bash
set -euo pipefail

if ! command -v crate2nix >/dev/null 2>&1; then
  echo "crate2nix not found in PATH. Run inside nix develop or install crate2nix."
  exit 1
fi

if ! command -v cargo >/dev/null 2>&1; then
  echo "cargo not found in PATH. Run inside nix develop or install cargo."
  exit 1
fi

cargo generate-lockfile
crate2nix generate
echo "Generated Cargo.nix"
