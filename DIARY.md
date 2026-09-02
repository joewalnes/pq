# Engineering Diary

Latest entries first. Record significant decisions, architecture changes, and non-obvious context — the *why* that isn't visible in commits.

---

## 2026-09-02 — Published tutorials left un-migrated; README claim narrowed instead

`docs/src/tutorials/*.md` (rendered onto the docs site by `docs/build.py`,
a plain-Markdown-to-HTML pipeline) and `tests/golden/tutorials/*.md` (the
same five tutorials, executed by `tests/golden/run.py` against real
command output) have diverged by ~58-172 changed lines each — confirmed by
diff, not by inspection. README.md claimed "the tutorials" double as tests,
which is only true of the golden copies; a reader has no way to know the
published copies aren't the ones being checked. Real fix is `docs/build.py`
parsing the golden `console`/`file:` DSL directly and rendering it in the
docs site's current style, so there is exactly one tutorial source; that's
a genuine (if bounded) parser-plus-renderer addition, and getting it wrong
would break `make docs` for every future writer of these files with no
test of its own to catch that. Chose the smaller honest fix instead:
narrowed the README claim to name `tests/golden/tutorials/` specifically
and say plainly that the published copies aren't covered, and logged the
migration as a tracked TODO item rather than leaving it undiscoverable. An
accurate README plus a disclosed gap beats a half-migrated docs build.

## 2026-09-02 — `-f` semantics for `export`/`sql -o`: extension by default, explicit flag wins, undetermined is an error

