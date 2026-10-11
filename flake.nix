{
  description = "GridCraft desktop and CLI: reproducible builds, tests and cross-platform packages";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    crane.url = "github:ipetkov/crane";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    {
      self,
      nixpkgs,
      crane,
      rust-overlay,
    }:
    let
      lib = nixpkgs.lib;
      systems = [
        "aarch64-darwin"
        "aarch64-linux"
        "x86_64-linux"
      ];
      forSystems = lib.genAttrs systems;
      overlays = [
        rust-overlay.overlays.default
        (import ./nix/windows-toolchain.nix)
      ];
      pkgsFor = system: import nixpkgs { inherit system overlays; };
      # The Cargo build's inputs only. Editing docs, scripts or Nix files must not trigger a new
      # optimized build of the GUI.
      rustSrc = lib.fileset.toSource {
        root = ./.;
        fileset = lib.fileset.unions [
          ./Cargo.toml
          ./Cargo.lock
          ./.cargo
          ./clippy.toml
          ./rustfmt.toml
          ./crates
          ./apps
          ./xtask
          ./assets
          ./contributors
          ./packaging
          ./ATTRIBUTION.md
          ./LICENSE-MIT
          ./LICENSE-APACHE
          ./NOTICE
        ];
      };
      # The whole checkout for the gates that read docs (assets, parity) and the shell tests.
      # Never capture local products or the owner's private agent instructions.
      fullSrc = lib.cleanSourceWith {
        src = self;
        filter =
          path: type:
          let
            name = baseNameOf path;
          in
          lib.cleanSourceFilter path type
          && !(builtins.elem name [
            "bin"
            "target"
            "dist"
            "plan"
            ".nix-out"
            ".codescan"
            "inbox"
            "result"
            "AGENTS.md"
            "AGENTS_orig.md"
            ".dirtree-state"
          ])
          && !(lib.hasPrefix "result-" name || lib.hasPrefix "target-" name || lib.hasPrefix "HANDOFF" name);
      };
      mk =
        buildSystem: crossSystem: profile:
        let
          buildPkgs = pkgsFor buildSystem;
          pkgs =
            if crossSystem == null then
              buildPkgs
            else
              import nixpkgs {
                localSystem = buildSystem;
                # Build-only cross packages never run their output, so never evaluate an emulator
                # (Wine on Darwin is unsupported by Nixpkgs).
                crossSystem = crossSystem // {
                  emulatorAvailable = _: false;
                };
                inherit overlays;
              };
        in
        import ./nix/package.nix {
          inherit
            pkgs
            buildPkgs
            crane
            profile
            ;
          src = rustSrc;
        };
      native = system: mk system null "release";
    in
    {
      packages = forSystems (
        system:
        let
          targets = {
            # Apple frameworks need a native Apple Silicon builder, local or a configured remote.
            aarch64-macos = mk "aarch64-darwin" null;
            aarch64-linux = mk system (
              if system == "aarch64-linux" then null else lib.systems.examples.aarch64-multiplatform
            );
            x86_64-linux = mk system (if system == "x86_64-linux" then null else lib.systems.examples.gnu64);
            aarch64-windows = mk system lib.systems.examples.mingw-ucrt-aarch64;
            x86_64-windows = mk system lib.systems.examples.mingwW64;
          };
        in
        lib.mapAttrs (_: make: (make "release").package) targets
        // lib.mapAttrs' (name: make: lib.nameValuePair "${name}-debug" (make "dev").package) targets
        // {
          default = (native system).package;
          web = import ./nix/web.nix {
            pkgs = pkgsFor system;
            inherit crane;
            src = rustSrc;
          };
        }
      );

      checks = forSystems (
        system:
        let
          pkgs = pkgsFor system;
          n = native system;
          dev = mk system null "dev";
          common = dev.args // {
            cargoArtifacts = dev.cargoArtifacts;
          };
          # One `cargo xtask` gate in the sandbox, with the whole checkout as its source.
          xtask =
            step:
            dev.craneLib.mkCargoDerivation (
              common
              // {
                pname = "gridcraft-${step}";
                src = fullSrc;
                nativeBuildInputs = dev.args.nativeBuildInputs ++ [ pkgs.git ];
                # The sandbox has no .git directory; the assets gate lists files with git, so give
                # it a real index of the source instead of an empty set.
                buildPhaseCargoCommand = ''
                  git -c init.defaultBranch=main init -q
                  git add -- .
                  cargo xtask ${step}
                '';
                installPhaseCommand = ''mkdir -p "$out"; touch "$out/passed"'';
                doInstallCargoArtifacts = false;
                doCheck = false;
              }
            );
        in
        {
          build = n.package;
          test = dev.craneLib.cargoTest (
            common
            // {
              cargoTestExtraArgs = "--no-fail-fast";
            }
          );
          clippy = dev.craneLib.cargoClippy (
            common // { cargoClippyExtraArgs = "--all-targets -- -D warnings"; }
          );
          fmt = dev.craneLib.cargoFmt {
            src = rustSrc;
            pname = "gridcraft";
          };
          assets = xtask "assets";
          layers = xtask "layers";
          wasm = xtask "wasm";
          parity = xtask "parity --check";
          scripts =
            pkgs.runCommand "gridcraft-build-script-tests"
              {
                nativeBuildInputs = [
                  pkgs.bash
                  pkgs.git
                  pkgs.coreutils
                  pkgs.gawk
                  pkgs.shellcheck
                ];
              }
              ''
                cp -r ${fullSrc} source
                chmod -R u+w source
                cd source
                export HOME=$TMPDIR GIT_CONFIG_NOSYSTEM=1 GIT_CONFIG_GLOBAL=/dev/null
                # The sandbox has no /usr/bin/env.
                patchShebangs build build-all deploy-web test tests/cli
                shellcheck build build-all deploy-web test scripts/build-common.sh tests/cli/build-scripts tests/cli/deploy-web tests/cli/apps tests/web/check-dist
                bash tests/cli/build-scripts
                bash tests/cli/deploy-web
                touch "$out"
              '';
          web =
            pkgs.runCommand "gridcraft-web-dist-tests"
              {
                nativeBuildInputs = [
                  pkgs.bash
                  pkgs.coreutils
                  pkgs.gnugrep
                  pkgs.gnused
                  pkgs.findutils
                ];
              }
              ''
                bash ${fullSrc}/tests/web/check-dist ${self.packages.${system}.web}
                touch "$out"
              '';
          cli =
            pkgs.runCommand "gridcraft-cli-tests"
              {
                nativeBuildInputs = [
                  pkgs.bash
                  pkgs.coreutils
                ];
              }
              ''
                export HOME=$TMPDIR
                bash ${fullSrc}/tests/cli/apps ${n.package}/bin
                touch "$out"
              '';
        }
      );

      devShells = forSystems (
        system:
        let
          pkgs = pkgsFor system;
          n = native system;
        in
        {
          default = n.craneLib.devShell {
            packages = [
              pkgs.pkg-config
              pkgs.shellcheck
              pkgs.git
              pkgs.trunk
            ];
            inputsFrom = [ n.package ];
            LD_LIBRARY_PATH = lib.optionalString pkgs.stdenv.hostPlatform.isLinux (
              lib.makeLibraryPath [
                pkgs.wayland
                pkgs.libxkbcommon
                pkgs.libGL
                pkgs.vulkan-loader
                pkgs.libx11
                pkgs.libxcursor
                pkgs.libxi
                pkgs.libxrandr
              ]
            );
          };
        }
      );
      formatter = forSystems (system: (pkgsFor system).nixfmt-tree);
    };
}
