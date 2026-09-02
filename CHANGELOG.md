# Changelog

## 2026-09-01

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
