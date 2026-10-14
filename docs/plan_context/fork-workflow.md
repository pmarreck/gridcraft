# Fork workflow

GridCraft's maintenance fork publishes its own `main` branch; `upstream` is storytold/gridcraft.

- Keep local agent-instruction overrides and handoff notes out of the repository.
- Standing policy (approved 2026-10-11): after the daily sync rebases `main`, publish it to the fork only with `git push --force-with-lease=main:<previous origin sha> origin main`, and only after a green `./test`. Never push to the upstream repository.
