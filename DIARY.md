# Engineering Diary

Latest entries first. Record significant decisions, architecture changes, and non-obvious context — the *why* that isn't visible in commits.

---

## 2026-09-02 — The preflight version check guarded the credential, not the version

`preflight`'s job comment says "one source, one value," but its actual check —
`grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+$'` — validated shape, not value. Extracted the
`run:` block with a YAML parser and ran it under bash against creatable git tags:
`v01.2.3` passed as `01.2.3`, and `v999999999999999999999.0.0` passed as-is.
Neither is where the bug shows up — it shows up one job downstream. npm's
`publish --dry-run` on `01.2.3` silently rewrites it to `1.2.3` ("version was
cleaned and set to"); on the 22-digit major it hard-fails with "Invalid version".
Since `publish-npm` runs after `release`, the second case is the exact scenario
the job exists to prevent: an immutable GitHub release cut on a version number no
registry will ever let you reuse. The first case is worse in a different way —
nothing fails, so nothing gets noticed, and the release ships under three
strings (the git tag, `pq --version`, and npm's silently-corrected one).

I checked whether PyPI does the same leading-zero normalization rather than
inferring it — no `packaging` module in this environment, but `pip` vendors its
own copy, and `pip._vendor.packaging.version.Version("01.2.3")` also prints as
`1.2.3`. Same divergence, both registries. The oversized major is the opposite
story: PEP 440's reference parser accepts a 22-digit component without
complaint (Python bignums don't care), so there's no registry-side ceiling to
lean on there — the 9-digit cap in the new regex is pq's own choice, not a
mirror of anyone else's limit.

The other thing worth recording: `grep -Eq '^...$'` anchors per *line*, not per
string. A version value with an embedded newline could satisfy the pattern on
one line while carrying a second `version=` line into `$GITHUB_OUTPUT`, and
whichever line lands last wins for every consumer downstream. Git currently
refuses newlines in ref names, so `$REF` can't carry one today — but the
anchoring bug is independent of that fact, and I don't want to be relying on
git's ref-name rules to keep a `$GITHUB_OUTPUT` injection closed. Bash's
`[[ "$x" =~ $re ]]` anchors to the whole string; switched to it and confirmed a
literal `$'v1.2.3\nversion=9.9.9'` REF is now rejected outright rather than
partially matched.

## 2026-09-02 — A rename you can see beats a column you can't reach

Parquet lets two top-level columns share a name. `pq cat` and `pq export` carried
both through; `pq sql` returned one, exit 0, no warning — and the one it returned
was the *second* column's data under the first column's name, so `SELECT id` on a
two-`id` file quietly answered with the wrong column.

I did not want to guess where that happened, so I instrumented it: a probe that
printed the schema at each hop between the file and the result. The file has
`["id", "id"]` (arrow-rs, no DataFusion). The `TableProvider` that
`register_parquet` builds already has `["id"]`. That is the whole answer — the
column is gone at *registration*, before any logical plan, projection or writer
exists, which is why no output-side fix could ever have worked.
`DFSchema::try_from` on the file's own schema returns `["id", "id"]` happily, so
`DFSchema` — the suspect I had expected, since it documents a unique-name
requirement — is innocent. The culprit is `ParquetFormat::infer_schema` ending in
`Schema::try_merge`, whose `SchemaBuilder::try_merge` matches fields *by name*
and merges the second into the first. pyarrow refuses the same file with
"Can't unify schema with duplicate field names", which is the same operation
being honest about it.

The obvious repair is to hand `register_parquet` a pre-disambiguated schema via
`ParquetReadOptions::schema`. I tried it and threw it away on the evidence. The
provider then reports `["id", "id:1"]`, but the file reader still matches columns
by name, finds no `id:1` in the file, and fills the column with nulls. It only
errored in my probe because the fixture's fields are non-nullable; on a nullable
column it would have produced a full column of NULLs — a fix that looks like it
preserves data while destroying it. That is precisely the failure mode worth
being paranoid about, and it was one probe away from being shipped.

So: when a file's top-level names are not unique, and only then, pq reads it and
registers it under unique names. The rename is deliberately visible, with a note
on stderr, and it is *not* reversed on the way out. Reversing it would hand back
a result set whose column names cannot be typed back into a query — the exact
trap that made the second `id` unreachable — and it isn't reversible anyway, since
a file may legitimately contain both `id` and `id_1`. The generator skips names
already present in the file for that reason. The cost is real and worth stating:
such a file goes through `MemTable`, so it is materialized and loses pushdown.
Every file with unique names takes the old path untouched.

Writing the fix was the easy half. The mechanism it introduced needed a
metadata read the old path never did, and that broke something: a *directory*
named `foo.parquet` is a valid DataFusion table (`ListingTable` reads every file
under it), and reading it as a single parquet file fails with "Is a directory".
`SELECT * FROM 'somedir.parquet'` went from working to exit 1. I found it by
driving the fixed binary against a directory and comparing against a pre-fix
build, which is the only way I would have found it — no test I wrote for the
original bug goes anywhere near a directory. The check is now keyed on "is a
regular file", never on the check having failed, so an unreadable *file* still
errors out instead of quietly falling back to the behaviour this change exists
to remove. Directories of duplicate-named files remain collapsed; that is logged
rather than papered over.

## 2026-09-02 — Generating capabilities.rs instead of re-listing

`capabilities.rs`'s 197-line hand-duplicate of the clap tree had already
drifted once (its tagline diverged from `--help`'s). Rewriting it to
*exactly* preserve every field the old JSON had turned out to be the wrong
goal: several of those fields encoded semantic knowledge clap genuinely
doesn't have (a `String` field being "a path" or "a regex"), and pretending
otherwise would mean either lying via reflection tricks that behave
differently in debug vs. release builds, or hand-listing per-command
overrides that just reintroduce the 197 lines. The fix that actually
removes the drift risk: everything clap *can* derive (which commands and
args exist, required-ness, defaults, enum values) now comes straight from
`Cli::command()`, and the only hand-written part left is a ~16-entry table
mapping an argument's *id* (not per-command — the same field name means the
same thing everywhere in this CLI, e.g. every `file` is a path) to the one
semantic fact clap can't infer. That table is checked against the live clap
tree on every invocation, and a deliberately injected stale entry proved it
fails loudly rather than silently. Generating for real also surfaced two
drifts nobody had caught: the hand list had never mentioned
`stats --sample-size` or `cat --output`, and claimed `count`/`merge`'s file
arguments were `required: true` when clap's actual parse behavior
(confirmed by running `pq count` with zero files) lets them through to an
application-level error instead.

## 2026-09-02 — One version read from two places

`pq --version` and `pq capabilities`'s `"version"` field read from different
env vars (`PQ_VERSION`, build-time and tag-derived, vs. `CARGO_PKG_VERSION`,
permanently `0.1.0`) and so disagreed on every build that set
`BUILD_VERSION` — i.e. every real release. Routed both through one constant
and added a test that pins them together (proved it bites by reintroducing
the old `CARGO_PKG_VERSION` read and watching it fail with the exact
disagreement shown in the assertion message).

## 2026-09-02 — Three flags nobody read

`--color`, `-q`/`--quiet`, `-v`/`--verbose` were all declared, parsed, stored on
`Cli`, and never read again — confirmed by grep across every crate and, more
importantly, by diffing real output with and without each flag (identical
bytes in every case). Two different responses seemed right depending on the
flag. `--color` had a specific claim riding on it: `pq capabilities` already
asserted `"respects_no_color": true`, which was flatly false — there was no
color output anywhere in the renderer to respect or not. So `--color` got
wired for real (bold+cyan table headers, gated on `auto`/`always`/`never`
resolved against `NO_COLOR` and a real TTY check), making that claim true
for the first time. `-q`/`-v` had no such anchor — nothing in this codebase
ever promised a quiet or verbose mode, and inventing what they should do is a
product decision, not a cleanup. Deleted instead of implemented. That's a
real behavior change (clap now rejects them with exit 2) and is called out
in the changelog rather than absorbed silently.

## 2026-09-02 — One version, derived once, and a release that refuses to start half-doomed

Two jobs each running `date +%Y%m%d%H%M` looked like duplication. It was worse
than that: `publish-npm` and `publish-pypi` run in parallel, so a run that
straddles a minute boundary publishes *different* version numbers for the same
commit to two registries, permanently. Nobody would notice until someone tried to
match an npm version to a wheel. The version now comes from the tag, is derived
once in a `preflight` job, and reaches everything else as a job output — including
the binaries' `BUILD_VERSION`, so `pq --version` finally says what was published.

`workflow_dispatch` with no tag had to mean something. A `version` input was the
obvious answer and is the wrong one: it lets a human publish a version with no
immutable git tag behind it, which is the same git-versus-registry divergence the
change exists to close. So a dispatch from a branch fails in preflight, and
dispatching *against a tag* — which the ref picker allows — still works as a
re-run. Only plain X.Y.Z is accepted, because `v0.1.0-rc.1` is valid semver and
invalid PEP 440, and I would rather reject a pre-release than silently publish it
under two different strings.

The fail-fast question was the interesting one. `release` creates the GitHub
release before `publish-npm` runs, and the repository's rules make that release
immutable — the ordering that turned an ordinary failure into a destroyed release
last time. `NPM_TOKEN` does not exist in this repo today, so tagging `v0.1.0` right
now would produce an immutable release plus a failed publish on a version number no
registry will ever let us reuse. A one-line presence check in a job that already
runs first converts that into a clean refusal with nothing created. It is a floor,
not a guarantee: an expired token is non-empty and still fails at publish, and
there is no equivalent check for PyPI's OIDC trusted publishing. Adding the
credential remains the human's job; the workflow now just declines to burn a
version while waiting for it.

## 2026-09-02 — Why a project with 14 successful releases has zero releases

Recording this as evidence rather than preference, because it is the concrete
argument for the release design the human chose.

`release.yml` maintained a single mutable release tagged `latest`, and every run
recreated it: `gh release delete latest --yes || true`, then
`git push origin :refs/tags/latest || true`, then `gh release create latest`. That
worked 14 times on 2026-04-11/12. On 2026-09-02, run `33579005702`, the delete
succeeded and the recreate then failed:

    HTTP 422: Validation Failed
    pre_receive Repository rule violations found
    Cannot create ref due to creations being restricted.
    tag_name was used by an immutable release

So a project with fourteen successful release runs now has **zero releases**, and
the README's documented `curl` install URL returns 404. The workflow did not fail
to create something new; it destroyed the only release it had and could not put it
back. Both surrounding commands are `|| true`, so the destructive half could not
fail loudly even in principle.

The general shape is worth naming: a delete-then-recreate against a shared mutable
name has a window where the artefact does not exist, and any failure in the second
half is unrecoverable rather than merely unsuccessful. Idempotence was assumed
because it had held fourteen times.

Hence the chosen design: no mutable `latest` tag at all. Releases are immutable
`v<semver>` tags pushed by hand, and the "newest release" pointer is GitHub's own
`/releases/latest/download/<asset>` redirect, which needs no tag of that name and
nothing to delete. `README.md` already used that pointer form, so it needs no
change; the Homebrew formula in the separate `joewalnes/homebrew-tap` repo uses the
`/releases/download/latest/` *tag* form and does.

One correction to my own earlier record: I described the tag name `latest` as
"permanently burned". The 422 above is real, but it was observed once and never
retested, and observing a refusal once establishes only that it refused once. It is
moot under this design, which never creates that tag again.

## 2026-09-02 — README's `-o`/`-O` format claim narrowed, not extended, to stay true

The "make the docs true" pass a few entries below fixed `-f` handling for
`sql`/`export -o` and then wrote one sentence covering all four file-writing
flags (`sql`/`jq -o`, `export -o`, `cat -O`) as if they'd all gotten the same
treatment. Confirmed only two had: `pq jq src.parquet '.' -o jq.csv -f json`
writes CSV (the `-f json` is silently dropped), and `pq cat src.parquet -O
cat.txt` silently writes JSONL with no note — same silent-default shape the
`sql`/`export` fix just closed, just not closed here too. Two ways to make
the sentence true: narrow it to what's real, or extend the fix to `jq`/`cat`.
The product case for extending is clean — it's the identical bug, just
unfixed in two more places — but `jq -o`/`cat -O` funnel through
`write_output.rs`'s `write_batches_to_file`/`json_values_to_file`, which
take no format parameter at all and call `File::create` directly, bypassing
`output_guard.rs::with_atomic_output` entirely (contradicting that module's
own doc comment that every `-o`-writing command goes through it). Giving
`jq`/`cat` the real fix means touching `write_output.rs` to add an explicit-
format parameter mirroring `sql.rs`'s `resolve_output_format`, and probably
`output_guard.rs` too — both owned by another agent working the write paths
right now. Half-implementing it in `jq.rs`/`cat.rs` alone (duplicating
private helpers that aren't visible outside `write_output.rs`) would produce
two more silently-inconsistent format resolvers, exactly the kind of partial
fix this project has been burned by before. Narrowed the README instead and
filed the real fix as a TODO naming both root causes precisely, so the gap
is disclosed rather than papered over.

## 2026-09-02 — Release decisions, and a correction to what was "burned"

The three open release questions are answered: keep all three channels, drive
releases from a hand-pushed `v*` semver tag with the version read from the tag, and
stop creating a `latest` tag at all — GitHub's own `/releases/latest/download/`
redirect already does that job, and the README was already using the pointer form.

Two things in the record needed correcting, both mine.

First, I described the tag name `latest` as "permanently burned". The evidence was
real — run `33579005702` failed with `HTTP 422 ... tag_name was used by an immutable
release`, after its own `gh release delete latest` had succeeded — but "that 422
happened" and "the name is permanently unusable" are different claims, and I only
measured the first. It is moot under the agreed design, which never creates that tag
again, but the overreach is worth recording: a single error observed once is evidence
about a moment, not a permanent property.

Second, the reason nothing has ever been published is not the failure I kept citing.
Release has run 17 times: 14 succeeded in April 2026, before publishing existed. The
one run that reached the publish jobs, `578a2b6` on 2026-05-26, had `release` succeed
and *both* `publish-npm` and `publish-pypi` fail — and those logs have expired, so the
cause is unrecorded. The two September failures died earlier, in `release`, and
skipped publishing entirely. So the credentials have never been proven to work, and
the first `v0.1.0` tag will fail the same way unless someone checks first — after the
GitHub release exists, leaving it half-published. That is now a P1 in TODO.md, and it
is the single thing most likely to spoil the first real release.

## 2026-09-02 — Multi-file semantics for `tail`/`sample`: concatenation, not per-file

`tail`, `sample`, `count`, `merge` all silently mishandled >1 file (`tail`
used only the last, `sample` only the first, `count`/`merge` never expanded
globs). For `count`/`merge` the fix is mechanical: route through
`files::resolve_files` like every other multi-file command already does.
`tail`/`sample` needed a semantics decision since there's no existing
per-command precedent, only `cat`/`head`'s: treat multiple files as one
logical concatenation, in argument order. Chose to extend that same rule
rather than invent "last/random N of each file", because `-n` is worded as
a total row budget ("show N rows"), and a per-file rule would silently
multiply the output size by the file count for an unchanged flag. Proved
the choice matters, not just asserted it: with a=5, b=10, c=20 rows,
`tail -n 25` must return b's *last* 5 rows (ids 5-9) plus all of c, not all
of b — a naive "walk files backward, take whole files until N is met"
implementation would get b wrong at the boundary. `sample` mirrors this:
uniform draw across the virtual concatenation, mapping each global index
back to (file, local offset) and grouping consecutive same-file indices
into ranges to avoid full scans, same trick the single-file code already
used for one file.

## 2026-09-02 — Decide the output format once, from the name the user typed

Two features that each parse a filename collided. `sql -o DEST` resolves the
format from `DEST`'s extension, then hands the writer the *staging* path from
`output_guard`, and that writer re-sniffed the extension of the path it was
given. The staging name is built from `resolve_symlinks(DEST)` — the symlink
*target*'s name — so `-o link.parquet` where `link.parquet -> target.csv`
staged as `....csv`, the second sniff won, and `pq sql` wrote a CSV file under
a `.parquet` name with exit 0 and "Wrote 2 rows". Confirmed by magic bytes:
`69 64 2c 6e` ("id,n") where a control run produced `50 41 52 31` ("PAR1").
Extensionless and dangling targets produced JSONL and CSV the same way.

The tempting fix — make the staging name mimic the *destination's* extension
rather than the target's — is a patch on a symptom: it leaves two independent
derivations of the same fact, and they will drift again the next time either
side learns a new rule. So the format is decided exactly once, from the string
the user typed, and passed down: `write_batches_as(path, batches, format)`
obeys a format it is given, and `write_batches_to_file(path, ...)` (the
sniffing entry point, still used by `cat -O`/`jq -o`, which pass the real
destination) is documented as safe only for a path the user named.
`staging_path`'s doc comment used to *justify* preserving the extension as "so
callers which sniff the format still see what the user asked for"; that claim
was false, and the correction now lives there.

## 2026-09-02 — Union headers align records by name; Arrow batches are positional

The union-header CSV fix deduped field names through a `HashSet` and then
resolved each header entry back to data with `Schema::index_of(name)` (stdout)
or a JSON map keyed by name (the file paths). Neither can represent two
columns with the same name — which Parquet permits and Arrow batches express
positionally. A file with two `id` columns therefore lost one in every CSV
path, silently, exit 0, and the paths did not even lose the same one:
`cat -f csv` emitted `id / 1 / 2`, `export -o` emitted `id / 10 / 20`, where
`-f table` correctly showed `id,id / 1,10 / 2,20`. A fix whose stated purpose
was to stop CSV dropping data introduced a new way for CSV to drop data.

The tension is real and worth writing down: the union header exists to align
*heterogeneous records* by name, while an Arrow batch is positional. The
resolution is to make column identity `(name, occurrence-within-schema)` —
positional inside a schema, name-aligned across schemas — so the union still
works across files with different columns while duplicates each keep their
own slot and the header reads `id,id`, matching the table renderer. Name-keyed
lookup survives only on the jq/values path, where the input genuinely *is* a
map of names to values and duplicate keys cannot exist.

Four hand-rolled batch-to-CSV implementations became one, which is the more
durable half of the fix: they had already drifted into disagreeing about which
column to keep, and they rendered cells differently besides (Arrow's formatter
on stdout, JSON stringification in files). The shared implementation uses
Arrow's `ArrayFormatter`, the same one `-f table` uses, so CSV and table now
agree cell for cell.

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

## 2026-09-02 — TUI panic hook: chain, don't uninstall; verified with a real panic under a pty

`pq view`'s terminal setup/teardown in `commands/view.rs` had no panic hook,
so any panic inside `App::run` unwound straight past the "leave alt screen /
disable raw mode / show cursor" cleanup, leaving the user's shell raw and
blind until `reset`. Fix: `std::panic::take_hook()` the existing hook,
install a new one that restores the terminal and then calls the old hook
(so the panic message/backtrace still reaches the user, just on a sane
terminal), and factor the restore into one `restore_terminal()` fn called
from both the hook and the normal Ok/Err return path.

Deliberately did **not** try to uninstall the hook and restore the previous
one after `app.run()` returns normally — `pq view` is a single subcommand
that returns straight to `main` and exits, so there's no later "real" TUI
session for a lingering hook to interfere with, and restoring a `Box<dyn
Fn>` you've already moved into a closure needs an `Arc` and a bit of
plumbing that buys nothing here. Would need revisiting if `pq` ever grows a
long-lived mode that opens/closes the TUI more than once per process.

Verified this wasn't a paper fix: found a real, independent panic to drive
it with (`schema_tree.rs`'s slice-index bug — see below) rather than
injecting a fake one. Built a throwaway worktree with *only* the view.rs
hook applied (schema_tree.rs left buggy), drove `pq view` on a genuine
zero-column Parquet file through `demos/driver.py` in a 3-row pty, pressed
Tab to hit the Schema pane, and diffed the captured raw bytes against the
same run on unpatched `main`. Unpatched: panic fires, `\x1b[?1049l` (leave
alternate screen) never appears in the output at all — the pty is left
inside the TUI's frozen frame. Patched: the same panic fires, but
`\x1b[?1049l` appears in the output *before* the "panicked at" text, i.e.
the terminal is restored first and the panic message prints onto a normal
screen. That ordering is the actual thing users need; a passing test that
never made a real terminal look broken would not have proven it.

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
