# Engineering Diary

Latest entries first. Record significant decisions, architecture changes, and non-obvious context — the *why* that isn't visible in commits.

---

## 2026-09-01 — Releases are now deliberate, and gated on CI

Until today every push to `main` ran `release.yml`, which minted a
`0.1.$(date +%Y%m%d%H%M)` version and published it to npm and PyPI with nothing
having run fmt, clippy, tests, or the golden suite first. npm and PyPI never allow
a version number to be reused, so each of those pushes was an irreversible public
release of unverified code. That is why unattended work was forbidden from pushing.

Two changes fix the immediate danger. The trigger moved to `v*` tags plus
`workflow_dispatch`, so a release is now an explicit act by a human rather than a
side effect of merging. (`v*` deliberately does not match the `latest` tag the
release job creates, so the workflow cannot re-trigger itself.) And CI is pulled
into `release.yml` as a real job via `workflow_call`, because `needs:` only
resolves job names within one workflow file — a `needs: ci` pointing at a job in
`ci.yml` would have been a gate that looks like a gate and does nothing. The
publish jobs need that job, and since a needed job that fails *or is skipped*
skips its dependents by default, and none of these jobs carries an `if:` that
could override that, there is no path from red CI to a registry.

Permissions came down at the same time: the workflow default was `contents: write`
+ `id-token: write` applied to every job, including the two that push to public
registries. Default is now `contents: read`; `release` gets `contents: write`
because it deletes and recreates the `latest` release and pushes a tag deletion;
the publish jobs get `contents: read` plus `id-token: write` for npm provenance
and PyPI trusted publishing.

The cost is that the `latest` GitHub release the README tells people to `curl` no
longer refreshes on every push — it refreshes when someone tags or dispatches.
That is the intended trade.

This is a stopgap, not the design. Everything about *what* gets released is
untouched on purpose: the timestamp version scheme, the delete-and-recreate of
`latest`, missing checksums and signatures for downloadable binaries, and absent
third-party license bundling. Those are the real release redesign, tracked as P1
in `ASKS.md`, and several of them are the human's call rather than an agent's.

## 2026-09-01 — Project process setup

Adopted AI-assisted development conventions via `/project-setup`: created `CLAUDE.md` (working rules: test-first, atomic commits, pre-commit `make test`/`make lint`, changelog discipline), this diary, and `CHANGELOG.md`. The existing `TODO.md` stays as the bug/task tracker in its current sectioned format.

Context on where the project stands: core commands, TUI viewer, remote file access (S3/GCS/Azure/HTTPS), docs site with generated CLI reference and demo GIFs, automated binary releases plus npm/PyPI publishing, and hosted example data at `data.pqtool.dev` (Cloudflare R2). Open threads are in `TODO.md` — notably `diff`, `repack`, `sort`, `schema evolve`, and the missing TUI viewer demo GIF on the docs site.
