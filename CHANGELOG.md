# Changelog

## 2026-09-02

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
