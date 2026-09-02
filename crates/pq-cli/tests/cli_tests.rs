use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use tempfile::TempDir;

fn pq() -> Command {
    assert_cmd::cargo_bin_cmd!("pq")
}

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

/// Directory for fixtures generated at test time. A real `TempDir`, held in
/// a process-wide `OnceLock` so it is created at most once per test binary
/// and cleaned up automatically on process exit — never written into the
/// shared source tree, so parallel `cargo test` runs (and other worktrees
/// checked out from the same repo) can't race on it or leave it dirty.
fn generated_fixture_dir() -> &'static Path {
    static DIR: OnceLock<TempDir> = OnceLock::new();
    DIR.get_or_init(|| TempDir::new().expect("failed to create temp fixture dir"))
        .path()
}

/// `test_data.parquet` is committed to git (`tests/fixtures/test_data.parquet`)
/// specifically so the ~40 tests that use it don't each pay for a `pq import`
/// subprocess: it's a fixed, deterministic 100-row file that never needs to
/// change alongside test code. Tests only ever *read* it — nothing regenerates
/// it into the source tree. If it's missing (e.g. a corrupted checkout), fail
/// loudly with the exact command to restore it, instead of silently writing a
/// fresh copy back into `tests/fixtures/`.
fn fixture_path() -> String {
    let parquet = workspace_root().join("tests/fixtures/test_data.parquet");
    assert!(
        parquet.exists(),
        "tracked fixture missing: {}\n\
         Regenerate it with:\n\
         \x20 python3 tests/fixtures/gen_test_data.py\n\
         \x20 cargo run -- import tests/fixtures/test_data.jsonl -o tests/fixtures/test_data.parquet\n\
         then `git add` the result — it is meant to be committed, not generated per test run.",
        parquet.display()
    );
    parquet.to_str().unwrap().to_string()
}

fn ensure_fixture() {
    // test_data.parquet is tracked (see `fixture_path` above); this just
    // turns "missing" into a clear panic instead of an obscure downstream
    // failure in whichever test happened to run first.
    fixture_path();
}

#[test]
fn test_info() {
    ensure_fixture();
    pq().args(["info", &fixture_path(), "-f", "json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"num_rows\""));
}

