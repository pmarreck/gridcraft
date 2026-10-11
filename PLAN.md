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
- [x] macOS products load only /usr/lib and /System libraries: libiconv relinked to /usr/lib/libiconv.2.dylib and re-signed ad hoc, with a build gate; verified on a Mac with Apple codesign and by running both binaries. (2026-10-11 5:25 PM EDT)
- [ ] Seal the GridCraft.app bundle signature with Apple codesign on the Mac signing host (passwordless over the tailnet; keep its name out of tracked files; sigtool signs Mach-O files only). Pre-existing: `codesign --verify` on the bundle reports "code has no resources but signature indicates they must be present".
- [x] Annotate new files and important directories with dirtree notes. (done 2026-10-11 14:00 EDT)
- [x] Fork published (origin is the fork, upstream is storytold/gridcraft; branch `main`); Mechatron Prime CI webhook provisioned; first build of 33e9712 passed all 14 targets in 541 s, badge PASSING. (2026-10-11 3:36 PM EDT)
- Keep local agent-instruction overrides and handoff notes out of the repository.

## Web application

- [x] Web build in Nix (`nix build .#web`, gated by `checks.web`) and `./deploy-web` publishing to the fork's gh-pages branch for GitHub Pages. Upstream's web app already opens local files (picker or drop) and Save downloads (exports) the workbook. (2026-10-11 5:25 PM EDT)
- [ ] Web: browser storage as the filesystem (IndexedDB-backed workbook list: open, save, rename, delete) and a Save choice between browser storage and export to the local filesystem; keep local-file import.
