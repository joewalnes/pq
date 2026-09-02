# Engineering Diary

Latest entries first. Record significant decisions, architecture changes, and non-obvious context — the *why* that isn't visible in commits.

---

## 2026-09-01 — Project process setup

Adopted AI-assisted development conventions via `/project-setup`: created `CLAUDE.md` (working rules: test-first, atomic commits, pre-commit `make test`/`make lint`, changelog discipline), this diary, and `CHANGELOG.md`. The existing `TODO.md` stays as the bug/task tracker in its current sectioned format.

Context on where the project stands: core commands, TUI viewer, remote file access (S3/GCS/Azure/HTTPS), docs site with generated CLI reference and demo GIFs, automated binary releases plus npm/PyPI publishing, and hosted example data at `data.pqtool.dev` (Cloudflare R2). Open threads are in `TODO.md` — notably `diff`, `repack`, `sort`, `schema evolve`, and the missing TUI viewer demo GIF on the docs site.
