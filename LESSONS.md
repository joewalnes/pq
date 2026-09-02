# Lessons

Append only when something actually bites. Before adding, read the existing entries and look
for one to sharpen instead — a better version of an existing lesson beats a near-duplicate.

---

## A red baseline turns every gate into a coin flip
**What happened:** `CLAUDE.md` mandated "do not commit if tests fail or lint errors are
present", but `make test` and `make lint` were both red on a clean checkout at `f0db2ea`
(2 stale golden expectations, 1 clippy error, 3 fmt violations). Any agent obeying the rule
literally was blocked from committing anything before it started, and its cheapest escapes
were all destructive: burn the session on unrelated lint, run `tests/golden/run.py --update`
to overwrite the expectations, or declare the gates known-broken and commit anyway — which,
via `release.yml`'s push-to-`main` trigger, would have published untested code to npm and PyPI.
**What it cost:** Nothing yet — caught by a `/scorecard` audit before an unattended run started.
Had it not been, the likely cost was a destroyed golden suite or a bad public release.
**The rule that would have prevented it:** A verification recipe must be green before a crew is
dispatched against it. Greening the gates is always the first task of a run, never a
background chore — and it must land before any parallel work, so agents can tell their own
breakage from inherited breakage.
**Scope:** general

## An escape hatch documented next to a failing gate will get used
**What happened:** `tests/golden/run.py --update` rewrites expectation files in place from
current binary output. It is documented at `run.py:11`, requires one word, and nothing marks
it as dangerous or restricts it. With two golden tests failing, it was the path of least
resistance to a green suite — and would have silently ratified whichever regression caused
the failure.
**What it cost:** Nothing yet; guard-blocked in `CLAUDE.md` before the first run.
**The rule that would have prevented it:** Any tool that can convert a real failure into a
green result must be named and forbidden in the project's agent config, not merely left
undocumented. Assume every documented affordance adjacent to a red gate will be discovered.
**Scope:** general
