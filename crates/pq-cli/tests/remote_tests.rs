//! Integration tests for remote file access via SeaweedFS.
//!
//! These tests require a SeaweedFS container running with S3 + filer exposed.
//! They are ignored by default and only run when `--ignored` is passed:
//!
//!     cargo test --test remote_tests -- --ignored
//!
//! Setup (run once before tests):
//!
//!     make test-seaweed-up
//!
//! Teardown:
//!
//!     make test-seaweed-down

use assert_cmd::Command;
use predicates::prelude::*;
use std::path::{Path, PathBuf};
use std::process;
use std::sync::{Once, OnceLock};
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

/// `test_data.parquet` is committed to git — see the matching comment in
/// `cli_tests.rs::fixture_path`. It never needs generating here.
fn fixture_path(name: &str) -> String {
    let path = workspace_root().join("tests/fixtures").join(name);
    assert!(
        path.exists(),
        "tracked fixture missing: {} (it should be committed to git)",
        path.display()
    );
    path.to_str().unwrap().to_string()
}

/// Directory for fixtures generated at test time, never written into the
/// shared source tree. Same rationale and pattern as
/// `cli_tests.rs::generated_fixture_dir` — this is a *separate* test
/// binary/process, so it gets its own `TempDir` rather than sharing one.
fn generated_fixture_dir() -> &'static Path {
    static DIR: OnceLock<TempDir> = OnceLock::new();
    DIR.get_or_init(|| TempDir::new().expect("failed to create temp fixture dir"))
        .path()
}

/// `nested_data.parquet` is NOT tracked in git — only `nested_data.jsonl`
/// is. Previously this file relied on `cli_tests.rs` having already
/// generated `tests/fixtures/nested_data.parquet` as a side effect of an
/// unrelated test binary running first (`make test-integration` runs only
/// `cargo test --test remote_tests`, so that side effect never actually
/// happened — this was a latent bug: uploading a file that doesn't exist).
/// Generate it here instead, once per process, into a private temp dir.
fn nested_local_fixture_path() -> PathBuf {
    static PARQUET: OnceLock<PathBuf> = OnceLock::new();
    PARQUET
        .get_or_init(|| {
            let jsonl = workspace_root().join("tests/fixtures/nested_data.jsonl");
            let parquet = generated_fixture_dir().join("nested_data.parquet");
            pq().args([
                "import",
                jsonl.to_str().unwrap(),
                "-o",
                parquet.to_str().unwrap(),
            ])
            .assert()
            .success();
            parquet
        })
        .clone()
}

/// SeaweedFS filer HTTP endpoint for range-request reads.
const FILER_URL: &str = "http://localhost:8888";
/// SeaweedFS S3 API endpoint.
const S3_ENDPOINT: &str = "http://localhost:8333";
const S3_BUCKET: &str = "pq-test";
const S3_KEY: &str = "testkey";
const S3_SECRET: &str = "testsecret";

fn http_url(file: &str) -> String {
    format!("{FILER_URL}/buckets/{S3_BUCKET}/{file}")
}

fn s3_url(file: &str) -> String {
    format!("s3://{S3_BUCKET}/{file}")
}

/// Build a pq Command pre-configured with S3 env vars for SeaweedFS.
fn pq_s3() -> Command {
    let mut cmd = pq();
    cmd.env("AWS_ACCESS_KEY_ID", S3_KEY);
    cmd.env("AWS_SECRET_ACCESS_KEY", S3_SECRET);
    cmd.env("AWS_REGION", "us-east-1");
    cmd.env("AWS_ENDPOINT_URL", S3_ENDPOINT);
    cmd.env("AWS_ALLOW_HTTP", "true");
    cmd
}

/// Upload a local file to SeaweedFS via the aws CLI.
fn s3_upload(local: &str, remote_key: &str) {
    let status = process::Command::new("aws")
        .args([
            "--endpoint-url",
            S3_ENDPOINT,
            "s3",
            "cp",
            local,
            &format!("s3://{S3_BUCKET}/{remote_key}"),
        ])
        .env("AWS_ACCESS_KEY_ID", S3_KEY)
        .env("AWS_SECRET_ACCESS_KEY", S3_SECRET)
        .stdout(process::Stdio::null())
        .stderr(process::Stdio::null())
        .status()
        .expect("aws CLI not found — install it or skip remote tests");
    assert!(status.success(), "s3 upload failed for {local}");
}

