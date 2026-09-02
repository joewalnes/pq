# Asks

The human's own requests. These outrank everything in `TODO.md` — rank by origin, not volume.
One agent must always be working the top open item.

Format: `- [ ] **P<n>** Title` with indented detail. Newest asks go at the bottom of Open unless
they are urgent, in which case say so explicitly.

## Open

- [ ] **P1** Redesign the release process properly — **decisions made, implementation outstanding**
  Raised 2026-09-01. The interim fix (gate `release.yml` behind CI, move off the
  push-to-`main` trigger) was a stopgap to make unattended work safe, and it landed.

  The three open questions were answered by the human on 2026-09-02:
  1. **Keep all three channels** — GitHub Releases, npm, and PyPI.
  2. **Manual semver, driven by a git tag.** A human tags `v0.1.0`; the tag triggers
     the release and the version is read from the tag. Delete the
     `0.1.$(date +%Y%m%d%H%M)` scheme entirely.
  3. **No `latest` tag at all.** GitHub already redirects
     `/releases/latest/download/<asset>` to the newest release, so remove the
     delete-and-recreate dance from `release.yml`. README URLs already use the
     pointer form and need no change.

  Still to implement:
  - version from the tag, in one place (both publish jobs currently compute
    `date` independently and can disagree across a minute boundary)
  - drop the `latest` delete/recreate
  - publish `SHA256SUMS` alongside the release assets
  - ship `LICENSE` + `THIRD-PARTY-LICENSES` in the **release assets** (already done
    for npm packages and wheels) — the `curl`-a-binary user is the one channel that
    still receives no attribution
  - pin `dtolnay/rust-toolchain` and `pypa/gh-action-pypi-publish` (both are
    *branches*, not tags) and the `cargo install cross --git` reference
  - **Before the first real tag:** confirm why publishing has never succeeded. See
    the Infrastructure entry in `TODO.md` — the credentials have never been proven
    to work, and `v0.1.0` will fail the same way if they are still absent.

  Already done under this ask: `pypi/build_wheels.py`'s broken console entry point,
  `THIRD-PARTY-LICENSES` generation via `cargo-about` and bundling into npm packages
  and wheels, and wiring `make licenses` into `release.yml`.

## Done