#[test]
fn test_info_table() {
    ensure_fixture();
    pq().args(["info", &fixture_path(), "-f", "table"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Rows:"))
        .stdout(predicate::str::contains("100"));
}

#[test]
fn test_schema_tree() {
    ensure_fixture();
    pq().args(["schema", &fixture_path(), "-f", "table"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Schema (6 columns)"));
}

#[test]
fn test_schema_ddl() {
    ensure_fixture();
    pq().args(["schema", &fixture_path(), "--style", "ddl"])
        .assert()
        .success()
        .stdout(predicate::str::contains("CREATE TABLE"));
}

#[test]
fn test_schema_json_schema() {
    ensure_fixture();
    pq().args(["schema", &fixture_path(), "--style", "json-schema"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"$schema\""));
}

#[test]
fn test_schema_pyarrow() {
    ensure_fixture();
    pq().args(["schema", &fixture_path(), "--style", "pyarrow"])
        .assert()
        .success()
        .stdout(predicate::str::contains("import pyarrow as pa"))
        .stdout(predicate::str::contains("pa.schema("));
}

#[test]
fn test_head() {
    ensure_fixture();
    pq().args(["head", &fixture_path(), "-n", "5", "-f", "jsonl"])
        .assert()
        .success()
        .stdout(predicate::str::contains("user_0"))
        .stdout(predicate::str::contains("user_4"));
}

#[test]
fn test_tail() {
    ensure_fixture();
    pq().args(["tail", &fixture_path(), "-n", "3", "-f", "jsonl"])
        .assert()
        .success()
        .stdout(predicate::str::contains("user_97"))
        .stdout(predicate::str::contains("user_99"));
}

#[test]
fn test_count() {
    ensure_fixture();
    pq().args(["count", &fixture_path(), "-f", "json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"count\""));
}

#[test]
fn test_cat_limit() {
    ensure_fixture();
    pq().args(["cat", &fixture_path(), "--limit", "3", "-f", "jsonl"])
        .assert()
        .success()
        .stdout(predicate::str::contains("user_0"))
        .stdout(predicate::str::contains("user_2"));
}

#[test]
fn test_cat_columns() {
    ensure_fixture();
    pq().args([
        "cat",
        &fixture_path(),
        "--limit",
        "2",
        "-c",
        "id,name",
        "-f",
        "jsonl",
    ])
    .assert()
    .success()
    .stdout(predicate::str::contains("\"id\""))
    .stdout(predicate::str::contains("\"name\""));
}

#[test]
fn test_cat_with_where() {
    ensure_fixture();
    pq().args(["cat", &fixture_path(), "-w", "id < 3", "-f", "jsonl"])
        .assert()
        .success()
        .stdout(predicate::str::contains("user_0"))
        .stdout(predicate::str::contains("user_2"));
}

#[test]
fn test_stats() {
    ensure_fixture();
    pq().args(["stats", &fixture_path(), "-f", "json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"column_name\""));
}

#[test]
fn test_layout() {
    ensure_fixture();
    pq().args(["layout", &fixture_path(), "-f", "json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"num_row_groups\""));
}

#[test]
fn test_sql() {
    ensure_fixture();
    pq().args([
        "sql",
        &format!("SELECT count(*) as cnt FROM '{}'", fixture_path()),
        "-f",
        "jsonl",
    ])
    .assert()
    .success()
    .stdout(predicate::str::contains("100"));
}

// ── `-f`/`--format` semantics on `sql -o` (PART 1) ───────────────────────
//
// Same bug as `export`: `sql -o out.csv -f json` used to silently write
// CSV (extension-inferred), never consulting `-f` at all. Chosen
// semantics mirror `export`, with one addition: a `.parquet` extension
// always wins, since `-f` has no value that means "write real Parquet".

#[test]
fn test_sql_explicit_format_overrides_conflicting_extension_with_warning() {
    ensure_fixture();
    let tmp = TempDir::new().unwrap();
    let output = tmp.path().join("out.csv");

    pq().args([
        "sql",
        &format!("SELECT id, name FROM '{}' LIMIT 2", fixture_path()),
        "-o",
        output.to_str().unwrap(),
        "-f",
        "json",
    ])
    .assert()
    .success()
    .stderr(predicate::str::contains("overrides the format implied by"));

    let content = fs::read_to_string(&output).unwrap();
    let rows: Vec<serde_json::Value> = serde_json::from_str(&content)
        .unwrap_or_else(|e| panic!("expected JSON (per -f json), got {content:?}: {e}"));
    assert_eq!(rows.len(), 2);
}

#[test]
fn test_sql_parquet_extension_always_wins_over_format_flag() {
    // `-f` has no "parquet" value, so a `.parquet` output extension must
    // always win — with a note, since an explicit `-f` is still ignored.
    ensure_fixture();
    let tmp = TempDir::new().unwrap();
    let output = tmp.path().join("out.parquet");

    pq().args([
        "sql",
        &format!("SELECT id, name FROM '{}' LIMIT 2", fixture_path()),
        "-o",
        output.to_str().unwrap(),
        "-f",
        "csv",
    ])
    .assert()
    .success()
    .stderr(predicate::str::contains("always wins"));

    // A real Parquet file, not CSV text under a .parquet name.
    let bytes = fs::read(&output).unwrap();
    assert!(
        bytes.starts_with(b"PAR1"),
        "expected a real Parquet file (PAR1 magic), got {} bytes starting {:?}",
        bytes.len(),
        &bytes[..bytes.len().min(16)]
    );
}

#[test]
fn test_sql_unrecognized_extension_without_format_errors() {
    ensure_fixture();
    let tmp = TempDir::new().unwrap();
    let output = tmp.path().join("out.txt");

    pq().args([
        "sql",
        &format!("SELECT id FROM '{}' LIMIT 2", fixture_path()),
        "-o",
        output.to_str().unwrap(),
    ])
    .assert()
    .failure()
    .stderr(predicate::str::contains("cannot determine output format"));

    assert!(
        !output.exists(),
        "no file should be created when the format can't be determined"
    );
}

#[test]
fn test_jq() {
    ensure_fixture();
    pq().args(["jq", &fixture_path(), ".name", "-r"])
        .assert()
        .success()
        .stdout(predicate::str::starts_with("user_0\n"));
}

#[test]
fn test_jq_slurp() {
    ensure_fixture();
    pq().args(["jq", &fixture_path(), "length", "--slurp"])
        .assert()
        .success()
        .stdout(predicate::str::contains("100"));
}

#[test]
fn test_sample_seed() {
    ensure_fixture();
    let output1 = pq()
        .args(["sample", &fixture_path(), "-n", "3", "--seed", "42"])
        .output()
        .unwrap();
    let output2 = pq()
        .args(["sample", &fixture_path(), "-n", "3", "--seed", "42"])
        .output()
        .unwrap();
    assert_eq!(output1.stdout, output2.stdout);
    assert!(!output1.stdout.is_empty());
}

#[test]
fn test_import_and_select() {
    let tmp = TempDir::new().unwrap();
    let jsonl_path = tmp.path().join("input.jsonl");
    let parquet_path = tmp.path().join("output.parquet");
    let selected_path = tmp.path().join("selected.parquet");

    fs::write(
        &jsonl_path,
        r#"{"a":1,"b":"hello"}
{"a":2,"b":"world"}
{"a":3,"b":"test"}
"#,
    )
    .unwrap();

    pq().args([
        "import",
        jsonl_path.to_str().unwrap(),
        "-o",
        parquet_path.to_str().unwrap(),
    ])
    .assert()
    .success()
    .stderr(predicate::str::contains("3 rows"));

    pq().args(["count", parquet_path.to_str().unwrap(), "-f", "json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"count\""));

    pq().args([
        "select",
        parquet_path.to_str().unwrap(),
        "-c",
        "a",
        "-o",
        selected_path.to_str().unwrap(),
    ])
    .assert()
    .success();

    pq().args(["cat", selected_path.to_str().unwrap(), "-f", "jsonl"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"a\":1"));
}

#[test]
fn test_slice() {
    ensure_fixture();
    let tmp = TempDir::new().unwrap();
    let output = tmp.path().join("sliced.parquet");

    pq().args([
        "slice",
        &fixture_path(),
        "--offset",
        "10",
        "--limit",
        "5",
        "-o",
        output.to_str().unwrap(),
    ])
    .assert()
    .success()
    .stderr(predicate::str::contains("5 rows"));

    pq().args(["count", output.to_str().unwrap(), "-f", "json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"count\""));
}

#[test]
fn test_merge() {
    ensure_fixture();
    let tmp = TempDir::new().unwrap();
    let output = tmp.path().join("merged.parquet");

    pq().args([
        "merge",
        &fixture_path(),
        &fixture_path(),
        "-o",
        output.to_str().unwrap(),
    ])
    .assert()
    .success()
    .stderr(predicate::str::contains("200 rows"));

    pq().args(["count", output.to_str().unwrap(), "-f", "json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"count\""));
}

#[test]
fn test_merge_strict_rejects_mismatch() {
    ensure_fixture();
    let tmp = TempDir::new().unwrap();
    let subset = tmp.path().join("subset.parquet");
    pq().args([
        "select",
        &fixture_path(),
        "-c",
        "id,name",
        "-o",
        subset.to_str().unwrap(),
    ])
    .assert()
    .success();

    let output = tmp.path().join("merged.parquet");
    pq().args([
        "merge",
        &fixture_path(),
        subset.to_str().unwrap(),
        "--schema-mode",
        "strict",
        "-o",
        output.to_str().unwrap(),
    ])
    .assert()
    .failure()
    .stderr(predicate::str::contains("Schema mismatch"));
}

#[test]
fn test_merge_union() {
    ensure_fixture();
    let tmp = TempDir::new().unwrap();
    let subset = tmp.path().join("subset.parquet");
    pq().args([
        "select",
        &fixture_path(),
        "-c",
        "id,name",
        "-o",
        subset.to_str().unwrap(),
    ])
    .assert()
    .success();

    let output = tmp.path().join("merged.parquet");
    pq().args([
        "merge",
        &fixture_path(),
        subset.to_str().unwrap(),
        "--schema-mode",
        "union",
        "-o",
        output.to_str().unwrap(),
    ])
    .assert()
    .success()
    .stderr(predicate::str::contains("200 rows"));

    let schema_output = pq()
        .args(["schema", output.to_str().unwrap(), "--style", "ddl"])
        .output()
        .unwrap();
    let schema = String::from_utf8(schema_output.stdout).unwrap();
    assert!(schema.contains("id"), "union schema should contain id");
    assert!(schema.contains("name"), "union schema should contain name");
    assert!(schema.contains("age"), "union schema should contain age");
    assert!(
        schema.contains("score"),
        "union schema should contain score"
    );
    assert!(
        schema.contains("active"),
        "union schema should contain active"
    );
    assert!(schema.contains("city"), "union schema should contain city");

    let cat_output = pq()
        .args([
            "cat",
            output.to_str().unwrap(),
            "-f",
            "jsonl",
            "--limit",
            "1",
            "--offset",
            "100",
        ])
        .output()
        .unwrap();
    let row: serde_json::Value =
        serde_json::from_str(String::from_utf8(cat_output.stdout).unwrap().trim()).unwrap();
    assert!(row["age"].is_null(), "subset row should have null age");
    assert!(row["score"].is_null(), "subset row should have null score");
}

#[test]
fn test_merge_intersect() {
    ensure_fixture();
    let tmp = TempDir::new().unwrap();
    let subset = tmp.path().join("subset.parquet");
    pq().args([
        "select",
        &fixture_path(),
        "-c",
        "id,name,age",
        "-o",
        subset.to_str().unwrap(),
    ])
    .assert()
    .success();

    let output = tmp.path().join("merged.parquet");
    pq().args([
        "merge",
        &fixture_path(),
        subset.to_str().unwrap(),
        "--schema-mode",
        "intersect",
        "-o",
        output.to_str().unwrap(),
    ])
    .assert()
    .success()
    .stderr(predicate::str::contains("200 rows"));

    let schema_output = pq()
        .args(["schema", output.to_str().unwrap(), "--style", "ddl"])
        .output()
        .unwrap();
    let schema = String::from_utf8(schema_output.stdout).unwrap();
    assert!(schema.contains("id"), "intersect schema should contain id");
    assert!(
        schema.contains("name"),
        "intersect schema should contain name"
    );
    assert!(
        schema.contains("age"),
        "intersect schema should contain age"
    );
    assert!(
        !schema.contains("score"),
        "intersect schema should NOT contain score"
    );
    assert!(
        !schema.contains("active"),
        "intersect schema should NOT contain active"
    );
    assert!(
        !schema.contains("city"),
        "intersect schema should NOT contain city"
    );
}

#[test]
fn test_capabilities() {
    pq().args(["capabilities", "-f", "json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"tool\""));
}

#[test]
fn test_csv_output() {
    ensure_fixture();
    pq().args(["head", &fixture_path(), "-n", "2", "-f", "csv"])
        .assert()
        .success()
        .stdout(predicate::str::contains("active,age,city,id,name,score"));
}

#[test]
fn test_nonexistent_file() {
    pq().args(["info", "nonexistent.parquet"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("Error"));
}

#[test]
fn test_help() {
    pq().arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("Parquet Swiss Army Knife"));
}

/// `pq --help` and `pq capabilities` used to hand-duplicate the tagline and
/// had drifted (one had an em-dash, the other an ASCII hyphen) — see
/// crate::cli::TAGLINE, now the single source both consume. This locks the
/// exact string so a future edit to only one of them fails loudly instead
/// of shipping the same tool describing itself two different ways
/// depending on which subcommand you hit.
#[test]
fn test_tagline_matches_between_help_and_capabilities() {
    let tagline = "A Parquet Swiss Army Knife - inspect, query, transform, and view Parquet files";

    pq().arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains(tagline));

    pq().args(["capabilities", "-f", "json"])
        .assert()
        .success()
        .stdout(predicate::str::contains(tagline));
}

// ── Complex / nested type tests ─────────────────────────────────────────

/// Unlike `test_data.parquet`, `nested_data.parquet` is NOT tracked in git —
/// only its `nested_data.jsonl` source is. It's generated once per test
/// binary process into `generated_fixture_dir()` (a real `TempDir`, torn
/// down automatically on exit) rather than into `tests/fixtures/` in the
/// source tree, so it can never show up as an untracked file in `git
/// status` and parallel test runs across worktrees can't collide on it.
fn nested_fixture_path() -> String {
    static PARQUET: OnceLock<PathBuf> = OnceLock::new();
    PARQUET
        .get_or_init(|| {
            let jsonl_path = workspace_root().join("tests/fixtures/nested_data.jsonl");
            let parquet = generated_fixture_dir().join("nested_data.parquet");
            pq().args([
                "import",
                jsonl_path.to_str().unwrap(),
                "-o",
                parquet.to_str().unwrap(),
            ])
            .assert()
            .success();
            parquet
        })
        .to_str()
        .unwrap()
        .to_string()
}

fn ensure_nested_fixture() {
    // Generation now happens lazily inside `nested_fixture_path` itself;
    // this is kept only so existing call sites (`ensure_nested_fixture();
    // ... &nested_fixture_path()`) don't all need editing.
    nested_fixture_path();
}

#[test]
fn test_nested_convert_roundtrip() {
    let tmp = TempDir::new().unwrap();
    let jsonl_in = workspace_root().join("tests/fixtures/nested_data.jsonl");
    let parquet = tmp.path().join("nested.parquet");

    pq().args([
        "import",
        jsonl_in.to_str().unwrap(),
        "-o",
        parquet.to_str().unwrap(),
    ])
    .assert()
    .success()
    .stderr(predicate::str::contains("5 rows"));

    let output = pq()
        .args([
            "cat",
            parquet.to_str().unwrap(),
            "--limit",
            "1",
            "-f",
            "jsonl",
        ])
        .output()
        .unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    let row: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();

    assert!(row["address"].is_object(), "address should be a struct");
    assert!(
        row["address"]["geo"].is_object(),
        "address.geo should be a nested struct"
    );
    assert!(
        row["address"]["geo"]["lat"].is_number(),
        "address.geo.lat should be a number"
    );
    assert!(row["tags"].is_array(), "tags should be an array");
    assert!(row["orders"].is_array(), "orders should be an array");

    let orders = row["orders"].as_array().unwrap();
    assert!(!orders.is_empty());
    assert!(
        orders[0]["item"].is_string(),
        "orders[0].item should be a string"
    );
    assert!(
        orders[0]["price"].is_number(),
        "orders[0].price should be a number"
    );
}

#[test]
fn test_nested_schema_tree() {
    ensure_nested_fixture();
    pq().args(["schema", &nested_fixture_path(), "-f", "table"])
        .assert()
        .success()
        .stdout(predicate::str::contains("address: struct"))
        .stdout(predicate::str::contains("geo: struct"))
        .stdout(predicate::str::contains("lat: float64"))
        .stdout(predicate::str::contains("orders: list<struct>"))
        .stdout(predicate::str::contains("tags: list<string>"));
}

#[test]
fn test_nested_schema_json_schema() {
    ensure_nested_fixture();
    let output = pq()
        .args(["schema", &nested_fixture_path(), "--style", "json-schema"])
        .output()
        .unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    let schema: serde_json::Value = serde_json::from_str(&stdout).unwrap();

    assert_eq!(schema["properties"]["address"]["type"], "object");
    assert!(schema["properties"]["address"]["properties"]["geo"].is_object());
    assert_eq!(
        schema["properties"]["address"]["properties"]["geo"]["type"],
        "object"
    );

    assert_eq!(schema["properties"]["orders"]["type"], "array");
    assert_eq!(schema["properties"]["orders"]["items"]["type"], "object");

    assert_eq!(schema["properties"]["tags"]["type"], "array");
    assert_eq!(schema["properties"]["tags"]["items"]["type"], "string");
}

#[test]
fn test_nested_schema_ddl() {
    ensure_nested_fixture();
    pq().args(["schema", &nested_fixture_path(), "--style", "ddl"])
        .assert()
        .success()
        .stdout(predicate::str::contains("STRUCT("))
        .stdout(predicate::str::contains("TEXT[]"));
}

#[test]
fn test_nested_cat_jsonl() {
    ensure_nested_fixture();
    let output = pq()
        .args(["cat", &nested_fixture_path(), "--limit", "1", "-f", "jsonl"])
        .output()
        .unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    let row: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();

    assert_eq!(row["address"]["geo"]["lat"], 47.6);
    assert_eq!(row["address"]["city"], "Seattle");
}

#[test]
fn test_nested_cat_json_array() {
    ensure_nested_fixture();
    let output = pq()
        .args(["cat", &nested_fixture_path(), "-f", "json"])
        .output()
        .unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    let rows: Vec<serde_json::Value> = serde_json::from_str(&stdout).unwrap();
    assert_eq!(rows.len(), 5);

    let carol = &rows[2];
    assert_eq!(carol["orders"].as_array().unwrap().len(), 0);
    assert_eq!(carol["tags"].as_array().unwrap().len(), 2);
}

#[test]
fn test_nested_jq_struct_field() {
    ensure_nested_fixture();
    pq().args(["jq", &nested_fixture_path(), ".address.city", "-r"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Seattle"))
        .stdout(predicate::str::contains("Portland"))
        .stdout(predicate::str::contains("Denver"));
}

#[test]
fn test_nested_jq_struct_of_struct() {
    ensure_nested_fixture();
    pq().args(["jq", &nested_fixture_path(), ".address.geo.lat"])
        .assert()
        .success()
        .stdout(predicate::str::contains("47.6"))
        .stdout(predicate::str::contains("45.5"));
}

#[test]
fn test_nested_jq_array_of_structs() {
    ensure_nested_fixture();
    pq().args(["jq", &nested_fixture_path(), ".orders[].item", "-r"])
        .assert()
        .success()
        .stdout(predicate::str::contains("laptop"))
        .stdout(predicate::str::contains("mouse"))
        .stdout(predicate::str::contains("keyboard"));
}

#[test]
fn test_nested_jq_transform() {
    ensure_nested_fixture();
    let output = pq()
        .args([
            "jq",
            &nested_fixture_path(),
            "{city: .address.city, num_orders: (.orders | length)}",
        ])
        .output()
        .unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    let lines: Vec<&str> = stdout.trim().lines().collect();
    assert_eq!(lines.len(), 5);

    let first: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
    assert_eq!(first["city"], "Seattle");
    assert_eq!(first["num_orders"], 2);
}

#[test]
fn test_nested_select_struct_column() {
    ensure_nested_fixture();
    let tmp = TempDir::new().unwrap();
    let output_path = tmp.path().join("selected.parquet");

    pq().args([
        "select",
        &nested_fixture_path(),
        "-c",
        "address",
        "-o",
        output_path.to_str().unwrap(),
    ])
    .assert()
    .success();

    let output = pq()
        .args([
            "cat",
            output_path.to_str().unwrap(),
            "--limit",
            "1",
            "-f",
            "jsonl",
        ])
        .output()
        .unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    let row: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();

    assert!(row["address"]["geo"]["lat"].is_number());
    assert!(row.get("id").is_none());
    assert!(row.get("name").is_none());
}

#[test]
fn test_nested_select_list_column() {
    ensure_nested_fixture();
    let tmp = TempDir::new().unwrap();
    let output_path = tmp.path().join("orders_only.parquet");

    pq().args([
        "select",
        &nested_fixture_path(),
        "-c",
        "name,orders",
        "-o",
        output_path.to_str().unwrap(),
    ])
    .assert()
    .success();

    let output = pq()
        .args(["cat", output_path.to_str().unwrap(), "-f", "jsonl"])
        .output()
        .unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    let rows: Vec<serde_json::Value> = stdout
        .trim()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(rows.len(), 5);

    let dave = &rows[3];
    assert_eq!(dave["name"], "Dave");
    assert_eq!(dave["orders"].as_array().unwrap().len(), 3);
}

#[test]
fn test_nested_slice() {
    ensure_nested_fixture();
    let tmp = TempDir::new().unwrap();
    let output_path = tmp.path().join("sliced.parquet");

    pq().args([
        "slice",
        &nested_fixture_path(),
        "--offset",
        "2",
        "--limit",
        "2",
        "-o",
        output_path.to_str().unwrap(),
    ])
    .assert()
    .success();

    let output = pq()
        .args(["cat", output_path.to_str().unwrap(), "-f", "jsonl"])
        .output()
        .unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    let rows: Vec<serde_json::Value> = stdout
        .trim()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(rows.len(), 2);

    assert_eq!(rows[0]["name"], "Carol");
    assert_eq!(rows[1]["name"], "Dave");
    assert!(rows[0]["address"]["geo"]["lat"].is_number());
}

#[test]
fn test_nested_merge() {
    ensure_nested_fixture();
    let tmp = TempDir::new().unwrap();
    let output_path = tmp.path().join("merged.parquet");

    pq().args([
        "merge",
        &nested_fixture_path(),
        &nested_fixture_path(),
        "-o",
        output_path.to_str().unwrap(),
    ])
    .assert()
    .success()
    .stderr(predicate::str::contains("10 rows"));

    let output = pq()
        .args(["cat", output_path.to_str().unwrap(), "-f", "json"])
        .output()
        .unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    let rows: Vec<serde_json::Value> = serde_json::from_str(&stdout).unwrap();
    assert_eq!(rows.len(), 10);

    for i in 0..5 {
        assert_eq!(rows[i]["name"], rows[i + 5]["name"]);
        assert_eq!(rows[i]["address"], rows[i + 5]["address"]);
        assert_eq!(rows[i]["orders"], rows[i + 5]["orders"]);
    }
}

#[test]
fn test_nested_sql_struct_access() {
    ensure_nested_fixture();
    let query = format!(
        "SELECT name, address['city'] as city FROM '{}' WHERE address['city'] = 'Seattle'",
        nested_fixture_path()
    );
    pq().args(["sql", &query, "-f", "jsonl"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Alice"))
        .stdout(predicate::str::contains("Seattle"));
}

#[test]
fn test_nested_info() {
    ensure_nested_fixture();
    let output = pq()
        .args(["info", &nested_fixture_path(), "-f", "json"])
        .output()
        .unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    let info: serde_json::Value = serde_json::from_str(&stdout).unwrap();

    assert_eq!(info["num_rows"], 5);
    assert_eq!(info["num_columns"], 11);
}

#[test]
fn test_nested_count() {
    ensure_nested_fixture();
    pq().args(["count", &nested_fixture_path(), "-f", "json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("5"));
}

#[test]
fn test_nested_head_table_output() {
    ensure_nested_fixture();
    pq().args(["head", &nested_fixture_path(), "-n", "2", "-f", "table"])
        .assert()
        .success();
}

#[test]
fn test_nested_cat_csv_output() {
    ensure_nested_fixture();
    pq().args(["head", &nested_fixture_path(), "-n", "2", "-f", "csv"])
        .assert()
        .success()
        .stdout(predicate::str::contains("address"))
        .stdout(predicate::str::contains("orders"));
}

#[test]
fn test_nested_stats() {
    ensure_nested_fixture();
    pq().args(["stats", &nested_fixture_path(), "-f", "json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("column_name"));
}

#[test]
fn test_nested_layout() {
    ensure_nested_fixture();
    pq().args(["layout", &nested_fixture_path(), "-f", "json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("num_row_groups"));
}

// ── Export tests ─────────────────────────────────────────────────────────

#[test]
fn test_export_jsonl() {
    ensure_fixture();
    let tmp = TempDir::new().unwrap();
    let output = tmp.path().join("export.jsonl");

    pq().args(["export", &fixture_path(), "-o", output.to_str().unwrap()])
        .assert()
        .success()
        .stderr(predicate::str::contains("100 rows"));

    let content = fs::read_to_string(&output).unwrap();
    let lines: Vec<&str> = content.trim().lines().collect();
    assert_eq!(lines.len(), 100);
    let row: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
    assert!(row["id"].is_number());
}

#[test]
fn test_export_csv() {
    ensure_fixture();
    let tmp = TempDir::new().unwrap();
    let output = tmp.path().join("export.csv");

    pq().args(["export", &fixture_path(), "-o", output.to_str().unwrap()])
        .assert()
        .success()
        .stderr(predicate::str::contains("100 rows"));

    let content = fs::read_to_string(&output).unwrap();
    let lines: Vec<&str> = content.trim().lines().collect();
    assert_eq!(lines.len(), 101);
    assert!(lines[0].contains("id"));
}

#[test]
fn test_export_json() {
    ensure_fixture();
    let tmp = TempDir::new().unwrap();
    let output = tmp.path().join("export.json");

    pq().args(["export", &fixture_path(), "-o", output.to_str().unwrap()])
        .assert()
        .success()
        .stderr(predicate::str::contains("100 rows"));

    let content = fs::read_to_string(&output).unwrap();
    let rows: Vec<serde_json::Value> = serde_json::from_str(&content).unwrap();
    assert_eq!(rows.len(), 100);
}

#[test]
fn test_export_stdout() {
    ensure_fixture();
    // Export to stdout should work with -f jsonl
    pq().args(["export", &fixture_path(), "-f", "jsonl", "--limit", "5"])
        .assert()
        .success();
}

// ── `-f`/`--format` semantics on `export` (PART 1) ───────────────────────
//
// Bug: `-f`/`--format` was silently ignored whenever `export` wrote to a
// file — the output path's extension governed unconditionally, and an
// unrecognized extension (e.g. `.parquet`) silently fell back to JSONL.
// `export data.parquet -o a.parquet -f csv` wrote JSONL into a file named
// `.parquet`, exit 0, no diagnostic. Separately, `-f csv` to *stdout* never
// worked at all: the stdout writer had no CSV branch and silently emitted
// JSONL instead. Chosen semantics (see `export::resolve_file_format`):
// extension governs by default; an explicit `-f` overrides it with a
// stderr note (never silently); if neither pins down a format, error
// instead of guessing.

#[test]
fn test_export_stdout_format_csv_actually_produces_csv() {
    // Regression test for the stdout-CSV bug: before the fix, this branch
    // didn't exist and `-f csv` silently fell through to the JSONL
    // catch-all. Run against the pre-fix binary this comment is describing
    // and it fails: stdout starts with `{"active":...`, not a CSV header.
    ensure_fixture();
    let assert = pq()
        .args(["export", &fixture_path(), "-f", "csv", "--limit", "2"])
        .assert()
        .success();
    let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    let mut lines = stdout.lines();
    let header = lines.next().expect("csv header line");
    assert!(
        header
            .split(',')
            .eq(["active", "age", "city", "id", "name", "score"]),
        "expected a CSV header, got: {header:?}"
    );
    assert!(
        !stdout.trim_start().starts_with('{'),
        "stdout looks like JSON, not CSV: {stdout:?}"
    );
}

#[test]
fn test_export_explicit_format_wins_over_unrecognized_extension() {
    // `-o out.parquet` has an extension `export` never recognizes as a
    // target (export produces CSV/JSON/JSONL, not Parquet) — before the
    // fix this silently defaulted to JSONL regardless of `-f`. An explicit
    // `-f csv` must now actually produce CSV.
    ensure_fixture();
    let tmp = TempDir::new().unwrap();
    let output = tmp.path().join("weird.parquet");

    pq().args([
        "export",
        &fixture_path(),
        "-o",
        output.to_str().unwrap(),
        "-f",
        "csv",
        "--limit",
        "2",
    ])
    .assert()
    .success();

    let content = fs::read_to_string(&output).unwrap();
    assert!(
        content.starts_with("active,age,city,id,name,score"),
        "expected CSV content, got: {content:?}"
    );
}

#[test]
fn test_export_explicit_format_overrides_conflicting_extension_with_warning() {
    // `-o out.csv -f json`: the flag was typed explicitly, so it wins — but
    // the loser (the `.csv` extension) must not be silent about losing.
    ensure_fixture();
    let tmp = TempDir::new().unwrap();
    let output = tmp.path().join("out.csv");

    pq().args([
        "export",
        &fixture_path(),
        "-o",
        output.to_str().unwrap(),
        "-f",
        "json",
        "--limit",
        "2",
    ])
    .assert()
    .success()
    .stderr(predicate::str::contains("overrides the format implied by"));

    let content = fs::read_to_string(&output).unwrap();
    let rows: Vec<serde_json::Value> = serde_json::from_str(&content)
        .unwrap_or_else(|e| panic!("expected JSON (per -f json), got {content:?}: {e}"));
    assert_eq!(rows.len(), 2);
}

#[test]
fn test_export_unrecognized_extension_without_format_errors() {
    // Neither a recognized extension nor an explicit `-f` pins down a
    // format. Before the fix this silently wrote JSONL; now it must fail
    // loudly instead of guessing, and must not leave a file behind.
    ensure_fixture();
    let tmp = TempDir::new().unwrap();
    let output = tmp.path().join("out.txt");

    pq().args(["export", &fixture_path(), "-o", output.to_str().unwrap()])
        .assert()
        .failure()
        .stderr(predicate::str::contains("cannot determine export format"));

    assert!(
        !output.exists(),
        "no file should be created when the format can't be determined"
    );
}

#[test]
fn test_export_invalid_file_format_errors_instead_of_panicking() {
    // `-f table`/`-f plain` have no file representation for `export`. This
    // used to reach an `unreachable!()` in the writer and panic; it must
    // now be a clean error.
    ensure_fixture();
    let tmp = TempDir::new().unwrap();
    let output = tmp.path().join("out.csv");

    pq().args([
        "export",
        &fixture_path(),
        "-o",
        output.to_str().unwrap(),
        "-f",
        "table",
    ])
    .assert()
    .failure()
    .stderr(predicate::str::contains("can't be written to a file"));
}

// ── Describe tests (now via stats --describe) ────────────────────────────

#[test]
fn test_describe() {
    ensure_fixture();
    pq().args(["stats", &fixture_path(), "--describe", "-f", "table"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Column"))
        .stdout(predicate::str::contains("Distinct"));
}

#[test]
fn test_describe_json() {
    ensure_fixture();
    let output = pq()
        .args(["stats", &fixture_path(), "--describe", "-f", "json"])
        .output()
        .unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    let desc: Vec<serde_json::Value> = serde_json::from_str(&stdout).unwrap();
    assert_eq!(desc.len(), 6);
    assert!(desc[0]["column"].is_string());
    assert!(desc[0]["count"].is_number());
}

// ── Grep tests ──────────────────────────────────────────────────────────

#[test]
fn test_grep() {
    ensure_fixture();
    pq().args(["grep", &fixture_path(), "Tokyo", "-f", "jsonl"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Tokyo"));
}

#[test]
fn test_grep_case_insensitive() {
    ensure_fixture();
    pq().args(["grep", &fixture_path(), "tokyo", "-i", "-f", "jsonl"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Tokyo"));
}

#[test]
fn test_grep_limit() {
    ensure_fixture();
    let output = pq()
        .args([
            "grep",
            &fixture_path(),
            "Tokyo",
            "--limit",
            "3",
            "-f",
            "jsonl",
        ])
        .output()
        .unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    let lines: Vec<&str> = stdout.trim().lines().collect();
    assert_eq!(lines.len(), 3);
}

#[test]
fn test_grep_no_match() {
    ensure_fixture();
    pq().args(["grep", &fixture_path(), "ZZZNOMATCH", "-f", "jsonl"])
        .assert()
        .failure();
}

// ── Split tests ─────────────────────────────────────────────────────────

#[test]
fn test_split_by_rows() {
    ensure_fixture();
    let tmp = TempDir::new().unwrap();
    let output_dir = tmp.path().join("parts");

    pq().args([
        "split",
        &fixture_path(),
        "--rows",
        "30",
        "-o",
        output_dir.to_str().unwrap(),
    ])
    .assert()
    .success()
    .stderr(predicate::str::contains("Split 100 rows into 4 files"));

    let parts: Vec<_> = std::fs::read_dir(&output_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .collect();
    assert_eq!(parts.len(), 4);
}

#[test]
fn test_split_by_partition() {
    ensure_fixture();
    let tmp = TempDir::new().unwrap();
    let output_dir = tmp.path().join("partitioned");

    pq().args([
        "split",
        &fixture_path(),
        "--partition-by",
        "city",
        "-o",
        output_dir.to_str().unwrap(),
    ])
    .assert()
    .success()
    .stderr(predicate::str::contains("5 partitions"));

    let dirs: Vec<_> = std::fs::read_dir(&output_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .collect();
    assert_eq!(dirs.len(), 5);
}

// ── Validate tests ──────────────────────────────────────────────────────

#[test]
fn test_validate() {
    ensure_fixture();
    pq().args(["validate", &fixture_path(), "-f", "table"])
        .assert()
        .success()
        .stdout(predicate::str::contains("VALID"));
}

#[test]
fn test_validate_json() {
    ensure_fixture();
    let output = pq()
        .args(["validate", &fixture_path(), "-f", "json"])
        .output()
        .unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    let result: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(result["valid"], true);
    assert_eq!(result["num_rows"], 100);
}

// ── Multi-file correctness tests ────────────────────────────────────────
//
// `tail`, `sample`, `count` and `merge` all take `Vec<String>` file
// arguments but historically diverged from `cat`/`head`/`grep` in how (or
// whether) they handled more than one: `tail` silently used only the last
// file, `sample` silently used only the first, and `count`/`merge` never
// ran their arguments through `files::resolve_files`, so a glob pattern
// that reaches the process unexpanded (quoted, or on a shell/platform that
// doesn't glob) was never expanded. These fixtures give each file a
// distinguishing `tag` and a known row count so a test can tell "touched
// every file" apart from "touched one file and got lucky".

/// Write a parquet file with `n` rows of `{"id": 0..n, "tag": tag}`, via
/// `pq import` — the same path real users hit, so these tests exercise the
/// actual CLI glob/multi-file handling rather than a synthetic writer.
fn write_tagged_fixture(dir: &Path, name: &str, tag: &str, n: usize) -> PathBuf {
    let jsonl_path = dir.join(format!("{name}.jsonl"));
    let parquet_path = dir.join(format!("{name}.parquet"));
    let mut body = String::new();
    for i in 0..n {
        body.push_str(&format!("{{\"id\":{i},\"tag\":\"{tag}\"}}\n"));
    }
    fs::write(&jsonl_path, body).unwrap();
    pq().args([
        "import",
        jsonl_path.to_str().unwrap(),
        "-o",
        parquet_path.to_str().unwrap(),
    ])
    .assert()
    .success();
    parquet_path
}

/// `tail` over several files must be the last N rows of the
/// *concatenation*, in argument order — matching `head`'s existing
/// treatment of multiple files as one logical stream (see `cat::run`,
/// which `head` dispatches into). a=5 rows, b=10, c=20 (ids 0..n-1 each);
/// asking for the last 25 of 35 total rows must cross the b/c boundary:
/// rows 5..9 of b (not 0..4) plus all of c, and none of a.
#[test]
fn test_tail_multi_file_concatenates_across_files() {
    let tmp = TempDir::new().unwrap();
    let a = write_tagged_fixture(tmp.path(), "a", "A", 5);
    let b = write_tagged_fixture(tmp.path(), "b", "B", 10);
    let c = write_tagged_fixture(tmp.path(), "c", "C", 20);

    let output = pq()
        .args([
            "tail",
            a.to_str().unwrap(),
            b.to_str().unwrap(),
            c.to_str().unwrap(),
            "-n",
            "25",
            "-f",
            "jsonl",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();

    assert_eq!(
        stdout.matches("\"tag\":\"A\"").count(),
        0,
        "no rows from `a` should appear in the last 25 of a 35-row concatenation:\n{stdout}"
    );
    assert_eq!(
        stdout.matches("\"tag\":\"B\"").count(),
        5,
        "exactly the last 5 rows of `b` (ids 5-9) should appear:\n{stdout}"
    );
    assert_eq!(
        stdout.matches("\"tag\":\"C\"").count(),
        20,
        "all 20 rows of `c` should appear:\n{stdout}"
    );
    assert!(
        stdout.contains("\"id\":5,\"tag\":\"B\""),
        "expected the first row of b's tail slice (id 5):\n{stdout}"
    );
    assert!(
        !stdout.contains("\"id\":4,\"tag\":\"B\""),
        "id 4 of b is before the 25-row tail boundary and must not appear:\n{stdout}"
    );
}

/// `sample` draws from the *concatenation* of all files (matching `count`'s
/// "sum across files" and `cat`/`head`'s single-logical-stream treatment),
/// not just the first file. `a` only has 5 rows total, so asking for 6
/// samples across a+b+c *must* include at least one row from b or c in any
/// correct implementation, regardless of the random draw.
#[test]
fn test_sample_multi_file_draws_from_all_files() {
    let tmp = TempDir::new().unwrap();
    let a = write_tagged_fixture(tmp.path(), "a", "A", 5);
    let b = write_tagged_fixture(tmp.path(), "b", "B", 10);
    let c = write_tagged_fixture(tmp.path(), "c", "C", 20);

    let output = pq()
        .args([
            "sample",
            a.to_str().unwrap(),
            b.to_str().unwrap(),
            c.to_str().unwrap(),
            "-n",
            "6",
            "--seed",
            "7",
            "-f",
            "jsonl",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();

    let row_count = stdout.matches("\"id\"").count();
    assert_eq!(
        row_count, 6,
        "requested 6 rows across 35 total; got {row_count}:\n{stdout}"
    );
    let non_a = stdout.matches("\"tag\":\"B\"").count() + stdout.matches("\"tag\":\"C\"").count();
    assert!(
        non_a >= 1,
        "a has only 5 rows total, so 6 samples across a+b+c must include b or c:\n{stdout}"
    );
}

/// `count` and `merge` must expand glob patterns themselves — `resolve_files`
/// is what every other multi-file command routes through, and a pattern
/// that arrives unexpanded (quoted, passed programmatically as here, or on
/// a shell/platform with no globbing) must not silently degrade.
#[test]
fn test_count_expands_glob_and_sums_across_files() {
    let tmp = TempDir::new().unwrap();
    write_tagged_fixture(tmp.path(), "multi-a", "A", 5);
    write_tagged_fixture(tmp.path(), "multi-b", "B", 10);
    write_tagged_fixture(tmp.path(), "multi-c", "C", 20);

    let pattern = tmp.path().join("multi-*.parquet");
    let output = pq()
        .args(["count", pattern.to_str().unwrap(), "-f", "json"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    let result: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(
        result["total"], 35,
        "expected 5+10+20=35 summed across the glob's 3 matches, got: {stdout}"
    );
}

/// A glob that matches zero files and a literal path that does not exist
/// are different problems and must be reported differently: the former
/// never reaches the filesystem layer (caught by `resolve_files` itself),
/// the latter fails trying to open exactly the path named.
#[test]
fn test_count_glob_no_match_differs_from_missing_literal_path() {
    let tmp = TempDir::new().unwrap();
    write_tagged_fixture(tmp.path(), "present", "A", 3);

    let no_match_glob = tmp.path().join("nope-*.parquet");
    let glob_output = pq()
        .args(["count", no_match_glob.to_str().unwrap(), "-f", "json"])
        .output()
        .unwrap();
    assert!(!glob_output.status.success());
    let glob_stderr = String::from_utf8_lossy(&glob_output.stderr);
    assert!(
        glob_stderr.contains("No files matched pattern"),
        "a glob matching nothing should say so distinctly: {glob_stderr}"
    );

    let missing_literal = tmp.path().join("definitely_missing.parquet");
    let literal_output = pq()
        .args(["count", missing_literal.to_str().unwrap(), "-f", "json"])
        .output()
        .unwrap();
    assert!(!literal_output.status.success());
    let literal_stderr = String::from_utf8_lossy(&literal_output.stderr);
    assert!(
        !literal_stderr.contains("No files matched pattern"),
        "a literal missing path is not a glob-match failure: {literal_stderr}"
    );
}

/// `merge` must expand a glob just like `count` does, not just accept
/// pre-expanded literal file lists.
#[test]
fn test_merge_expands_glob() {
    let tmp = TempDir::new().unwrap();
    write_tagged_fixture(tmp.path(), "part-a", "A", 5);
    write_tagged_fixture(tmp.path(), "part-b", "B", 10);
    write_tagged_fixture(tmp.path(), "part-c", "C", 20);

    let pattern = tmp.path().join("part-*.parquet");
    let output = tmp.path().join("merged.parquet");
    pq().args([
        "merge",
        pattern.to_str().unwrap(),
        "-o",
        output.to_str().unwrap(),
        "--schema-mode",
        "union",
    ])
    .assert()
    .success()
    .stderr(predicate::str::contains("35 rows"));

    let count_output = pq()
        .args(["count", output.to_str().unwrap(), "-f", "json"])
        .output()
        .unwrap();
    assert!(count_output.status.success());
    let stdout = String::from_utf8(count_output.stdout).unwrap();
    let result: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(result["count"], 35, "merged output: {stdout}");
}

// ── Layout correctness tests ────────────────────────────────────────────

/// `pq layout` must accumulate row offsets across row groups and account
/// for a preceding dictionary page in a column chunk's byte start. Ground
/// truth is read independently via the `parquet` crate's own metadata
/// reader (`SerializedFileReader`), not via `extract_physical_layout` or
/// any code path the CLI command shares — so this doesn't just check that
/// `layout.rs` agrees with itself.
#[test]
fn test_layout_row_offsets_and_dictionary_byte_start() {
    use arrow::array::{Int64Array, StringArray};
    use arrow::datatypes::{DataType, Field, Schema};
    use arrow::record_batch::RecordBatch;
    use parquet::file::reader::FileReader;
    use std::sync::Arc;

    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("multi_rg.parquet");

    let n: usize = 300;
    let ids: Vec<i64> = (0..n as i64).collect();
    let cats = ["alpha", "beta", "gamma", "delta"];
    let categories: Vec<&str> = (0..n).map(|i| cats[i % 4]).collect();

    let schema = Arc::new(Schema::new(vec![
        Field::new("id", DataType::Int64, false),
        Field::new("category", DataType::Utf8, false),
    ]));
    let batch = RecordBatch::try_new(
        schema,
        vec![
            Arc::new(Int64Array::from(ids)),
            Arc::new(StringArray::from(categories)),
        ],
    )
    .unwrap();

    let opts = pq_core::writer::WriteOptions {
        compression: parquet::basic::Compression::SNAPPY,
        max_row_group_size: 100,
    };
    pq_core::writer::write_batches(&path, &[batch], &opts).unwrap();

    // Independent ground truth, read directly from the file's own Parquet
    // metadata (bypasses layout.rs and extract_physical_layout entirely).
    let file = std::fs::File::open(&path).unwrap();
    let reader = parquet::file::reader::SerializedFileReader::new(file).unwrap();
    let meta = reader.metadata();
    assert!(
        meta.num_row_groups() >= 3,
        "fixture must have multiple row groups to exercise the row-offset bug; got {}",
        meta.num_row_groups()
    );

    let mut expected_row_start: i64 = 0;
    let mut expectations = Vec::new();
    let mut saw_dictionary = false;
    for rg_i in 0..meta.num_row_groups() {
        let rg = meta.row_group(rg_i);
        let row_start = expected_row_start;
        let row_end = row_start + rg.num_rows() - 1;
        expected_row_start += rg.num_rows();

        let col = rg.column(1); // "category": dictionary-encoded low-cardinality string
        let byte_start = col
            .dictionary_page_offset()
            .unwrap_or_else(|| col.data_page_offset());
        if col.dictionary_page_offset().is_some() {
            saw_dictionary = true;
        }
        let byte_end = byte_start + col.compressed_size();
        expectations.push((row_start, row_end, byte_start, byte_end));
    }
    assert!(
        saw_dictionary,
        "fixture must dictionary-encode `category` to exercise the dictionary-offset bug"
    );

    let output = pq()
        .args(["layout", path.to_str().unwrap(), "-f", "table"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();

    for (row_start, row_end, byte_start, byte_end) in &expectations {
        let row_marker = format!(
            "rows {}\u{2013}{}",
            format_thousands(*row_start),
            format_thousands(*row_end)
        );
        assert!(
            stdout.contains(&row_marker),
            "expected row range {row_marker:?} in layout output:\n{stdout}"
        );
        let byte_marker = format!(
            "{}\u{2013}{}",
            format_thousands(*byte_start),
            format_thousands(*byte_end)
        );
        assert!(
            stdout.contains(&byte_marker),
            "expected byte range {byte_marker:?} (dictionary-inclusive) in layout output:\n{stdout}"
        );
    }
}

/// Mirrors `commands::layout::format_number` (private to the `pq` binary,
/// so not reachable from an integration test) — comma-grouped digits, exactly
/// as the CLI renders them, so the markers above match the real output text.
fn format_thousands(n: impl std::fmt::Display) -> String {
    let s = n.to_string();
    let bytes = s.as_bytes();
    let mut result = String::with_capacity(s.len() + s.len() / 3);
    for (i, &b) in bytes.iter().rev().enumerate() {
        if i > 0 && i % 3 == 0 && b != b'-' {
            result.insert(0, ',');
        }
        result.insert(0, b as char);
    }
    result
}
