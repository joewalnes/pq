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

## A harness must assert the identity of its subject, not merely its availability
**What happened:** Twice in one session, a check reported numbers while never reaching the
code under test. (1) `tests/golden/run.py` resolved the binary by putting its *directory* on
`PATH` and invoking bare `pq`. With a missing or typo'd `PQ`, `pq` fell through to a stale
`brew`-installed build: `PQ=/nonexistent/pq ... run.py help-output.md` printed **"4 passed,
1 failed"** — four blocks passing against a binary never compiled from this tree. It exited 1
only incidentally, because one block happened to differ; had the stale binary matched, the
suite would have reported fully green. (2) The foreman's own CLI drive of a data-loss fix
reported four clean "INPUT INTACT" results. The shell is zsh, which does not word-split
unquoted variables, so `$PQ $cmd` passed the whole command string as a single filename and
every invocation died in argument parsing. Both looked exactly like passes.
**What it cost:** Nearly ratified a fix on evidence that touched none of the changed code.
Caught only because the two results disagreed with a third measurement.
**The rule that would have prevented it:** A test that cannot reach its subject must fail
loudly, not pass. Resolving a subject by *name* through an ambient search path is the failure
mode — it silently substitutes a different instrument and still produces a number. Assert the
identity of what you are testing (absolute path, and check it is the artefact you just built),
and check stderr, not just exit codes. When a measurement looks clean, ask what it would have
printed had the subject never been invoked at all.
**Scope:** general

## An intermittent gate reports the state of the dice, not the state of the code
**What happened:** The merge gate passed on one merge, then failed on the next — a merge that
changed only YAML and prose and could not possibly have broken a Rust test. Because that
failure was *impossible*, it got investigated instead of retried, and the investigation found
the gate had been non-deterministic all along: the `nested_jq` tests failed ~40% of runs
(measured 4/10 at the exact commit the gate had just declared green). `cli_tests` regenerates
an untracked fixture via `pq import`, and the truncate-before-write bug left a window where
the output file existed but was empty, which the `if parquet.exists()` guard then accepted.
**What it cost:** One merge was gated and declared green on a coin flip. Had the next merge's
failure been plausible rather than impossible, the standard response — rerun the flake — would
have buried it, and the crew would have spent the session unable to distinguish real breakage
from noise.
**The rule that would have prevented it:** A gate is a measurement, and a measurement taken
once has no error bar. When a gate fails on a change that cannot have caused it, that is
evidence about the gate, never about the change — never retry it, always explain it. Establish
a flake rate by repetition before trusting a green, and treat "passed once" as unverified.
**Scope:** general

## Verifying a fix on the cases it was designed to fix proves only that it was written
**What happened:** A fix replaced "truncate the destination, then write" with "stage to a
sibling, then rename over the destination", to stop `pq merge a.parquet b.parquet -o a.parquet`
destroying its own input. The foreman verified it by driving exactly that: every in-place
command, correct row counts, no litter. It passed, and was merged and pushed. An adversarial
round then broke it in minutes by attacking what the *new mechanism* needs rather than what the
old bug did. `rename()` requires write permission on the **directory**; `File::create()`
required it on the **file** — so the fix walked straight through a `chmod 444` that the old
code had refused, silently, exit 0, and then copied mode 0444 onto the replacement so `ls -l`
still showed the file as protected. Separately, the guard's `Err(_) => return write(dest_path)`
fallback meant any failure to create the staging file silently reverted to the original
destructive path, leaving the worst bug of the round — a silent, exit-0, emptied CSV — fully
reachable via a read-only parent directory, a 254-character filename, or stale staging litter
plus pid reuse.
**What it cost:** A silent, irreversible data-loss regression reached `origin/main`. The
happy-path verification was real and correct; it was simply blind by construction.
**The rule that would have prevented it:** When a fix changes the *mechanism*, the old bug's
test cases no longer bound the risk. Ask what the new mechanism requires that the old one did
not — different syscall, different permission, different resource, a new failure branch — and
attack those. Give particular weight to any fallback path: a `catch`/`Err(_)` that quietly
resumes the behaviour the fix existed to remove converts a fix into a narrowing, and narrowing
is not fixing. Schedule the adversarial pass against the *previous* round's fixes as
standing work, not as a treat for spare capacity.
**How often this happens, measured:** in a later round of the same run, an adversarial pass
over four merged branches found that **three of the four had each introduced a fresh defect** —
a CSV rewrite whose stated purpose was to stop silently dropping data introduced a new silent
column drop; a panic fix over-corrected and began rejecting files the underlying operation
handles fine; and a "make the docs true" commit introduced a new false claim. Every one had
passed review, a full green gate, and CI. At a defect-injection rate that high, an adversarial
pass scheduled as a *follow-up round* rather than as part of the merge would have shipped all
three. Treat the hunt as a step in the merge, not as work that happens afterwards if there is
capacity — and note that the fixes most likely to inject a defect are the ones whose authors
were most careful, because care concentrates on the case they already understood.
**Scope:** general

