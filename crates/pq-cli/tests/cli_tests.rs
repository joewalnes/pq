use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use std::path::PathBuf;
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

fn fixture_path() -> String {
    workspace_root()
        .join("tests/fixtures/test_data.parquet")
        .to_str()
        .unwrap()
        .to_string()
}

fn ensure_fixture() {
    let parquet = workspace_root().join("tests/fixtures/test_data.parquet");
    if parquet.exists() {
        return;
    }
    let jsonl_path = workspace_root().join("tests/fixtures/test_data.jsonl");
    if !jsonl_path.exists() {
        let mut data = String::new();
        for i in 0..100 {
            let city = ["New York", "London", "Tokyo", "Paris", "Berlin"][i % 5];
            data.push_str(&format!(
                r#"{{"id":{},"name":"user_{}","age":{},"score":{},"active":{},"city":"{}"}}"#,
                i,
                i,
                20 + (i % 50),
                i as f64 * 1.5,
                i % 3 != 0,
                city
            ));
            data.push('\n');
        }
        fs::write(&jsonl_path, data).unwrap();
    }
    pq().args([
        "convert",
        jsonl_path.to_str().unwrap(),
        "-o",
        parquet.to_str().unwrap(),
    ])
    .assert()
    .success();
}

#[test]
fn test_info() {
    ensure_fixture();
    pq().args(["info", &fixture_path(), "-O", "json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"num_rows\""));
}

#[test]
fn test_info_table() {
    ensure_fixture();
    pq().args(["info", &fixture_path(), "-O", "table"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Rows:"))
        .stdout(predicate::str::contains("100"));
}

#[test]
fn test_schema_tree() {
    ensure_fixture();
    pq().args(["schema", &fixture_path(), "-O", "table"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Schema (6 columns)"));
}

#[test]
fn test_schema_ddl() {
    ensure_fixture();
    pq().args(["schema", &fixture_path(), "--format", "ddl"])
        .assert()
        .success()
        .stdout(predicate::str::contains("CREATE TABLE"));
}

#[test]
fn test_schema_json_schema() {
    ensure_fixture();
    pq().args(["schema", &fixture_path(), "--format", "json-schema"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"$schema\""));
}

#[test]
fn test_head() {
    ensure_fixture();
    pq().args(["head", &fixture_path(), "-n", "5", "-O", "jsonl"])
        .assert()
        .success()
        .stdout(predicate::str::contains("user_0"))
        .stdout(predicate::str::contains("user_4"));
}

#[test]
fn test_tail() {
    ensure_fixture();
    pq().args(["tail", &fixture_path(), "-n", "3", "-O", "jsonl"])
        .assert()
        .success()
        .stdout(predicate::str::contains("user_97"))
        .stdout(predicate::str::contains("user_99"));
}

#[test]
fn test_count() {
    ensure_fixture();
    pq().args(["count", &fixture_path(), "-O", "json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"count\""));
}

#[test]
fn test_cat_limit() {
    ensure_fixture();
    pq().args(["cat", &fixture_path(), "--limit", "3", "-O", "jsonl"])
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
        "-O",
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
    pq().args(["cat", &fixture_path(), "-w", "id < 3", "-O", "jsonl"])
        .assert()
        .success()
        .stdout(predicate::str::contains("user_0"))
        .stdout(predicate::str::contains("user_2"));
}

#[test]
fn test_stats() {
    ensure_fixture();
    pq().args(["stats", &fixture_path(), "-O", "json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"column_name\""));
}

#[test]
fn test_layout() {
    ensure_fixture();
    pq().args(["layout", &fixture_path(), "-O", "json"])
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
        "-O",
        "jsonl",
    ])
    .assert()
    .success()
    .stdout(predicate::str::contains("100"));
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
fn test_convert_and_select() {
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
        "convert",
        jsonl_path.to_str().unwrap(),
        "-o",
        parquet_path.to_str().unwrap(),
    ])
    .assert()
    .success()
    .stderr(predicate::str::contains("3 rows"));

    pq().args(["count", parquet_path.to_str().unwrap(), "-O", "json"])
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

    pq().args(["cat", selected_path.to_str().unwrap(), "-O", "jsonl"])
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

    pq().args(["count", output.to_str().unwrap(), "-O", "json"])
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

    pq().args(["count", output.to_str().unwrap(), "-O", "json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"count\""));
}

#[test]
fn test_capabilities() {
    pq().args(["capabilities", "-O", "json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"tool\""));
}

#[test]
fn test_csv_output() {
    ensure_fixture();
    pq().args(["head", &fixture_path(), "-n", "2", "-O", "csv"])
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
