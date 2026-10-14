# The browser build (apps/gridcraft-web) as a static site: Trunk runs offline with the
# wasm-bindgen CLI that matches Cargo.lock, and the output is ready to serve from any subpath.
{
  pkgs,
  crane,
  src,
}:
let
  lib = pkgs.lib;
  toolchain =
    p:
    p.rust-bin.stable.latest.default.override {
      targets = [ "wasm32-unknown-unknown" ];
    };
  craneLib = (crane.mkLib pkgs).overrideToolchain toolchain;
  version = (builtins.fromTOML (builtins.readFile ../Cargo.toml)).workspace.package.version;
  # Must equal the wasm-bindgen version in Cargo.lock; the flake's web check fails otherwise.
  wasmBindgenVersion =
    (lib.findFirst (p: p.name == "wasm-bindgen") (throw "wasm-bindgen is not in Cargo.lock")
      (builtins.fromTOML (builtins.readFile ../Cargo.lock)).package
    ).version;
  wasm-bindgen-cli =
    pkgs."wasm-bindgen-cli_${lib.replaceStrings [ "." ] [ "_" ] wasmBindgenVersion}"
      or (throw "nixpkgs has no wasm-bindgen-cli ${wasmBindgenVersion}; package it or update nixpkgs");
  args = {
    pname = "gridcraft-web";
    inherit src version;
    strictDeps = true;
    CARGO_BUILD_TARGET = "wasm32-unknown-unknown";
    cargoExtraArgs = "--locked -p gridcraft-web";
    doCheck = false;
  };
  # The app alone; `site` adds the fonts, so changing the font set never rebuilds the app.
  app = craneLib.buildTrunkPackage (
    args
    // {
      inherit wasm-bindgen-cli;
      cargoArtifacts = craneLib.buildDepsOnly args;
      # Trunk.toml lives beside the page and writes to the workspace's dist/web.
      buildPhaseCargoCommand = ''
        (cd apps/gridcraft-web && trunk build --release=true)
      '';
      installPhaseCommand = ''
        cp -r dist/web "$out"
      '';
    }
  );
  testArgs = args // {
    pname = "gridcraft-web-storage-tests";
    doCheck = true;
  };
in
{
  # The site: the app plus the self-hosted fonts it fetches on demand (nix/web-fonts.nix).
  site = pkgs.runCommand "gridcraft-web-${version}" { } ''
    cp -r ${app} "$out"
    chmod u+w "$out"
    cp -r ${import ./web-fonts.nix { inherit pkgs; }} "$out/fonts"
  '';

  # The browser-only tests (the IndexedDB layer and the file picker), run in headless Chromium
  # through wasm-bindgen-test-runner.
  # Linux only: nixpkgs has no Chromium for macOS.
  storageTests = craneLib.cargoTest (
    testArgs
    // {
      cargoArtifacts = craneLib.buildDepsOnly testArgs;
      nativeBuildInputs = [
        wasm-bindgen-cli
        pkgs.chromium
        pkgs.chromedriver
      ];
      CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUNNER = "wasm-bindgen-test-runner";
      CHROMEDRIVER = lib.getExe pkgs.chromedriver;
      WASM_BINDGEN_TEST_TIMEOUT = "120";
      # Without a font configuration Chromium aborts in the sandbox before any page loads.
      FONTCONFIG_FILE = pkgs.makeFontsConf { fontDirectories = [ pkgs.dejavu_fonts ]; };
      preCheck = ''
        export HOME="$TMPDIR"
      '';
    }
  );
}
