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
use std::path::PathBuf;
use std::process;
use std::sync::Once;

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

fn fixture_path(name: &str) -> String {
    workspace_root()
        .join("tests/fixtures")
        .join(name)
        .to_str()
        .unwrap()
        .to_string()
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
        s3_upload(&fixture_path("nested_data.parquet"), "nested_data.parquet");
    });
}

// -------------------------------------------------------------------------
// HTTP tests (via SeaweedFS filer)
// -------------------------------------------------------------------------

#[test]
#[ignore]
fn test_http_info() {
    ensure_remote_fixtures();
    pq().args(["info", &http_url("test_data.parquet"), "-O", "json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"num_rows\": 100"));
}

#[test]
#[ignore]
fn test_http_schema() {
    ensure_remote_fixtures();
    pq().args(["schema", &http_url("test_data.parquet"), "-O", "table"])
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
        "-O",
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
        "-O",
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
    pq().args(["count", &http_url("test_data.parquet"), "-O", "json"])
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
        "-O",
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
    pq().args(["stats", &http_url("test_data.parquet"), "-O", "json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"column_name\": \"id\""));
}

#[test]
#[ignore]
fn test_http_layout() {
    ensure_remote_fixtures();
    pq().args(["layout", &http_url("test_data.parquet"), "-O", "json"])
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
        "-O",
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
    pq().args([
        "head",
        &http_url("nested_data.parquet"),
        "-n",
        "1",
        "-O",
        "jsonl",
    ])
    .assert()
    .success()
    .stdout(predicate::str::contains("\"address\""));
}

// -------------------------------------------------------------------------
// S3 tests (via SeaweedFS S3 gateway)
// -------------------------------------------------------------------------

#[test]
#[ignore]
fn test_s3_info() {
    ensure_remote_fixtures();
    pq_s3()
        .args(["info", &s3_url("test_data.parquet"), "-O", "json"])
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
            "-O",
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
        .args(["count", &s3_url("test_data.parquet"), "-O", "json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"count\": 100"));
}

#[test]
#[ignore]
fn test_s3_schema() {
    ensure_remote_fixtures();
    pq_s3()
        .args(["schema", &s3_url("test_data.parquet"), "-O", "table"])
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
            "-O",
            "jsonl",
        ])
        .assert()
        .success()
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
            "-O",
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
        .args(["sql", &query, "-O", "jsonl"])
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
            "-O",
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
    pq_s3()
        .args(["info", &s3_url("nested_data.parquet"), "-O", "json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"num_rows\""));
}
