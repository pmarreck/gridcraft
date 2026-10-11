# PLAN

## Build and distribution

- [x] Read the setup handoff and the upstream agent instructions (done 2026-10-11 13:05 EDT).
- [x] Add `--about`/`--help` to the desktop app and `--about` to gridcraft-cli, test-first (done 2026-10-11 13:12 EDT).
- [x] Add a pinned flake.nix (crane + rust-overlay) whose `nix build` matches `./build`'s optimized host package. (done 2026-10-11 14:00 EDT)
- [x] Add Nix-backed `./test` covering every `cargo xtask ci` gate (fmt, clippy, tests, assets, layers, wasm, parity) plus CLI regressions. (done 2026-10-11 14:00 EDT)
- [x] Add optimized `./build` with safe upstream/main fetch and rebase, `--no-update`, `--debug`, `--test`, `--help`, `--version`, `--about`. (done 2026-10-11 14:00 EDT)
- [x] Upstream sync cache under .git: default builds fetch+rebase when the last verified sync is >24 h old (fail before compiling if that fails); `--no-update` fails without a valid sync <24 h old; `--update` forces; HEAD must still contain the recorded upstream commit. Tests inject the clock and use real local Git remotes. (done 2026-10-11 14:00 EDT)
- [x] Add `./build-all` (sole name, no underscore alias) for aarch64 macOS/Linux/Windows and x86_64 Linux/Windows; executables under `bin/<os>/<arch>/`. (done 2026-10-11 14:00 EDT)
- [x] Built all five targets and inspected headers: x86_64/aarch64 Linux ELF, x86_64/ARM64 Windows PE32+ (GUI + console, .rsrc, real DLLs), arm64 macOS Mach-O min 14.0 via the remote Mac builder; launchers are native (done 2026-10-11 14:00 EDT)
- [ ] macOS Nix products link /nix/store libiconv: relink to /usr/lib/libiconv.2.dylib and re-sign (ad hoc) so the .app runs on a Mac without Nix; verify with llvm-objdump --dylibs-used.
- [x] Annotate new files and important directories with dirtree notes. (done 2026-10-11 14:00 EDT)
- [ ] Create the pmarreck/gridcraft fork (origin → upstream rename), keep `main`, push after green tests; set up Mechatron Prime CI.
- Keep local agent-instruction overrides and handoff notes out of the repository.

## Web application

- [ ] Compile GridCraft for web/WASM and deploy on GitHub Pages with browser local storage as its filesystem, local-file import, and save choices for browser storage or export to the local filesystem.
