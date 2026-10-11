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
- [ ] Seal the GridCraft.app bundle signature in Nix: `buildPkgs.rcodesign` and `rcodesign sign "$out/Applications/GridCraft.app"` at the end of postFixup (after stripping and the libiconv relink), as WordCraft does. Accept only Apple's `/usr/bin/codesign --verify --deep --strict` on a Mac (rcodesign 0.29 verify falsely rejects ad hoc signatures); the Mac signing host is passwordless over the tailnet (keep its name out of tracked files). Pre-existing: `codesign --verify` on the bundle reports "code has no resources but signature indicates they must be present".
- [x] Annotate new files and important directories with dirtree notes. (done 2026-10-11 14:00 EDT)
- [x] Fork published (origin is the fork, upstream is storytold/gridcraft; branch `main`); Mechatron Prime CI webhook provisioned; first build of 33e9712 passed all 14 targets in 541 s, badge PASSING. (2026-10-11 3:36 PM EDT)
- Keep local agent-instruction overrides and handoff notes out of the repository.
- Standing policy (approved 2026-10-11): after the daily sync rebases `main`, publish it to the fork only with `git push --force-with-lease=main:<previous origin sha> origin main`, and only after a green `./test`. Never push to the upstream repository.

## Web application

- [x] Web build in Nix (`nix build .#web`, gated by `checks.web`) and `./deploy-web` publishing to the fork's gh-pages branch for GitHub Pages. Upstream's web app already opens local files (picker or drop) and Save downloads (exports) the workbook. (2026-10-11 5:25 PM EDT)
- [x] Web: browser storage as the filesystem. The engine reads and writes through an optional `FileStore`; the browser library (`crates/ui-egui/src/library.rs`) mirrors IndexedDB with a write-behind journal. Save offers "In this browser" or a download; Ctrl+S and AutoSave write to the library; File › Open lists stored workbooks (open, rename, delete) and imports local files. (2026-10-11 7:00 PM EDT)
- [ ] Web: confirm in a real browser that saved workbooks survive a reload (the IndexedDB adapter has no automated test). Known limits: two tabs saving one name is last-writer-wins; the library is held in memory.
- [x] Compared browser storage with WordCraft (localStorage, native JSON envelopes, revisions; reply received 2026-10-11 6:55 PM EDT).
- [ ] Web durability: clear a workbook's dirty flag only after its IndexedDB transaction commits (transaction complete, not request success). Capture the saved edit generation and clear only if it still matches; keep failed journal batches for retry along with the dirty state.
- [ ] Web conflicts: store a revision per workbook and compare it inside the same read-write transaction, so a stale tab cannot silently overwrite a newer save.
- [ ] Web end-to-end test: real headless Chromium through Nix (WordCraft is building `nix/browser-test.nix`): save, restart the browser profile, reopen, compare contents. Covers the IndexedDB adapter. WordCraft's version passes in a pure derivation (reference: its `nix/browser-test.nix`, `tests/cli/browser-storage`). Traps: set `FONTCONFIG_FILE = pkgs.makeFontsConf { fontDirectories = [ pkgs.dejavu_fonts ]; }` or Chromium aborts in SkFontMgr before the app loads; for file import, send CDP `Page.setInterceptFileChooserDialog {enabled: true}` before clicking Import, then fill the real `input[type=file]`. Use the previous deployed build as a negative control.
