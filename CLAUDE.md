# CLAUDE.md

pq is a Parquet Swiss Army Knife — a Rust CLI/TUI to inspect, query, transform, and view Parquet files. Workspace crates live in `crates/` (`pq-cli`, `pq-core`, `pq-query`, `pq-transform`, `pq-tui`). Docs are generated (`make docs`) and published; example data is hosted at `data.pqtool.dev` (Cloudflare R2, see `make upload-examples`).

## Bug tracking

Bugs and tasks are tracked in `TODO.md`, grouped into sections (Tier 1, Tier 2, Bugs, Infrastructure). Entries are `- [ ]`/`- [x]` bullets with a short title and an em-dash description; completed entries keep a brief note of how they were resolved. Use `/todo` to add entries and `/bug-bash` to work through them, following the existing format.

## Engineering diary

Maintain `DIARY.md` — add an entry when making significant changes, architectural decisions, or non-obvious tradeoffs. Latest entries at top. Write in narrative form, not bullet dumps. Focus on *why* and *context*, not *what* (that's in the commits).

## Changelog

Update `CHANGELOG.md` with every commit. Format: grouped by date (newest first), one bullet per change with a short description. Keep it human-readable — no commit hashes, no authors.

## Pre-commit checks

Always run tests and linting before committing:

```bash
make test    # cargo test --workspace + golden tests (tests/golden/run.py)
make lint    # cargo clippy -D warnings + cargo fmt --check
```

Do not commit if tests fail or lint errors are present. Fix first.

## Commits

Break work into small atomic commits — one logical change per commit. Don't bundle unrelated changes. A bug fix, a new feature, and a refactor are three commits, not one.

## Test-first

Before implementing a feature or fix:

1. Write a test that captures the expected behavior
2. Run it — verify it **fails** (if it passes, the test isn't testing the right thing)
3. Implement until the test passes
4. Keep a healthy mix: fast unit tests for logic, plus golden tests (`tests/golden/`) and integration tests (`make test-integration` for remote/S3) to validate it works in context

Don't skip step 2 — a test that never failed never caught anything.

## Documentation

Update README.md and the docs (`docs/`) before committing if the change affects:

- Public API, CLI interface, or configuration
- Setup/installation steps
- Feature behavior visible to users

The CLI reference is generated from the binary (`make docs`), so flag/help changes flow through automatically — but tutorial pages and README examples must be updated by hand.

## Code quality

Run `/scorecard` periodically — after completing a feature, before major PRs, or when onboarding to assess health. Address critical findings before moving on.

## Completing requests

When the user gives multiple requests:

1. Queue them mentally but complete ONE fully before starting the next
2. "Complete" means: code written, built, tested, and verified working
3. If a request involves the TUI or docs site: actually run/render it and confirm it matches what was asked
4. Never mark something done until you've verified it works end-to-end
5. If you can't complete a request in one go, say so explicitly rather than half-doing it and moving on
6. If multiple requests conflict or depend on each other, state the dependency and ask which to prioritize

Anti-patterns to avoid:

- Starting several things, finishing none
- Saying "let me commit this and move on" when the current thing isn't verified
- Changing behavior without testing that the behavior changed
- Reacting to new user messages mid-task instead of finishing current work first
- Saying "let me ignore X for now" — either fix it or explicitly tell the user it's queued (e.g. add it to `TODO.md`)

## Evolving preferences

When the user expresses a coding preference, convention, or correction during a session, offer to encode it into this CLAUDE.md file so it persists across sessions. Examples: naming conventions, preferred libraries, architecture patterns, things to avoid.

## Mistake retrospectives

When you make a mistake (especially forgetting something the user asked for):

1. Acknowledge it directly
2. Identify the root cause — why did this happen? (e.g. no checklist, unclear convention, missing rule)
3. Suggest a concrete project change to prevent recurrence (add a rule to CLAUDE.md, add a pre-commit check, create a checklist in the relevant skill)

Don't just apologize — fix the system.
