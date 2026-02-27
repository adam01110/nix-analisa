{
  description = "Obsidian-style NixOS closure size analyzer in Rust";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-parts = {
      url = "github:hercules-ci/flake-parts";
      inputs.nixpkgs-lib.follows = "nixpkgs";
    };
    systems.url = "github:nix-systems/default";
    treefmt-nix = {
      url = "github:numtide/treefmt-nix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = inputs @ {
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
        app = pkgs.rustPlatform.buildRustPackage {
          pname = "nix-analisa";
          version = "0.1.0";
          src = ./.;

          cargoLock.lockFile = ./Cargo.lock;

          nativeBuildInputs = [
            pkgs.makeWrapper
            pkgs.pkg-config
            pkgs.clang
            pkgs.mold
          ];

          buildInputs = runtimeLibs;

          postInstall = ''
            wrapProgram "$out/bin/nix-analisa" \
              --set-default WINIT_UNIX_BACKEND wayland
          '';
        };
      in {
        packages = {
          default = app;
          nix-analisa = app;
        };

        devShells.default = pkgs.mkShell ({
            packages = builtins.attrValues {
              inherit
                (pkgs)
                cargo
                rustc
                rust-analyzer
                rustfmt
                clippy
                pkg-config
                clang
                mold
                ;
            };

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

        checks.build = app;

        treefmt = {
          flakeCheck = true;
          flakeFormatter = true;
          projectRootFile = "flake.nix";
          settings.global.excludes = [
            ".direnv/**"
            "target/**"
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
