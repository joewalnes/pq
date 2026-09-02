# Changelog

## 2026-09-02

- **Breaking:** `pq cat`/`jq`/`grep`/`sql`/`export` (any JSON/JSONL output) now render `Decimal128` values as a JSON **string** with exact digits, not a JSON number. The old arm went through `v as f64 / 10^scale`, silently losing precision beyond `2^53` (`decimal128(38,2)` holding `12345678901234567.89` printed `1.2345678901234568e+16`). `Decimal256` was outright wrong, not just lossy: it appended `scale` as if it were the fractional digits after trimming the mantissa's trailing zeros, so `123.45` printed `"12345.2"` and `1.23`/`123.00` both printed `"123.2"` — two different values collapsing to one output, unrecoverable from the JSON alone. Both widths now render the unscaled integer's digit string directly (no float involved), exact at any precision, and identical in output type for a given value regardless of width. `-f csv`/`-f table` already went through arrow's `ArrayFormatter` and were already correct; unaffected

- Close a hole in the release `preflight` version check: `grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+$'` accepted `v01.2.3` (npm and PyPI both silently rewrite the leading zero, splitting one release across differing version strings) and `v999999999999999999999.0.0` (accepted here, then rejected by npm's own "Invalid version" *after* `release` had already cut an immutable GitHub release — the exact burned-version scenario preflight exists to prevent). Also replaced grep's per-line `^...$` anchoring, which a multi-line input could satisfy one line at a time, with bash `[[ =~ ]]` matched against the whole string. New check: no leading zeros, 9 digits max per component; rejection message explains why and how to retag

- Fix errors printing the same sentence twice, e.g. `Error: Failed to read
  parquet file 'x.parquet': EOF: file size of 0 is less than footer: EOF:
  file size of 0 is less than footer`. `pq-core`'s error type interpolated
  its own cause into its message *and* implemented `source()`, so `anyhow`'s
  `{:#}` (used for every top-level error) printed the cause a second time.
  Affects every command whose error carries a filesystem, Parquet, Arrow, or
  JSON cause (missing/corrupt files, malformed JSON, IO failures); context
  like the filename and the underlying cause is unchanged, just no longer
  duplicated

## 2026-09-02

- Fix `pq sql` (and `pq cat --where`) silently dropping duplicate-named columns. A file with two `id` columns now queries as `id` and `id_1`, with the rename announced on stderr; previously one column vanished and `SELECT id` returned the *other* one's data, exit 0, no warning. Duplicate-named files are read into memory rather than streamed, so they lose predicate/projection pushdown; files with unique column names are unaffected

- `pq sql` and `pq cat --where` now **refuse** a directory table containing a file with duplicate column names, instead of answering it wrongly. `SELECT * FROM 'dir.parquet'` used to return one column carrying the *second* column's data under the *first* column's name — exit 0, no note — while the same bytes queried as `dir.parquet/part0.parquet` answered correctly. There is no correct merge to repair to (a directory's files are merged by column name), and the only repair pq has would materialize the whole directory, so it errors with the offending file named. pyarrow refuses the same input. Directories of unique-named files, of differing-but-mergeable schemas, and Hive-partitioned directories are unaffected
- A duplicate-named `.parquet` path that appears in a query only as a string literal (`WHERE src = '.../x.parquet'`) is no longer read: the file is now materialized on first scan, not at registration. Measured on a 132 MB file, peak memory dropped from 168 MB to 6.9 MB and the misleading "renamed column" note about a file the query never selects from is gone
- Correction to the entry below: that fix covered **single-file** tables only. Directory tables were deliberately exempted from the duplicate check and stayed silently wrong until the two entries above

- Fix `pq sql` (and `pq cat --where`) silently dropping duplicate-named columns *for single-file tables* (directory tables stayed broken — see above). A file with two `id` columns now queries as `id` and `id_1`, with the rename announced on stderr; previously one column vanished and `SELECT id` returned the *other* one's data, exit 0, no warning. A duplicate-named file that is actually scanned is read into memory rather than streamed, so it loses predicate/projection pushdown; files with unique column names are unaffected

- **Breaking:** removed `-q`/`--quiet` and `-v`/`--verbose` — both were parsed and stored but never read by any command; behaviorally confirmed identical output with and without them. Inventing real quiet/verbose semantics is a product decision that wasn't made, so this deletes rather than fakes it. A script passing either flag now gets `error: unexpected argument ... found` and exit code 2 instead of the flag being silently ignored
- `--color auto|always|never` is now real: table headers render bold+cyan when color is on. `auto` (the default) follows the no-color.org convention — any non-empty `NO_COLOR` disables color, otherwise it follows whether stdout is a real terminal — and `always`/`never` are explicit overrides that win regardless. Previously accepted and silently ignored, while `pq capabilities` claimed `"respects_no_color": true` with zero color output anywhere to respect
- Fixed `pq --version` and `pq capabilities`'s `"version"` field disagreeing: `--version` read the real build version (`PQ_VERSION`, tag-derived in release builds) while `capabilities` read `CARGO_PKG_VERSION`, permanently `0.1.0` from Cargo.toml. Both now read the same constant; a new test pins them together
- Regenerated `pq capabilities`'s `commands`/`global_options` from the actual clap command tree instead of a 197-line hand-maintained duplicate, which had already silently drifted: it was missing `stats --sample-size` and `cat --output` entirely, and wrongly claimed `count`/`merge`'s file arguments and `slice --offset`'s default were something other than what clap actually parses. A small hand-written table now covers only what clap can't derive (semantic value types like "path" or "regex", which output formats a command supports), guarded by an assertion that fails if that table or its exclusion list ever names something that no longer exists in the real CLI
- `pq capabilities` arg names for options now always use the long flag form (e.g. `--lines` not `-n`) for consistency — the old hand list picked short or long per-arg with no discernible rule
- Fail a release before it starts if the `NPM_TOKEN` secret is missing, so the irreversible half (an immutable GitHub release, a burned version number) cannot run when the npm publish is already doomed. `NPM_TOKEN` does not currently exist in the repo, so today a `v0.1.0` tag would stop here
- Pin the floating refs the release workflow trusted: `dtolnay/rust-toolchain@stable` and `pypa/gh-action-pypi-publish@release/v1` (both branches) to exact commits, and `cargo install cross --git` to an exact `--rev` with `--locked`, in both `release.yml` and the `Makefile`
- Ship `LICENSE` and `THIRD-PARTY-LICENSES` as release assets, so the `curl`-a-binary install gets the same attribution npm packages and wheels already carry
- Publish `SHA256SUMS` as a release asset, and document verifying a downloaded binary against it in README.md
- Releases are now created for the git tag itself. Removes the `gh release delete latest` / `git push origin :refs/tags/latest` / `gh release create latest` dance that destroyed the project's only release; GitHub's own `/releases/latest/download/` redirect provides the pointer, so nothing needs deleting
- Release version now comes from the git tag, derived once in a new `preflight` job and consumed by every other job. Deletes the `0.1.$(date +%Y%m%d%H%M)` scheme, which the two publish jobs computed independently and could disagree on across a minute boundary
- Record in LESSONS.md that an adversarial pass over four merged branches found three had each injected a fresh defect, and that a destructive step suppressed with `|| true` will eventually destroy something
- Record the credentials finding: no repo secrets exist, so `NPM_TOKEN` is absent and tagging `v0.1.0` would create a release then fail to publish
- Record in DIARY.md why 14 successful release runs left zero releases: the `latest` delete succeeded and the recreate failed, which is the evidence for dropping the mutable tag in favour of GitHub's `/releases/latest/` redirect
- Record the human's release decisions in ASKS.md: three channels, tag-driven semver, no `latest` tag
- Log two unfixed bugs found by adversarial review: `pq sql` silently drops duplicate-named columns via DataFusion planning, and `pq-core` error Display doubles its own source chain
- Log that npm/PyPI publishing has never succeeded and must be verified before the first `v0.1.0` tag
- Correct the record: the `latest` tag name was not shown to be permanently unusable, only refused once
## 2026-09-02 (5)

- Fix `pq layout`: row group ranges never accumulated an offset (every
  group reported "rows 0–N"), and a column chunk's byte range started at
  its data page even when a dictionary page preceded it, understating the
  chunk. Verified against pyarrow (independent instrument) on a 3-row-group,
  dictionary-encoded fixture — pq's row starts were 0/0/0 (should be
  0/100/200) and its first column's byte start was 432 (should be 4, the
  dictionary page offset). Both now match pyarrow exactly.
