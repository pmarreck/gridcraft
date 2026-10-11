# One target's Cargo build: the desktop app and gridcraft-cli, plus the shared dependency
# artifacts the flake's checks reuse. `pkgs` is the target package set, `buildPkgs` the host's.
{
  pkgs,
  buildPkgs,
  crane,
  src,
  profile,
}:
let
  lib = pkgs.lib;
  platform = pkgs.stdenv.hostPlatform;
  target = platform.rust.rustcTarget;
  exe = platform.extensions.executable;
  makeToolchain =
    p:
    p.rust-bin.stable.latest.default.override {
      extensions = [
        "rust-src"
        "rustfmt"
        "clippy"
      ];
      targets = [
        target
        "wasm32-unknown-unknown"
      ];
    };
  toolchain = makeToolchain buildPkgs;
  craneLib = (crane.mkLib pkgs).overrideToolchain makeToolchain;
  version = (builtins.fromTOML (builtins.readFile ../Cargo.toml)).workspace.package.version;
  # Runtime libraries winit and wgpu open with dlopen on Linux.
  linuxRuntime = lib.optionals platform.isLinux [
    pkgs.wayland
    pkgs.libxkbcommon
    pkgs.libGL
    pkgs.vulkan-loader
    pkgs.libx11
    pkgs.libxcursor
    pkgs.libxi
    pkgs.libxrandr
    pkgs.libxcb
  ];
  # Regex for `file -b` output of a correct executable for this target.
  headerPattern =
    if platform.isDarwin then
      "Mach-O 64-bit.*arm64"
    else if platform.isWindows && platform.isAarch64 then
      "PE32\\+.*(ARM64|Aarch64)"
    else if platform.isWindows then
      "PE32\\+.*x86-64"
    else if platform.isAarch64 then
      "ELF 64-bit.*ARM aarch64"
    else
      "ELF 64-bit.*x86-64";
  args = {
    pname = "gridcraft";
    inherit src version;
    strictDeps = true;
    CARGO_PROFILE = profile;
    cargoExtraArgs = "--workspace --locked";
    nativeBuildInputs = [
      buildPkgs.pkg-config
      # A compiled launcher for the target; a shell wrapper would name the build machine's bash.
      pkgs.buildPackages.makeBinaryWrapper
      buildPkgs.file
      buildPkgs.gnugrep
    ];
    buildInputs = lib.optionals platform.isLinux [
      pkgs.wayland
      pkgs.libxkbcommon
    ];
  };
  cargoArtifacts = craneLib.buildDepsOnly (args // { doCheck = profile == "dev"; });
  package = craneLib.buildPackage (
    args
    // {
      inherit cargoArtifacts;
      cargoExtraArgs = "--locked -p gridcraft -p gridcraft-cli";
      doCheck = false;
      postInstall = ''
        mkdir -p "$out/share/doc/gridcraft"
        cp LICENSE-MIT LICENSE-APACHE NOTICE ATTRIBUTION.md "$out/share/doc/gridcraft/"
      ''
      + lib.optionalString platform.isLinux ''
        wrapProgram "$out/bin/gridcraft" --prefix LD_LIBRARY_PATH : ${lib.makeLibraryPath linuxRuntime}
      ''
      + lib.optionalString platform.isDarwin ''
        app="$out/Applications/GridCraft.app/Contents"
        mkdir -p "$app/MacOS" "$app/Resources"
        mv "$out/bin/gridcraft" "$app/MacOS/GridCraft"
        cp assets/app-icon/gridcraft.icns "$app/Resources/GridCraft.icns"
        substitute packaging/macos/Info.plist.in "$app/Info.plist" \
          --replace-fail '@VERSION@' '${version}' \
          --replace-fail '@SHORT_VERSION@' '${version}' \
          --replace-fail '@BUILD_SHA@' 'nix' \
          --replace-fail '<string>11.0</string>' '<string>${platform.darwinMinVersion}</string>'
        printf 'APPL????' > "$app/PkgInfo"
        makeWrapper "$app/MacOS/GridCraft" "$out/bin/gridcraft"
      ''
      + lib.optionalString platform.isWindows ''
        # Keep runtime DLLs beside the executables for native Windows loading.
        for dll in ${pkgs.stdenv.cc.cc}/bin/*.dll ${
          pkgs.stdenv.cc.cc.lib or pkgs.stdenv.cc.cc
        }/bin/*.dll ${pkgs.windows.pthreads}/bin/*.dll; do
          if [ -f "$dll" ]; then cp -L "$dll" "$out/bin/"; fi
        done
      '';
      # Nixpkgs's Windows fixup links runtime DLLs (libunwind on ARM64) into bin/ as symlinks to
      # the store, which dangle once the folder is copied to Windows. Ship real files instead.
      postFixup =
        lib.optionalString platform.isWindows ''
          for dll in "$out"/bin/*.dll; do
            if [ -L "$dll" ]; then cp --remove-destination "$(readlink -f "$dll")" "$dll"; fi
          done
          if find "$out/bin" -type l | grep -q .; then
            echo "symlinks remain in $out/bin" >&2
            exit 1
          fi
        ''
        # Every executable and launcher, after wrapping and stripping, is machine code for this
        # target (a script launcher would name the build machine's interpreter).
        + ''
          for executable in "$out/bin/gridcraft${exe}" "$out/bin/gridcraft-cli${exe}" ${
            lib.optionalString platform.isDarwin ''"$out/Applications/GridCraft.app/Contents/MacOS/GridCraft"''
          }; do
            file "$executable"
            file -b "$executable" | grep -E '${headerPattern}'
          done
        '';
      meta = {
        description = "Open-source, clean-room spreadsheet with XLSX support and a headless CLI";
        homepage = "https://github.com/storytold/gridcraft";
        license = [
          lib.licenses.mit
          lib.licenses.asl20
        ];
        mainProgram = "gridcraft";
      };
    }
  );
in
{
  inherit
    package
    craneLib
    args
    cargoArtifacts
    toolchain
    ;
}