/// Ensure the SeaweedFS bucket exists and test fixtures are uploaded.
/// Uses `Once` so that parallel test threads don't race on upload.
fn ensure_remote_fixtures() {
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        // Create bucket (idempotent — ignores AlreadyOwnedByYou)
        let _ = process::Command::new("aws")
            .args([
                "--endpoint-url",
                S3_ENDPOINT,
                "s3",
                "mb",
                &format!("s3://{S3_BUCKET}"),
            ])
            .env("AWS_ACCESS_KEY_ID", S3_KEY)
            .env("AWS_SECRET_ACCESS_KEY", S3_SECRET)
            .stdout(process::Stdio::null())
            .stderr(process::Stdio::null())
            .status();

        s3_upload(&fixture_path("test_data.parquet"), "test_data.parquet");
        s3_upload(
            nested_local_fixture_path().to_str().unwrap(),
            "nested_data.parquet",
        );
    });
}

// -------------------------------------------------------------------------
// HTTP tests (via SeaweedFS filer)
// -------------------------------------------------------------------------

#[test]
#[ignore]
fn test_http_info() {
    ensure_remote_fixtures();
    pq().args(["info", &http_url("test_data.parquet"), "-f", "json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"num_rows\": 100"));
}

#[test]
#[ignore]
fn test_http_schema() {
    ensure_remote_fixtures();
    pq().args(["schema", &http_url("test_data.parquet"), "-f", "table"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Schema (6 columns)"));
}

#[test]
#[ignore]
fn test_http_head() {
    ensure_remote_fixtures();
    pq().args([
        "head",
        &http_url("test_data.parquet"),
        "-n",
        "3",
        "-f",
        "jsonl",
    ])
    .assert()
    .success()
    .stdout(predicate::str::contains("\"id\":0"))
    .stdout(predicate::str::contains("\"id\":1"))
    .stdout(predicate::str::contains("\"id\":2"));
}

#[test]
#[ignore]
fn test_http_tail() {
    ensure_remote_fixtures();
    pq().args([
        "tail",
        &http_url("test_data.parquet"),
        "-n",
        "2",
        "-f",
        "jsonl",
    ])
    .assert()
    .success()
    .stdout(predicate::str::contains("\"id\":98"))
    .stdout(predicate::str::contains("\"id\":99"));
}

#[test]
#[ignore]
fn test_http_count() {
    ensure_remote_fixtures();
    pq().args(["count", &http_url("test_data.parquet"), "-f", "json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"count\": 100"));
}

#[test]
#[ignore]
fn test_http_cat_with_columns() {
    ensure_remote_fixtures();
    pq().args([
        "cat",
        &http_url("test_data.parquet"),
        "-c",
        "id,city",
        "-l",
        "2",
        "-f",
        "jsonl",
    ])
    .assert()
    .success()
    .stdout(predicate::str::contains("\"city\""))
    .stdout(predicate::str::contains("\"id\""))
    // Should NOT contain other columns
    .stdout(predicate::str::contains("\"score\"").not());
}

#[test]
#[ignore]
fn test_http_stats() {
    ensure_remote_fixtures();
    pq().args(["stats", &http_url("test_data.parquet"), "-f", "json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"column_name\": \"id\""));
}

#[test]
#[ignore]
fn test_http_layout() {
    ensure_remote_fixtures();
    pq().args(["layout", &http_url("test_data.parquet"), "-f", "json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"num_row_groups\": 1"));
}

#[test]
#[ignore]
fn test_http_jq() {
    ensure_remote_fixtures();
    pq().args([
        "jq",
        &http_url("test_data.parquet"),
        "{id, city}",
        "-f",
        "jsonl",
    ])
    .assert()
    .success()
    .stdout(predicate::str::contains("\"city\":\"New York\""));
}

#[test]
#[ignore]
fn test_http_nested() {
    ensure_remote_fixtures();
    // Checking only for the top-level "address" key would pass even if a
    // struct-nested-in-struct or list-nested-in-struct got flattened or
    // silently dropped while decoding the remote-fetched Arrow schema —
    // exactly the failure class covered locally by
    // pq-transform::schema_inference::tests::list_nested_in_struct_is_not_dropped.
    // Assert on the doubly-nested `address.geo.lat` value instead, so a
    // regression in remote nested-type handling actually fails this test.
    pq().args([
        "head",
        &http_url("nested_data.parquet"),
        "-n",
        "1",
        "-f",
        "jsonl",
    ])
    .assert()
    .success()
    .stdout(predicate::str::contains("\"address\""))
    .stdout(predicate::str::contains("\"geo\":{\"lat\":47.6"))
    .stdout(predicate::str::contains("\"tags\":[\"admin\",\"user\"]"));
}

// -------------------------------------------------------------------------
// S3 tests (via SeaweedFS S3 gateway)
// -------------------------------------------------------------------------

#[test]
#[ignore]
fn test_s3_info() {
    ensure_remote_fixtures();
    pq_s3()
        .args(["info", &s3_url("test_data.parquet"), "-f", "json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"num_rows\": 100"));
}

#[test]
#[ignore]
fn test_s3_head() {
    ensure_remote_fixtures();
    pq_s3()
        .args([
            "head",
            &s3_url("test_data.parquet"),
            "-n",
            "3",
            "-f",
            "jsonl",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"id\":0"))
        .stdout(predicate::str::contains("\"id\":1"))
        .stdout(predicate::str::contains("\"id\":2"));
}

#[test]
#[ignore]
fn test_s3_count() {
    ensure_remote_fixtures();
    pq_s3()
        .args(["count", &s3_url("test_data.parquet"), "-f", "json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"count\": 100"));
}

#[test]
#[ignore]
fn test_s3_schema() {
    ensure_remote_fixtures();
    pq_s3()
        .args(["schema", &s3_url("test_data.parquet"), "-f", "table"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Schema (6 columns)"));
}

#[test]
#[ignore]
fn test_s3_tail() {
    ensure_remote_fixtures();
    pq_s3()
        .args([
            "tail",
            &s3_url("test_data.parquet"),
            "-n",
            "2",
            "-f",
            "jsonl",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"id\":98"))
        .stdout(predicate::str::contains("\"id\":99"));
}

#[test]
#[ignore]
fn test_s3_jq() {
    ensure_remote_fixtures();
    pq_s3()
        .args([
            "jq",
            &s3_url("test_data.parquet"),
            ".city",
            "-r",
            "-f",
            "jsonl",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("New York"));
}

#[test]
#[ignore]
fn test_s3_sql() {
    ensure_remote_fixtures();
    let url = s3_url("test_data.parquet");
    let query = format!("SELECT count(*) as n FROM '{url}'");
    pq_s3()
        .args(["sql", &query, "-f", "jsonl"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"n\":100"));
}

#[test]
#[ignore]
fn test_s3_cat_with_where() {
    ensure_remote_fixtures();
    pq_s3()
        .args([
            "cat",
            &s3_url("test_data.parquet"),
            "-w",
            "city = 'Tokyo'",
            "-l",
            "3",
            "-f",
            "jsonl",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"city\":\"Tokyo\""));
}

#[test]
#[ignore]
fn test_s3_nested() {
    ensure_remote_fixtures();
    // `info` only reports metadata (row/column counts), which a nested-type
    // decode bug wouldn't necessarily touch — `info` on a file with silently
    // dropped nested fields would still report the same num_rows/num_columns
    // and this test would pass regardless of whether nested types actually
    // decoded correctly. Read a row's actual nested content instead, the
    // same way test_http_nested does for the filer path, so a struct- or
    // list-nested-in-struct regression over the S3 gateway fails here.
    pq_s3()
        .args([
            "head",
            &s3_url("nested_data.parquet"),
            "-n",
            "1",
            "-f",
            "jsonl",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"address\""))
        .stdout(predicate::str::contains("\"geo\":{\"lat\":47.6"))
        .stdout(predicate::str::contains("\"tags\":[\"admin\",\"user\"]"));
}