- Fix `tail`/`sample`/`count`/`merge` silently mishandling multiple files:
  `tail` used only the last file, `sample` only the first, and
  `count`/`merge` never expanded globs (`resolve_files` was skipped).
  `tail` now returns the last N rows of the file concatenation (matching
  `head`'s existing multi-file treatment); `sample` draws N rows uniformly
  across all files; `count`/`merge` now expand globs like every other
  multi-file command. A glob matching zero files and a literal missing
  path now produce distinct errors.

- Fix CSV silently dropping duplicate-named columns — a regression from the
  union-header change, which deduped field names through a `HashSet` and
  resolved them back by name. A Parquet file with two `id` columns lost one
  in every CSV path, and the paths disagreed about which: `cat -f csv` kept
  the first column's values, `export -o`/`cat --output`/`sql -o` the
  second's, both under a single `id` header. Columns are now identified by
  `(name, occurrence)` and resolved positionally, so the header is `id,id`
  and both columns' data survives. All four batch-to-CSV implementations
  were replaced by one shared implementation, which also makes `-f csv`
  agree cell-for-cell with `-f table`.
- Fix `pq sql -o <file>` writing the wrong format when the destination is a
  symlink whose target has a different extension: `-o link.parquet` where
  `link.parquet -> target.csv` wrote CSV under a `.parquet` name, exit 0.
  The format was being resolved twice from two different strings — once
  from the destination the user named, then again by sniffing the staging
  path (whose name comes from the resolved symlink target). It is now
  resolved once and passed down.
- Fix `export`/`sql -o` printing `note: -f/--format table overrides ...`
  and then failing — the note announced an override that never took effect.
  A format that can't be written to a file is now rejected before any note.

- Fix `stats --describe`: the panic-avoidance fix for cross-file schema
  mismatches over-corrected by comparing whole `arrow::datatypes::Field`s,
  whose equality also covers nullability and field metadata —
  `arrow::compute::concat` (the operation being guarded) only cares about
  `data_type()`. Two files that differed only in nullability (e.g. one
  written `NOT NULL`, one not) were rejected even though `concat` handles
  them fine. Now compares column count and `DataType` only. Also: when a
  real mismatch remains, the error used to render both files' column lists
  through the friendly `format_dtype`, which can collapse genuinely
  different types onto the same string (every `Timestamp` unit prints as
  "timestamp", every same-arity `Struct` prints as "struct<N fields>") and
  could show two IDENTICAL-looking lists for a real, unmergeable mismatch.
  The error now renders the exact `DataType` instead.
- Narrow README.md's `-o`/`-O` format-detection claim: it stated `-f`
  overrides the extension-based default on `sql`/`jq`/`export -o` and
  `cat -O` alike. Confirmed true only for `sql`/`export`; `jq -o`/`cat -O`
  silently ignore `-f` and always use the extension (`pq jq f.parquet '.'
  -o out.csv -f json` writes CSV). Chose to fix the README rather than the
  behavior: closing the gap needs `write_output.rs`, which another agent
  owns right now. Logged as a TODO instead of leaving it undiscoverable.
  See DIARY.md.
- Strengthen `remote_tests.rs::test_http_truncated_response_produces_clear_error`,
  found vacuous by mutation (a `pq` shim that fails `cat` without touching
  the network passed its old `.failure()` + no-"panicked" checks). Now
  asserts the test server actually served a short `206` body and that the
  error text names an incomplete transfer, not just any failure.

## 2026-09-02 (4)

- Fix `export`/`sql -o`: an explicit `-f`/`--format` was silently ignored
  whenever output went to a file (extension always won, and an
  unrecognized extension silently fell back to JSONL — confirmed:
  `export -o a.parquet -f csv` wrote JSONL into a file named `.parquet`,
  exit 0). Now: extension governs by default, an explicit `-f` overrides
  it with a stderr note, and an undetermined format is a hard error
  instead of a silent guess. Also fixed `export -f csv` to stdout, which
  had no CSV branch at all and silently emitted JSONL.
- Fix `TODO.md`: `describe` doesn't exist as a standalone command (it's
  `stats --describe`); the `union` command item was already satisfied by
  `merge --schema-mode union`, not a separate TODO; one stale `convert`
  reference updated to `import`