Confirmed bug: `pq export a.parquet -o a.parquet -f csv` wrote JSONL into a
file named `.parquet`, exit 0, no diagnostic — `-f` was never consulted
once output went to a file; the extension always won, and an unrecognized
extension (including `.parquet` itself, which `export` can't produce)
silently fell back to JSONL. `sql -o` had the identical shape via
`write_output::write_batches_to_file`, which only ever infers from the
path. Considered three options: extension always wins (matches existing
`cat -O`/`jq -o` precedent, but a typed flag being silently discarded is
exactly the bug); `-f` always wins (breaks the common, unremarkable case of
`-o out.csv` picking csv with no flag typed); explicit-`-f`-wins (chosen).
Rule: extension governs when `-f` wasn't typed or agrees with it; an
explicit, disagreeing `-f` overrides the extension but prints a stderr note
naming both, so the loser is never silent; if neither a recognized
extension nor an explicit `-f` pins down a format, that's a hard error
instead of a guess. `sql -o out.parquet` is the one asymmetry: `-f` has no
"parquet" value, so `.parquet` always wins there, `-f` or not (noted on
stderr when an explicit `-f` is thus overridden). Also fixed, same file:
`export -f csv` to stdout had no CSV branch at all and silently fell
through to JSONL — CSV writing already existed for the to-file path
(`write_csv`, extracted and shared by both). Not fixed: `cat -O` and `jq
-o` have the identical extension-always-wins shape and are out of this
worker's file scope (`export.rs`/`sql.rs`/`cli.rs` only); `select`/`slice`/
`merge`/`split`/`import` all show `-f` in `--help` (it's a global flag) but
silently ignore it since they only ever write real Parquet — confirmed by
running each, left as a separate finding.

## 2026-09-02 — HTTP remote tests run for real, no Docker

The previous entry accepted "remote regressions won't be caught by CI"
because the only harness available was SeaweedFS via Docker. For the HTTP
half that trade-off wasn't actually necessary: pq talks to HTTP via plain
range requests, so a hand-rolled `std`-only HTTP/1.1 server (`TcpListener`
on `127.0.0.1:0`, GET+HEAD, single-range `Range:`/`206`) is enough to
exercise it, with no new dependency. Ported all 10 `test_http_*` cases
onto it and un-`#[ignore]`d them — they now run in `cargo test --workspace`
with nothing but the test binary. `test_s3_*` still needs SeaweedFS and
stays `#[ignore]`d.

The one bug this caught in itself, not in pq: the server's HEAD response
carried Content-Length *twice* (once derived from the empty HEAD body,
once from an `extra` header meant to hold the real size) — the HTTP
client believed the first, saw a 0-byte object, and every ranged read
failed before it started. Content-Length must be decoupled from the
bytes actually written on the wire; a HEAD response describes a resource
it isn't sending.

Verified non-vacuousness by fault injection, reverted after each: serving
`test_data.parquet` disabled-Range makes `test_http_info` fail (`object_store`
treats a 200 reply to a Range request as an error, never a silent whole-file
read); swapping in the wrong fixture makes `test_http_count` fail on the row
count; pointing at an unreachable port fails loudly after retries rather than
hanging or passing. Per-request logging on the server also shows pq's actual
request shape: 1 HEAD + 2 ranged GETs (an 8-byte footer-length probe, then
the footer) for metadata-only commands, +1 more ranged GET for row data on
commands that read rows — never a full unranged download, and `cat -c` reads
a visibly narrower byte range than a full-row read, so column projection is
happening before the bytes leave the wire.

## 2026-09-01 — Remote-file tests stay `#[ignore]`d, not moved into the default suite

`crates/pq-cli/tests/remote_tests.rs` (19 tests covering HTTP-range and S3
access via SeaweedFS) had never run — every test used `-O <format>`, a
flag that doesn't exist on most subcommands (clap: "unexpected argument
'-O' found") and on `cat` specifically means `--output <file>`, so two
tests silently wrote a stray file named `jsonl` and asserted on empty
stdout. Fixed the flag (`-f`), fixed two nested-type assertions that
checked only for a top-level key's presence — weak enough to keep
passing even if remote nested-type decoding silently dropped
struct/list fields, a real historical bug class in this codebase (see
`pq-transform::schema_inference::tests::list_nested_in_struct_is_not_dropped`)
— and fixed a latent cross-binary bug where `remote_tests.rs` assumed
`nested_data.parquet` already existed in `tests/fixtures/` because
`cli_tests.rs` happened to generate it there; `make test-integration`
runs only `remote_tests`, so that file was never actually present on a
clean `make test-integration` run. It now generates its own copy into a
private `TempDir`.

Decision: keep all 19 `#[ignore]`d, gated behind `make test-integration`
/ `cargo test -- --ignored`, rather than moving them into the default
`cargo test --workspace` suite. CI runs `cargo test --workspace` with no
Docker available; un-ignoring would turn every PR red on a dependency
CI can't satisfy. The trade-off this accepts: remote-access regressions
are only caught when a human or agent explicitly runs the Docker-backed
suite, not on every push. What was fixed instead is the failure mode
when someone does run them without SeaweedFS up — confirmed each test
now fails loudly (`s3 upload failed for ...`, assert panic, nonzero
exit) rather than silently skipping or passing for the wrong reason.

## 2026-09-01 — CSV header: union of all columns, not row 0's

Every CSV writer in the crate froze its header from the first row/batch,
then wrote each row's values by iteration order with no per-row key
lookup. Combine two Parquet files with different schemas (`pq cat
a.parquet b.parquet -o out.csv`) and a value from the second file lands
either under the wrong column name or nowhere at all — silently, exit 0.
It has nothing to do with key *order*: this codebase's `serde_json` has
no `preserve_order` feature, so `Value::Object` is a `BTreeMap` and
always iterates alphabetically; the corruption is entirely from
differing key *sets* against a header that never grows.

Decision: the header is now the union of every input's columns
(first-seen order), and every row is looked up by column name against
it — a column absent from a given row/file gets an empty field, never a
dropped value. This costs a second pass to compute the union, but
that pass is over data already fully resident: `write_output.rs`'s
batch/values writers receive an already-collected slice, and
`export.rs`'s CSV path gets its union from a cheap Parquet-footer-only
metadata read per file (no row data), so the row-writing pass still
streams file-by-file exactly as before. Emitting empty for a missing
key was the only alternative that doesn't reintroduce the same class of
bug (a value the user has that never reaches the output).

Also replaced three independent hand-rolled CSV escapers (each checking
only `,`, `"`, `\n` — missing a bare `\r`, which a compliant reader
treats as its own record terminator) with the `csv` crate, already a
declared-but-unused dependency.

## 2026-09-02 — Peer review of the licensing/wheel work found real bugs

A verifying peer built and installed the wheel/license changes below rather
than trusting the report, and found four real defects: generated
`THIRD-PARTY-LICENSES` files were untracked-but-not-gitignored (dirtied the
shared checkout and blocked the merge gate for every other agent); the
cargo-about install check used `command -v`, which misses `~/.cargo/bin`
when it's off `$PATH` (it was, on this machine); the NOTICE-appendix scratch
file used a fixed `/tmp` path (a collision hazard the CLAUDE.md singleton
warnings already called out); and `release.yml` never actually ran `make
licenses`, so the gap noted below as "deferred" would have failed the first
real release. All four are fixed on this branch, each reproduced against
the broken behavior before fixing, same as the original work. Lesson worth
generalizing: "I noted the gap in a comment" is not the same as "someone
will act on the gap" — a landmine documented next to where it will detonate
is still a landmine.

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

## 2026-09-01 — Tagline drift: picked the ASCII hyphen, deduplicated to one constant

`pq --help` (from `cli.rs`'s clap `about`) and `pq capabilities` (a
machine-readable JSON command aimed at scripts/agents) each hand-carried
their own copy of the one-line tagline "A Parquet Swiss Army Knife
{-,—} inspect, query, transform, and view Parquet files" — `cli.rs` had
drifted to an ASCII hyphen while `capabilities.rs` still had an em-dash,
and the golden expectation in `tests/golden/tests/help-output.md`
expected the em-dash, so the golden suite was red against `cli.rs`'s
actual output.

Decision: ASCII hyphen. Reasoning — `README.md` and `docs/src/index.md`
(the project's actual public-facing self-description) don't use an
em-dash for this sentence at all; scanning the whole public docs/
README surface for em-dash usage turns up zero occurrences, versus 21
in `CLAUDE.md` (an AI-agent-facing doc, not user-facing product text).
`pypi/build_wheels.py`'s package Summary field also already uses the
hyphen. `cli.rs` is what actually ships to every `pq --help` invocation
and is the oldest/most load-bearing copy of the three. All the
evidence points to hyphen being this project's established voice for
its own tagline, and the em-dash in `capabilities.rs` (a newer,
JSON-output-only command) and in the golden expectation as the
drift — not the other way around.

Fix, not just find-and-replace: added `pub const TAGLINE` to `cli.rs`
and pointed both clap's `about` and `capabilities.rs`'s JSON
`description` field at it, so the string exists in exactly one place in
the source. Added `crates/pq-cli/tests/cli_tests.rs::
test_tagline_matches_between_help_and_capabilities`, which runs both
commands and asserts the same literal tagline appears in each — proven
to fail on the pre-fix code (checked out via a throwaway
`git worktree add --detach ... main`, not stash) and pass after.
Left untouched: `docs/src/cli-reference.md` (generated by
`docs/generate-cli-reference.sh` from `cli.rs`'s own help text — will
pick up the fix next `make docs` run, not hand-edited) and
`pypi/build_wheels.py`'s Summary field (already hyphen; a separate
hand-copy for package metadata, out of this task's stated scope of
`cli.rs`/`capabilities.rs`).

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

## 2026-09-01 — Output aliasing: why stage-and-rename beat a path check

Five writers opened their destination with `File::create` before their input readers had produced a single row. When `-o` named an input, the input was gone: `merge`/`select`/`slice` left a 4-byte `PAR1` stub, `export` left zero bytes, and `pq import data.csv -o data.csv` was the worst of them — it truncated the CSV, then read zero rows from the file it had just emptied, wrote a valid empty parquet, printed "Converted 0 rows" and exited 0. Silent, total, and plausible enough that a user would not look twice.

The obvious fix is to refuse when output resolves to an input. It is the wrong fix, for two reasons. First, deciding whether two paths name the same file is not something you can do cheaply and correctly: `./a.parquet` vs `a.parquet`, symlinks, hardlinks, and a case-insensitive APFS all defeat string comparison, and dev+inode comparison — the only test that actually works — is Unix-only and still has to be threaded through every call site as an extra argument. Second, even a correct check only converts data loss into an error message, when what the user asked for is a perfectly reasonable operation.

So the whole class is handled by never truncating the destination in the first place. `pq_transform::output_guard::with_atomic_output` reserves a hidden sibling temp file in the destination's directory, hands that path to the writer, and `rename`s it over the destination once the writer has returned successfully. The input keeps its inode and its bytes for the entire duration of the read, so aliasing needs no detection at all — every disguise is handled by construction. It also buys atomicity for free: a merge that blows up mid-stream on a schema mismatch used to leave the destination as a stub, and now leaves it exactly as it was.

The costs are real but small. Staging resolves symlinked destinations first so the link survives the rename rather than being replaced by a regular file, and copies the destination's permissions onto the temp file so a rename cannot widen them. Destinations that already exist as something other than a regular file — a fifo, `/dev/stdout` — fall back to writing the path directly, because renaming over a character device is not what anyone means. Remote destinations are untouched by this: none of these writers can emit to `s3://` today (they all go through `std::fs`), and a URL-shaped path has no existing parent directory, so it takes the same fallback and produces the same error it always did. That is reasoned, not measured — `make test-integration` was not run.

The helper is a single choke point on purpose. The check being a property of one function rather than a line pasted into five files is what makes the guard a class assertion instead of five call-site assertions.

## 2026-09-01 — Project process setup

Adopted AI-assisted development conventions via `/project-setup`: created `CLAUDE.md` (working rules: test-first, atomic commits, pre-commit `make test`/`make lint`, changelog discipline), this diary, and `CHANGELOG.md`. The existing `TODO.md` stays as the bug/task tracker in its current sectioned format.

Context on where the project stands: core commands, TUI viewer, remote file access (S3/GCS/Azure/HTTPS), docs site with generated CLI reference and demo GIFs, automated binary releases plus npm/PyPI publishing, and hosted example data at `data.pqtool.dev` (Cloudflare R2). Open threads are in `TODO.md` — notably `diff`, `repack`, `sort`, `schema evolve`, and the missing TUI viewer demo GIF on the docs site.