## A destructive step that cannot fail loudly will eventually destroy something
**What happened:** `release.yml` maintained one mutable GitHub release tagged `latest` and
recreated it on every run:

    gh release delete latest --yes 2>/dev/null || true
    git push origin :refs/tags/latest 2>/dev/null || true
    gh release create latest ...

It worked fourteen times. On the fifteenth the delete succeeded and the create failed
(`HTTP 422 ... tag_name was used by an immutable release`). A project with fourteen successful
release runs was left with **zero releases**, and the `curl` URL its own README documents
returned 404 — not because the workflow failed to make something new, but because it destroyed
the only release it had and could not put it back.
**What it cost:** The project's entire published distribution, and it went unnoticed until an
agent audited why installs were broken.
**The rule that would have prevented it:** Look for the shape, not the command: a destructive
first half suppressed with `|| true` (or `2>/dev/null`, or an ignored exit code), followed by a
recreate whose failure is *unrecoverable* rather than merely unsuccessful. The suppression
guarantees the destructive half can never fail loudly, so the only signal anyone ever sees
comes from the half that cannot undo it. Prefer operations that never leave a gap — create the
new thing first and move a pointer, or use a name that is never reused — over delete-then-
recreate against a shared mutable identity. And treat a long run of successes as weak evidence
about a rare failure branch: fourteen prior successes are precisely why nobody looked.
**Scope:** general

## A confirmed finding that lives only in a report is indistinguishable from one nobody found
**What happened:** A reconnaissance agent confirmed four data-correctness bugs and returned them
in one report. The foreman dispatched fixes for three — the output-aliasing cluster, mixed-type
JSON columns being NULLed, and the CSV pair — and never dispatched the fourth. It stayed live in
`main` for the rest of the run: `pq cat` rendering a `decimal256(40,2)` value of `123.45` as
`"12345.2"`, and collapsing `1.23` and `123.00` to the identical string `"123.2"`, so the error
was not even recoverable from the output. It was found only when the foreman went back and
re-read the original report against current source. A deliberate sweep of every other confirmed
finding then turned up two more in the same state — `stats --describe --sample-size` silently
reporting on the first file only, and the atomic output guard breaking hardlinks and dropping
xattrs and ACLs. Nine findings checked, seven already handled, **two still unassigned: a 22%
leak rate on work that had already been paid for.**
**What it cost:** A 100x-wrong financial-precision rendering shipped in every build for a full
run, plus two silent-wrong-answer bugs, none of them new discoveries — all three had been found,
reproduced and written down hours earlier.
**The rule that would have prevented it:** None of the usual machinery watches the gap between
*confirmed* and *assigned*. A merge gate cannot — there is no test for a bug nobody fixed. An
adversarial pass cannot — it attacks what changed, not what was never touched. Verification
cannot — you only verify what you dispatched. So reconcile findings against dispatches
explicitly and in writing: when a report returns N findings, enumerate them, and record for each
one whether it was fixed, dispatched, or deliberately logged. Splitting one multi-item finding
across several dispatches is the specific shape that leaks, because each dispatch looks complete
on its own. Do the reconciliation from the original report, never from recollection of what you
sent.
**Scope:** general
