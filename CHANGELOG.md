# Changelog

## 2026-09-01

- Release workflow now triggers on `v*` tags or manual dispatch instead of every push to main
- Gate releases and npm/PyPI publishing behind CI passing
- Scope release workflow permissions down to read by default, write only where needed
- Add CI workflow: fmt, clippy, cargo test, and golden tests on PRs and pushes to main
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