- Correct README.md: `-O` was documented as the output-*format* flag (real
  flag is `-f`; `-O` on `cat` means `--output <FILE>`), `pq convert` was
  documented instead of the real `pq import`, `pq select` was documented
  as filtering rows (it only projects columns), `grep`/`split`/`validate`/
  `import`/`export` were missing from the feature list entirely, and the
  flagship example used a table-rendering style (`┌┬┐`/`│`, no row
  separators, comma-formatted counts, a fabricated `…` truncation row, a
  `Created by: pq 0.1.0` line the tool never emits) that hasn't matched
  the real renderer's output (`╭╮╰╯`/`┆`, per-row separators, no comma
  formatting) since it was written
- Correct `docs/src/example-data.md` and `docs/src/index.md`: their
  `pq schema` tree output was fabricated (wrong Unicode box-drawing glyph
  for the last branch, wrong/missing `(nullable)` markers, invented type
  names like `date32`/`decimal128`/`binary` instead of the real
  `date`/`decimal`/`fixed_binary`); replaced with real, verified output.
  `docs/src/faq.md` updated to describe the new `-f`-vs-extension
  semantics on `export`/`sql -o`
- Correct the README's "tutorials double as tests" claim: only
  `tests/golden/tutorials/` is executed by the test harness. The published
  `docs/src/tutorials/` copies are hand-formatted and have drifted
  (~58-172 changed lines per file across all five); tracked in TODO.md
  rather than silently migrated

