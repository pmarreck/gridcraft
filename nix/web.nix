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
    (lib.findFirst (p: p.name == "wasm-bindgen") (throw "wasm-bindgen is not in Cargo.lock") (
      builtins.fromTOML (builtins.readFile ../Cargo.lock)
    ).package).version;
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
in
craneLib.buildTrunkPackage (
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
)
