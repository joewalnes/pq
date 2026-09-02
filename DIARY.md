# Engineering Diary

Latest entries first. Record significant decisions, architecture changes, and non-obvious context — the *why* that isn't visible in commits.

---

## 2026-09-02 — Third-party license bundling: generate at build time, don't commit

Chose not to commit the `cargo-about`-generated `THIRD-PARTY-LICENSES` (~525KB,
~250 dependencies). It tracks `Cargo.lock` exactly, so a committed copy would
silently drift stale on the next dependency bump with nothing to catch it.
Instead `make licenses` regenerates it and copies it (with pq's own `LICENSE`)
into the npm package directories; `pypi/build_wheels.py` reads both straight
from the repo root and embeds them in the wheel, failing loudly if
`THIRD-PARTY-LICENSES` is missing rather than shipping unattributed. Wiring
`make licenses` into `.github/workflows/release.yml` is deferred to whoever
merges this with the in-flight release-workflow rewrite - until then it's a
manual pre-release step.

Also: `cargo-about` has no concept of dependency `NOTICE` files at all
(confirmed by reading its source), so Apache-2.0 section 4(d) isn't satisfied
by cargo-about alone. `make licenses` separately walks the resolved crate
graph for `NOTICE`/`NOTICE.txt`/`NOTICE.md` files and appends every unique one
found (2 unique texts covering 19 crates: 18 `datafusion-*` + `object_store`).

## 2026-09-01 — Project process setup

Adopted AI-assisted development conventions via `/project-setup`: created `CLAUDE.md` (working rules: test-first, atomic commits, pre-commit `make test`/`make lint`, changelog discipline), this diary, and `CHANGELOG.md`. The existing `TODO.md` stays as the bug/task tracker in its current sectioned format.

Context on where the project stands: core commands, TUI viewer, remote file access (S3/GCS/Azure/HTTPS), docs site with generated CLI reference and demo GIFs, automated binary releases plus npm/PyPI publishing, and hosted example data at `data.pqtool.dev` (Cloudflare R2). Open threads are in `TODO.md` — notably `diff`, `repack`, `sort`, `schema evolve`, and the missing TUI viewer demo GIF on the docs site.
