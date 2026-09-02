# Asks

The human's own requests. These outrank everything in `TODO.md` — rank by origin, not volume.
One agent must always be working the top open item.

Format: `- [ ] **P<n>** Title` with indented detail. Newest asks go at the bottom of Open unless
they are urgent, in which case say so explicitly.

## Open

- [ ] **P1** Redesign the release process properly — **implemented; blocked on one human action**
  Raised 2026-09-01. All three decisions the human made on 2026-09-02 are
  implemented in `release.yml` (branch `p1-release-redesign`, 2026-09-02):

  - [x] version from the tag, derived once in a new `preflight` job and consumed
        via job outputs; the `0.1.$(date …)` scheme is gone. `workflow_dispatch`
        from a branch now fails in `preflight`; dispatching against a tag works.
  - [x] `latest` delete/recreate dropped — the release is created for the tag,
        with `--latest` so GitHub's `/releases/latest/` redirect resolves to it.
        No `gh release delete`, no tag deletion, no `|| true` left in the file.
  - [x] `SHA256SUMS` published as a release asset; README documents verifying
        a download against it.
  - [x] `LICENSE` + `THIRD-PARTY-LICENSES` shipped as release assets, reusing the
        existing `licenses` job's artifact.
  - [x] `dtolnay/rust-toolchain`, `pypa/gh-action-pypi-publish` and
        `cargo install cross --git` pinned to exact commits (the cross pin is
        duplicated in the `Makefile` and must be bumped with it).
  - [x] the workflow now refuses to start if `NPM_TOKEN` is empty, so a missing
        credential can no longer burn a version number behind an immutable
        GitHub release.

  **What remains — the human's, not an agent's:** add the `NPM_TOKEN` repository
  secret (it does not exist: `gh secret list` is empty, the Actions secrets API
  reports `total_count=0`) and confirm the `pypi` environment's trusted publisher
  is configured. Until `NPM_TOKEN` exists, tagging `v0.1.0` will now stop in
  `preflight` having created and published nothing — the tag stays reusable. See
  the P1 Infrastructure entry in `TODO.md`.

  **Added 2026-09-02 (branch `l2-publish-atomicity`), after an adversarial pass:**

  - [x] the two publishes are no longer parallel siblings — `publish-pypi` now
        needs `publish-npm`, so the version can no longer end up live on PyPI
        and absent from npm. The reverse is still reachable; there is no way
        to make two registries atomic, and the workflow no longer implies
        there is.
  - [x] a new `package-check` job runs before `release` cuts anything
        immutable: artifacts present, neither registry already holds the
        version, all four npm packages pack cleanly, three wheels build and
        pass `twine check`.
  - [x] the run's job summary is now a ledger written as each publish
        succeeds, plus a pre-written playbook for a failed PyPI upload, so a
        partial release names itself instead of being discovered later.
  - [x] `publish-npm` would have failed on its first `cp` — `npm/<platform>/bin/`
        is not in git. Fixed. See DIARY.md 2026-09-02.

  Not verified by any agent: nothing here has been observed in a real Actions run.
  Verification was YAML parse, in-file `needs:` resolution, and executing the
  workflow's own shell scripts locally against good and bad inputs. Publishing,
  tagging, pushing and triggering workflows were all guard-blocked.

  Already done earlier under this ask: `pypi/build_wheels.py`'s broken console
  entry point, `THIRD-PARTY-LICENSES` generation via `cargo-about` and bundling
  into npm packages and wheels, and wiring `make licenses` into `release.yml`.

## Done
