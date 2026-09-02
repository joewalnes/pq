# Changelog

## 2026-09-01

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
