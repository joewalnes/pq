# Asks

The human's own requests. These outrank everything in `TODO.md` — rank by origin, not volume.
One agent must always be working the top open item.

Format: `- [ ] **P<n>** Title` with indented detail. Newest asks go at the bottom of Open unless
they are urgent, in which case say so explicitly.

## Open

- [ ] **P1** Redesign the release process properly
  Raised 2026-09-01. The interim fix (gate `release.yml` behind CI, move off the
  push-to-`main` trigger) is a stopgap to make unattended work safe — it is not the
  design. Wanted: a deliberate release process. Open questions to answer as part of it:
  real semantic versions instead of `0.1.$(date +%Y%m%d%H%M)`, how a release is
  triggered and by whom, whether `latest` should keep being deleted and recreated,
  checksums/signatures for the binaries the README tells people to `curl` and run,
  and third-party license bundling (Apache-2.0 attribution currently ships with
  nothing on any channel).

## Done
