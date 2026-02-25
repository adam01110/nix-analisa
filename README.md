# nix-analisá

## ⚠️ Warning

Very vibecoded; you should probably not use it. I just wanted to try to
vibecode something even just only once, and I subsequently have been pulling a
lot of my hair out because the AI makes stupid mistakes consistently.

`nix-analisá` is a Rust GUI tool that inspects the active NixOS system closure
and visualizes it as an interactive dependency graph.

Key behavior:

- Reads NixOS closure metadata from `nix path-info` (not filesystem traversal).
- Builds a derivation/store-path dependency graph from
  `/run/current-system` by default.
- Renders an Obsidian-style graph with node size mapped to `narSize` or `closureSize`.
- Uses live physics simulation to keep nodes separated and reduce clumping.
- Highlights dependency paths and neighborhood edges related to the selected node.
- Shows "why large" details (direct size, transitive weight, reverse
  dependency pressure).

## Run

```bash
nix run .
```

The packaged binary defaults to a Wayland backend and injects runtime libraries.

If you need to force another backend for a specific session:

```bash
WINIT_UNIX_BACKEND=x11 nix run .
```

Custom target path:

```bash
nix run . -- --system-path /run/current-system
```

## Development shell

```bash
nix develop
```

## crate2nix flake workflow

The flake uses crate2nix `appliedCargoNix`, so Nix derives build metadata from
`Cargo.lock` during evaluation.

Common commands:

```bash
nix build
nix run .
nix flake check
```

Optional manual regeneration (not required for normal flake builds):

```bash
./scripts/regenerate-cargo-nix.sh
```

## Graph controls

- **Node size mode**: switch between `narSize` and `closureSize`.
- **Min node size**: hide tiny paths to reduce visual noise.
- **Max rendered nodes**: cap graph complexity for responsiveness.
- **Live physics simulation**: continuously spread nodes in the viewport.
- **Intensity**: tune repulsion and spring strength.
- **Search**: filter by hash or derivation name.
