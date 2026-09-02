//! Runs the `pq sql` examples that `--help` actually advertises, and asserts
//! they succeed.
//!
//! Why this exists: `tests/golden/tests/help-output.md` pins the *text* of
//! `--help` byte-for-byte, but never executes anything inside it. Two defects
//! shipped inside that pinned text and the golden suite stayed green through
//! both:
//!
//! 1. Every `pq sql` example used a bareword path like `'data.parquet'`.
//!    DataFusion parses the dot in an unquoted-looking bareword as a
//!    schema-qualifier (`schema.table`), so every one of those examples
//!    failed: `Error: DataFusion error: Error during planning: failed to
//!    resolve schema: data` (reproduced against the pre-fix binary; see
//!    DIARY.md). The fix is `./data.parquet`.
//! 2. `sql --help` claimed "Glob patterns (e.g., 'logs/*.parquet') are
//!    supported", which was false in every form before this same change:
//!    `register_files_from_query` (`crates/pq-query/src/sql.rs`) only
//!    registered a quoted path if `Path::canonicalize().exists()`, and a glob
//!    never exists as a literal path, so the table was never registered:
//!    `Error: ... table 'datafusion.public.logs/*.parquet' not found`.
//!
//! Both were caught by hand, not by any gate — `help-output.md` cannot catch
//! either, because pinning text is not running text. This test closes that
//! *class* rather than just these two instances: it pulls the example
//! commands out of the binary's own `--help`/`sql --help` output (so it tests
//! whatever the examples currently say, not a second hand-typed copy that
//! could itself drift from the real `long_about`) and actually runs each one
//! to completion against a real fixture, asserting success.
//!
//! Scope, stated plainly: this only extracts and runs `pq sql "..."`
//! examples — the surface `long_about`/`after_help` mention in `cli.rs`
//! that Findings 1 and 2 are about, and the one this crate is scoped to
//! (`crates/pq-cli/src/cli.rs`, `crates/pq-query/src/sql.rs`,
//! `crates/pq-cli/src/commands/sql.rs`). Other subcommands (`jq`, `grep`,
//! `select`, ...) also advertise examples in their own `long_about`s and are
//! NOT covered here — extending this to every subcommand needs a fixture
//! matching each of *their* example columns too (nested structs and arrays
//! for `jq`, specific column names for `grep`/`select`), which is real,
//! separate work belonging to whoever owns those files. Logged in TODO.md.
//!
//! Fixture requirement, stated plainly: this test only works because the
//! fixtures below are named and shaped to match what the *current* `sql`
//! examples reference (`data.parquet` with an `id`/`city` column,
//! `a.parquet`/`b.parquet` with `id`/`name` for the JOIN example, a `logs/`
//! directory of two files with a `level` column for the glob example). If a
//! future edit to `cli.rs` adds a `pq sql` example referencing a new file or
//! column, this test needs a matching fixture added alongside it, or it will
//! fail for "no such file" rather than for the bug class it exists to catch.
//! That coupling is the acknowledged cost of "run it for real" over "pin the
//! text": a hand-maintained fixture set instead of a hand-maintained command
//! list. The alternative — inventing a fixture generically from whatever
//! filenames appear in the query text — would have to guess column names and
//! types from the query alone, which is guessing at what a passing test
//! means; guessing was rejected in favor of a fixture that is legible by
//! inspection.
//!
//! Identity guard: `pq()` below resolves to the binary `cargo test` just
//! built (`assert_cmd::cargo_bin_cmd!`), never a `PATH`-resolved `pq` —
//! `LESSONS.md` names exactly this substitution as a way for a check to pass
//! while never touching the code under test.

use arrow::array::{Int64Array, StringArray};
use arrow::datatypes::{DataType, Field, Schema};
use arrow::record_batch::RecordBatch;
use assert_cmd::Command;
use regex::Regex;
use std::sync::Arc;
use tempfile::TempDir;

fn pq() -> Command {
    assert_cmd::cargo_bin_cmd!("pq")
}

fn write_parquet(
    path: &std::path::Path,
    fields: Vec<Field>,
    columns: Vec<Arc<dyn arrow::array::Array>>,
) {
    let schema = Arc::new(Schema::new(fields));
    let batch = RecordBatch::try_new(schema, columns).expect("building fixture RecordBatch");
    pq_core::writer::write_batches(
        path,
        std::slice::from_ref(&batch),
        &pq_core::writer::WriteOptions::default(),
    )
    .expect("writing fixture parquet file");
}

