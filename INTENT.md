# GridCraft intent

GridCraft is a clean-room, open-source, Rust-native spreadsheet for people who
need Excel-compatible files, formulas, and familiar spreadsheet workflows.
It targets desktop platforms and the web with fast recalculation, open formats,
and command-based control. The upstream [roadmap](ROADMAP.md) and
[architecture](docs/architecture.md) define the product direction.

This fork's immediate work adds reproducible Nix builds and complete tests,
optimized native executable publication, and five build targets: ARM64 macOS,
ARM64/x86-64 Linux, and ARM64/x86-64 Windows. Builds keep the fork rebased on
upstream main: a verified sync is reused for 24 hours, then refreshed before
compiling, and an offline build is allowed only within that window. Build success includes
existing quality gates and inspection of actual executable headers; runtime
claims require tests on the relevant platform.

The queued web direction is a GitHub Pages deployment with browser local
storage for spreadsheets, local-file import, and either browser storage or
file export when saving. Preserve workbook contents and recoverable errors.
Never introduce proprietary assets or copy copyleft implementations.

Agent instructions for the upstream project are in CLAUDE.md.
Current work is tracked in [PLAN.md](PLAN.md).
