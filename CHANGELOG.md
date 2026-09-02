# Changelog

## 2026-09-01

- Fix all `cargo clippy --workspace --all-targets -- -D warnings` findings: collapsible-match in `pq-tui/src/app.rs` and `pq-cli/src/commands/grep.rs`, redundant closures in `write_output.rs`, `map_or` -> `is_some_and` in `main.rs`, needless borrow in `cli_tests.rs`, and a justified `#[allow(clippy::too_many_arguments)]` on `cat::run` (8 CLI-flag passthrough params, no other command in the crate has this shape, a struct wrapper adds indirection without adding clarity)
- Reformat `pq-cli` sources with `cargo fmt` to green the `cargo fmt --all -- --check` gate (write_output.rs, main.rs); whitespace only, no behavior change
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
