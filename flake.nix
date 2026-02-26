{
  description = "Obsidian-style NixOS closure size analyzer in Rust";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-parts.url = "github:hercules-ci/flake-parts";
    systems.url = "github:nix-systems/default";
    treefmt-nix.url = "github:numtide/treefmt-nix";
    crate2nix.url = "github:nix-community/crate2nix";
  };

  outputs = inputs @ {
    crate2nix,
    flake-parts,
    systems,
    treefmt-nix,
    ...
  }:
    flake-parts.lib.mkFlake {inherit inputs;} {
      imports = [treefmt-nix.flakeModule];

      systems = builtins.filter (system: builtins.match ".*-linux" system != null) (import systems);

      perSystem = {
        config,
        pkgs,
        system,
        ...
      }: let
        rustTargetEnv = pkgs.lib.toUpper (builtins.replaceStrings ["-"] ["_"] pkgs.stdenv.hostPlatform.config);
        runtimeLibs = with pkgs; [
          wayland
          libxkbcommon
          libglvnd
          vulkan-loader
          libx11
          libxcursor
          libxi
          libxrandr
        ];
        runtimeLibPath = pkgs.lib.makeLibraryPath runtimeLibs;
        crateOverrides =
          pkgs.defaultCrateOverrides
          // {
            nix-analisa = old: {
              nativeBuildInputs = (old.nativeBuildInputs or []) ++ [pkgs.makeWrapper];
              postInstall =
                (old.postInstall or "")
                + ''
                  wrapProgram "$out/bin/nix-analisa" \
                    --set-default WINIT_UNIX_BACKEND wayland
                '';
            };
          };
        cargoNix = crate2nix.tools.${system}.appliedCargoNix {
          name = "nix-analisa";
          src = ./.;
        };
        app = cargoNix.rootCrate.build.override {
          inherit crateOverrides;
        };
      in {
        packages.default = app;
        packages.nix-analisa = app;

        devShells.default = pkgs.mkShell ({
            packages = with pkgs; [
              cargo
              rustc
              rust-analyzer
              rustfmt
              clippy
              pkg-config
              clang
              mold
              crate2nix.packages.${system}.default
            ];

            LD_LIBRARY_PATH = runtimeLibPath;
            RUSTFLAGS = "-C link-arg=-fuse-ld=mold";
          }
          // {
            "CARGO_TARGET_${rustTargetEnv}_LINKER" = "clang";
          });

        apps.default = {
          type = "app";
          program = "${app}/bin/nix-analisa";
          meta.description = "Obsidian-style NixOS closure size analyzer";
        };

        formatter = config.treefmt.build.wrapper;

        checks.build = cargoNix.rootCrate.build.override {
          runTests = true;
        };

        treefmt = {
          flakeCheck = true;
          flakeFormatter = true;
          projectRootFile = "flake.nix";
          settings.global.excludes = [
            "Cargo.nix"
            ".direnv/**"
          ];

          programs = {
            alejandra.enable = true;
            nixf-diagnose.enable = true;
            deadnix.enable = true;
            statix.enable = true;

            rustfmt.enable = true;

            rumdl-check.enable = true;
            rumdl-format.enable = true;
          };
        };
      };
    };
}