## 2026-09-02 (3)

- Give HTTP remote-file access real, running test coverage: 10 `test_http_*`
  cases in `remote_tests.rs` now run against an in-process Range-supporting
  HTTP server (no Docker needed) and are un-`#[ignore]`d, plus 3 new tests
  for 404 / Range-disabled / truncated-response error paths. S3 tests are
  untouched and remain `#[ignore]`d pending SeaweedFS/Docker.

- Fix `pq stats --describe` panicking (index out of bounds) when given
  multiple files whose schemas differ; now fails with an error naming both
  mismatched files instead
- Fix `pq view`'s TUI leaving the terminal in raw mode / alternate screen /
  mouse-capture-on if a panic occurs mid-session; install a panic hook that
  restores the terminal before the panic message prints
- Fix a slice-index panic in the TUI's schema tree pane when rendered with
  zero schema fields and a collapsed (zero-height) pane
- Investigated (not changed, unreachable via any real input): `.unwrap()`s
  on `ArrayFormatter::try_new` in `output/table.rs` and `output/csv.rs`, and
  on `arrow::compute::take` in `commands/split.rs` — all three can only
  fail for `DataType::ListView`/`LargeListView`, which neither Parquet nor
  this codebase's DataFusion path can produce

## 2026-09-02 (2)

- Fix `make licenses` leaving generated `THIRD-PARTY-LICENSES` files
  untracked and ungitignored (blocked the shared-checkout merge gate);
  gitignore them instead of committing
- Fix `make licenses`' cargo-about install check using `command -v`, which
  misses `~/.cargo/bin` when it's off `$PATH`; probe with `cargo about
  --version` instead
- Fix `make licenses` writing its NOTICE-appendix scratch file to a fixed
  `/tmp` path (collision risk with concurrent agents); use `mktemp` with
  cleanup on both success and failure
- Wire `make licenses` into `.github/workflows/release.yml` (new
  `licenses` job feeding `publish-npm`/`publish-pypi`), pinning cargo-about
  to an exact version — closes the gap where the first tagged release
  would have failed at the wheel-build step

## 2026-09-02

- Fix PyPI wheel: removed a `[console_scripts]` entry point that made pip
  clobber the real `pq` binary with a broken Python shim, and fixed the
  binary's zip permission bits so pip actually marks it executable
- Add `pypi/build_wheels.py --self-test` regression guard for both of the above
- Add `about.toml`/`about.hbs` (cargo-about config) and `make licenses` to
  generate `THIRD-PARTY-LICENSES` for the workspace's dependencies
- Ship `LICENSE` and `THIRD-PARTY-LICENSES` in the npm packages and PyPI wheel

## 2026-09-01

- Record three lessons in LESSONS.md: a harness must assert the identity of its subject rather than resolving it by name through an ambient PATH, and an intermittent gate must be explained rather than retried, and a fix that changes the mechanism must be attacked on what the new mechanism requires
- Fix all 19 remote-file tests (`crates/pq-cli/tests/remote_tests.rs`): they used a non-existent `-O <format>` flag (real flag is `-f`); on `cat` specifically `-O` means `--output <file>`, so two tests silently wrote a stray file named `jsonl`. Also fixed a latent bug where the tests assumed `nested_data.parquet` had already been generated into `tests/fixtures/` by an unrelated test binary — `make test-integration` alone never produced it — and strengthened two assertions that only checked a nested value's top-level key was present (would still pass if nested struct/list fields were silently dropped). Kept `#[ignore]`d; see DIARY.md for why
- Move `cli_tests.rs`'s `nested_data.parquet` fixture generation out of `tests/fixtures/` into a per-process `TempDir`, so it no longer shows up as an untracked file in `git status` or races with other test binaries / worktrees writing the same path. `test_data.parquet` stays tracked in git and is read directly, never regenerated

