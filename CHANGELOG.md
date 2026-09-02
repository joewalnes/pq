# Changelog

## 2026-09-02

- Fix PyPI wheel: removed a `[console_scripts]` entry point that made pip
  clobber the real `pq` binary with a broken Python shim, and fixed the
  binary's zip permission bits so pip actually marks it executable
- Add `pypi/build_wheels.py --self-test` regression guard for both of the above
- Add `about.toml`/`about.hbs` (cargo-about config) and `make licenses` to
  generate `THIRD-PARTY-LICENSES` for the workspace's dependencies
- Ship `LICENSE` and `THIRD-PARTY-LICENSES` in the npm packages and PyPI wheel

## 2026-09-01

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