/// Extracts every `pq sql "<query>"` example line out of rendered help text,
/// returning the unquoted query string for each. Deliberately reads the
/// *rendered* text rather than re-typing the query strings a second time in
/// this file: a hand-duplicated copy could itself drift from `cli.rs` and
/// silently stop testing the real examples, which is exactly the failure
/// mode this test exists to close.
fn extract_sql_examples(help_text: &str) -> Vec<String> {
    let re = Regex::new(r#"^\s*pq sql "(.+)"\s*$"#).expect("valid regex");
    help_text
        .lines()
        .filter_map(|line| {
            re.captures(line)
                .map(|caps| caps.get(1).unwrap().as_str().to_string())
        })
        .collect()
}

/// Build the fixtures every currently-advertised `pq sql` example needs, laid
/// out under `dir` with the exact relative names the examples reference.
/// Relative and exact on purpose: the bug this test guards (bareword paths
/// parsed as `schema.table`) is specific to a slash-less relative name run
/// from the file's own directory — an absolute path or one already
/// containing a `/` sidesteps the bug entirely and would make the test blind
/// to a regression that dropped the `./` prefix back out of the examples.
fn build_sql_example_fixtures(dir: &std::path::Path) {
    // `./data.parquet` — needs `id` (JOIN-shaped queries elsewhere reuse the
    // name, harmless here) and `city` (GROUP BY city).
    write_parquet(
        &dir.join("data.parquet"),
        vec![
            Field::new("id", DataType::Int64, false),
            Field::new("city", DataType::Utf8, false),
        ],
        vec![
            Arc::new(Int64Array::from(vec![1, 2, 3])),
            Arc::new(StringArray::from(vec!["nyc", "sf", "nyc"])),
        ],
    );

    // `./a.parquet` JOIN `./b.parquet` ON a.id = b.id
    write_parquet(
        &dir.join("a.parquet"),
        vec![Field::new("id", DataType::Int64, false)],
        vec![Arc::new(Int64Array::from(vec![1, 2]))],
    );
    write_parquet(
        &dir.join("b.parquet"),
        vec![
            Field::new("id", DataType::Int64, false),
            Field::new("name", DataType::Utf8, false),
        ],
        vec![
            Arc::new(Int64Array::from(vec![1, 2])),
            Arc::new(StringArray::from(vec!["alice", "bob"])),
        ],
    );

    // `./logs/*.parquet` WHERE level = 'ERROR'
    let logs = dir.join("logs");
    std::fs::create_dir(&logs).expect("creating logs/ fixture dir");
    write_parquet(
        &logs.join("one.parquet"),
        vec![Field::new("level", DataType::Utf8, false)],
        vec![Arc::new(StringArray::from(vec!["ERROR", "INFO"]))],
    );
    write_parquet(
        &logs.join("two.parquet"),
        vec![Field::new("level", DataType::Utf8, false)],
        vec![Arc::new(StringArray::from(vec!["ERROR"]))],
    );
}

#[test]
fn sql_examples_in_top_level_help_actually_run() {
    let help = pq().arg("--help").output().expect("running pq --help");
    assert!(help.status.success(), "pq --help itself must succeed");
    let help_text = String::from_utf8(help.stdout).expect("utf8 help output");

    let examples = extract_sql_examples(&help_text);
    assert!(
        !examples.is_empty(),
        "extracted zero `pq sql \"...\"` examples from `pq --help` — the \
         regex may no longer match the current after_help text (a change to \
         the wording, not a real absence of examples). A test that finds \
         nothing here must not silently pass: fix the regex or the extractor \
         rather than let this go green on a false zero.",
    );

    let dir = TempDir::new().expect("tempdir");
    build_sql_example_fixtures(dir.path());

    for query in examples {
        pq().current_dir(dir.path())
            .args(["sql", &query])
            .assert()
            .success();
    }
}

#[test]
fn sql_examples_in_sql_subcommand_help_actually_run() {
    let help = pq()
        .args(["sql", "--help"])
        .output()
        .expect("running pq sql --help");
    assert!(help.status.success(), "pq sql --help itself must succeed");
    let help_text = String::from_utf8(help.stdout).expect("utf8 help output");

    let examples = extract_sql_examples(&help_text);
    assert!(
        examples.len() >= 4,
        "expected at least the 4 examples `sql --help`'s long_about documents \
         (LIMIT, GROUP BY, JOIN, glob); got {}: {examples:?}. Fewer than \
         expected means either an example was removed (update this count) or \
         the extraction regex stopped matching (fix it) — either way, a \
         silent drop here would mean this guard stops testing what it claims \
         to.",
        examples.len(),
    );

    let dir = TempDir::new().expect("tempdir");
    build_sql_example_fixtures(dir.path());

    for query in examples {
        pq().current_dir(dir.path())
            .args(["sql", &query])
            .assert()
            .success();
    }
}

#[test]
fn extractor_ignores_non_sql_example_lines() {
    // A harness that can't tell a real example from prose is worse than
    // useless — it would either miss real examples or try to execute
    // narrative text as a command. Prove the regex is selective.
    let text = "\
Examples:
  pq sql \"SELECT * FROM './data.parquet' LIMIT 10\"
  pq jq data.parquet '.name'
  pq info data.parquet
Some unrelated line mentioning pq sql in prose, no quotes.
";
    let examples = extract_sql_examples(text);
    assert_eq!(
        examples,
        vec!["SELECT * FROM './data.parquet' LIMIT 10".to_string()]
    );
}