- Fix CSV column-shift/data-loss bug: all four CSV emission paths (`cat --output`, `export`, `cat --jq -O`, and `cat -f csv` to stdout) froze the header from row/batch 0 with no per-row key lookup, so combining files with different schemas silently shifted values into the wrong-named column or dropped them entirely. Header is now the union of every input's columns, and each row is keyed by column name against it (missing key -> empty field, never dropped). Replaced three hand-rolled CSV escapers (which quoted `,`/`"`/`\n` but not a lone `\r`, letting a bare CR split one CSV record into two) with the already-declared-but-unused `csv` crate. Guarded with `crates/pq-cli/tests/csv_correctness_tests.rs` (10 tests, each proven to fail against the unfixed code)
- Fix flaky/stale golden expectation in the SQL tutorial's JOIN example: `ORDER BY num_orders DESC` left a 3-way tie (Charlie/Diana/Eve, all num_orders=1) with unspecified order, so the expected row order was one DataFusion version bump from breaking. Added `, u.name` as a tie-break so the result is deterministic by construction, and hand-updated the expectation to the true output (Charlie, Diana, Eve alphabetically) in both `tests/golden/tutorials/sql-queries.md:160` and its diverged hand-copy `docs/src/tutorials/sql-queries.md` (same query, same stale tie order, not covered by the golden runner)
- Fix tagline drift: `pq --help` had an ASCII hyphen, `pq capabilities` still had an em-dash, and the golden expectation expected the em-dash. Picked the hyphen (matches README.md/docs/ house style and pypi/build_wheels.py; see DIARY.md), extracted both to a shared `cli::TAGLINE` constant so it can't drift again, updated `tests/golden/tests/help-output.md`, and added `test_tagline_matches_between_help_and_capabilities` in `cli_tests.rs` (proven to fail pre-fix, pass post-fix)
- Fix `tests/golden/run.py` silently testing the wrong binary: `find_pq_binary()` never checked that a `PQ=...` override actually existed before returning it, and console blocks invoke the bare name `pq` via a PATH with only the binary's directory prepended — so a bad/stale `PQ` silently fell through to whatever `pq` was already on the ambient PATH (a stale `brew`-installed build on this machine) instead of failing. Now exits non-zero immediately if `PQ` doesn't resolve to an existing, executable file, and again if prepending its directory to PATH doesn't make bare `pq` resolve to that exact binary. Proven: `PQ=/nonexistent/pq python3 tests/golden/run.py tests/golden/tests/help-output.md` used to print "4 passed, 1 failed" (measuring the homebrew binary); now exits 1 with a clear error before running any commands
- Fix all `cargo clippy --workspace --all-targets -- -D warnings` findings: collapsible-match in `pq-tui/src/app.rs` and `pq-cli/src/commands/grep.rs`, redundant closures in `write_output.rs`, `map_or` -> `is_some_and` in `main.rs`, needless borrow in `cli_tests.rs`, and a justified `#[allow(clippy::too_many_arguments)]` on `cat::run` (8 CLI-flag passthrough params, no other command in the crate has this shape, a struct wrapper adds indirection without adding clarity)
- Reformat `pq-cli` sources with `cargo fmt` to green the `cargo fmt --all -- --check` gate (write_output.rs, main.rs); whitespace only, no behavior change
- Release workflow now triggers on `v*` tags or manual dispatch instead of every push to main
- Gate releases and npm/PyPI publishing behind CI passing
- Scope release workflow permissions down to read by default, write only where needed
- Add CI workflow: fmt, clippy, cargo test, and golden tests on PRs and pushes to main

- Fix data loss: lists nested inside objects (`{"o":{"l":[1,2]}}`) no longer import as NULL — they were dropped entirely, at any element type
- Fix data loss: JSON columns holding mixed types no longer import as NULL. A column widened to string now keeps numbers and booleans as their text (`42`, `true`) and objects/arrays as compact JSON, at every nesting level — previously every non-string value in such a column was dropped, silently and with exit 0
- Fix data loss: `-o` pointing at an input file no longer destroys it. `merge`, `select`, `slice`, `export` and CSV `import` now stage their output in a sibling temp file and rename it into place, so in-place transforms work and a failed write leaves the destination untouched
- Add project process docs: CLAUDE.md, engineering diary, changelog
- Ignore .wrangler/ directory

## 2026-04-19

- Add npm and PyPI package publishing to release workflow

## 2026-04-12

- Fix TUI background rendering glitch on some terminals
- Add example data page to docs site

## 2026-04-11

- Host public example parquet files at data.pqtool.dev (Cloudflare R2)
- Add Plausible analytics to docs site
- Add automated binary releases on push to main
- Deploy docs site via GitHub Pages
