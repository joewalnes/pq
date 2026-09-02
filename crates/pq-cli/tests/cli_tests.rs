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
        "import",
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

fn nested_fixture_path() -> String {
    workspace_root()
        .join("tests/fixtures/nested_data.parquet")
        .to_str()
        .unwrap()
        .to_string()
}

fn ensure_nested_fixture() {
    let parquet = workspace_root().join("tests/fixtures/nested_data.parquet");
    if parquet.exists() {
        return;
    }
    let jsonl_path = workspace_root().join("tests/fixtures/nested_data.jsonl");
    pq().args([
        "import",
        jsonl_path.to_str().unwrap(),
        "-o",
        parquet.to_str().unwrap(),
    ])
    .assert()
    .success();
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
