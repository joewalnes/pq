//! Integration tests for remote file access.
//!
//! HTTP tests (`test_http_*`) run against an in-process, pure-`std` HTTP/1.1
//! server (see `TestHttpServer` below) that implements `Range:` / `206
//! Partial Content`. They need nothing beyond the test binary itself, run by
//! default in `cargo test --workspace`, and are NOT `#[ignore]`d.
//!
//! S3 tests (`test_s3_*`) still require a SeaweedFS container for the S3
//! gateway and remain `#[ignore]`d:
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
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, Once, OnceLock};
use std::thread;
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

/// SeaweedFS S3 API endpoint.
const S3_ENDPOINT: &str = "http://localhost:8333";
const S3_BUCKET: &str = "pq-test";
const S3_KEY: &str = "testkey";
const S3_SECRET: &str = "testsecret";

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
// In-process HTTP server with Range support
// -------------------------------------------------------------------------
//
// pq reads remote parquet via HTTP Range requests (see
// `pq-core::async_reader::stream_builder`, which does one HEAD for object
// size followed by ranged GETs for the footer and row-group data, through
// `object_store`'s HTTP backend). `object_store` *requires* a `206 Partial
// Content` + `Content-Range` response whenever it sends a `Range:` header —
// a `200` reply to a ranged request is treated as an error
// (`RangeNotSupported`), never silently accepted as the whole body. That
// means a server which ignores Range would make pq fail outright, not
// quietly serve wrong data — see `test_http_server_without_range_support`.
//
// This is a small hand-rolled HTTP/1.1 server (GET + HEAD, single Range per
// request) so this file adds no new dependency. Binds `127.0.0.1:0` (OS
// picks a free port) so concurrent test binaries — and concurrent tests
// within this binary, since each test gets its own server instance — never
// collide on a fixed port.

/// One request as observed by the test server, for asserting on afterwards.
#[derive(Debug, Clone)]
struct LoggedRequest {
    method: String,
    path: String,
    /// Raw value of the `Range:` header, if the client sent one.
    range: Option<String>,
}

struct ServerState {
    files: HashMap<String, Vec<u8>>,
    requests: Mutex<Vec<LoggedRequest>>,
    /// When set, Range headers are accepted but ignored: the server always
    /// answers with a full `200` body. Used to prove the happy-path tests
    /// are not vacuous — see `test_http_server_without_range_support`.
    disable_range: AtomicBool,
    /// When set, a ranged GET writes only half its promised bytes and then
    /// closes the connection, simulating a truncated/interrupted transfer.
    truncate_body: AtomicBool,
    shutdown: AtomicBool,
}

struct TestHttpServer {
    addr: SocketAddr,
    state: Arc<ServerState>,
    handle: Option<thread::JoinHandle<()>>,
}

impl TestHttpServer {
    fn start(files: HashMap<String, Vec<u8>>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind 127.0.0.1:0");
        let addr = listener.local_addr().expect("local_addr");
        let state = Arc::new(ServerState {
            files,
            requests: Mutex::new(Vec::new()),
            disable_range: AtomicBool::new(false),
            truncate_body: AtomicBool::new(false),
            shutdown: AtomicBool::new(false),
        });
        let accept_state = state.clone();
        let handle = thread::spawn(move || {
            for stream in listener.incoming() {
                if accept_state.shutdown.load(Ordering::SeqCst) {
                    break;
                }
                match stream {
                    Ok(s) => {
                        let conn_state = accept_state.clone();
                        thread::spawn(move || handle_connection(s, conn_state));
                    }
                    Err(_) => break,
                }
            }
        });
        TestHttpServer {
            addr,
            state,
            handle: Some(handle),
        }
    }

    /// Build the URL pq should be given for a file served by this server.
    fn url_for(&self, name: &str) -> String {
        format!("http://{}/{}", self.addr, name.trim_start_matches('/'))
    }

    fn requests(&self) -> Vec<LoggedRequest> {
        self.state.requests.lock().unwrap().clone()
    }

    fn range_request_count(&self) -> usize {
        self.requests().iter().filter(|r| r.range.is_some()).count()
    }

    fn set_disable_range(&self, disabled: bool) {
        self.state.disable_range.store(disabled, Ordering::SeqCst);
    }

    fn set_truncate_body(&self, truncate: bool) {
        self.state.truncate_body.store(truncate, Ordering::SeqCst);
    }
}

impl Drop for TestHttpServer {
    fn drop(&mut self) {
        // Unblock the accept() loop, which is otherwise parked forever.
        self.state.shutdown.store(true, Ordering::SeqCst);
        let _ = TcpStream::connect(self.addr);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

/// Parse a single-range `Range:` header value (`bytes=START-END`,
/// `bytes=START-`, or the suffix form `bytes=-N`) into an inclusive
/// `(start, end)` byte range clamped to `len`. Multi-range requests
/// (comma-separated) are not supported — pq/object_store never sends them.
fn parse_range(value: &str, len: usize) -> Option<(usize, usize)> {
    let v = value.trim().strip_prefix("bytes=")?;
    if v.contains(',') || len == 0 {
        return None;
    }
    let (start_s, end_s) = v.split_once('-')?;
    if start_s.is_empty() {
        let suffix: usize = end_s.parse().ok()?;
        if suffix == 0 {
            return None;
        }
        let suffix = suffix.min(len);
        return Some((len - suffix, len - 1));
    }
    let start: usize = start_s.parse().ok()?;
    if start >= len {
        return None;
    }
    let end: usize = if end_s.is_empty() {
        len - 1
    } else {
        end_s.parse().ok()?
    };
    Some((start, end.min(len - 1)))
}

fn write_status(stream: &mut TcpStream, code: u16, reason: &str, extra: &str, body: &[u8]) {
    let head = format!(
        "HTTP/1.1 {code} {reason}\r\nConnection: close\r\nContent-Length: {}\r\n{extra}\r\n",
        body.len()
    );
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(body);
    let _ = stream.flush();
}

fn handle_connection(stream: TcpStream, state: Arc<ServerState>) {
    let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(5)));
    let mut reader = BufReader::new(stream.try_clone().expect("clone stream"));
    let mut stream = stream;

    let mut request_line = String::new();
    if reader.read_line(&mut request_line).unwrap_or(0) == 0 {
        return; // dummy shutdown-unblock connection, or client hung up
    }
    let mut parts = request_line.trim().split_whitespace();
    let method = parts.next().unwrap_or("").to_string();
    let path = parts.next().unwrap_or("").to_string();

    let mut range_header: Option<String> = None;
    loop {
        let mut line = String::new();
        match reader.read_line(&mut line) {
            Ok(0) => break,
            Ok(_) => {
                let trimmed = line.trim_end();
                if trimmed.is_empty() {
                    break;
                }
                if let Some((name, value)) = trimmed.split_once(':') {
                    if name.trim().eq_ignore_ascii_case("range") {
                        range_header = Some(value.trim().to_string());
                    }
                }
            }
            Err(_) => break,
        }
    }

    state.requests.lock().unwrap().push(LoggedRequest {
        method: method.clone(),
        path: path.clone(),
        range: range_header.clone(),
    });

    if method.is_empty() {
        return;
    }

    let Some(body) = state.files.get(&path) else {
        write_status(&mut stream, 404, "Not Found", "", b"");
        return;
    };

    match method.as_str() {
        "HEAD" => {
            write_status(
                &mut stream,
                200,
                "OK",
                &format!("Content-Length: {}\r\n", body.len()),
                b"",
            );
        }
        "GET" => match range_header {
            Some(ref rv) if !state.disable_range.load(Ordering::SeqCst) => {
                match parse_range(rv, body.len()) {
                    Some((start, end)) => {
                        let slice = &body[start..=end];
                        let extra = format!(
                            "Content-Range: bytes {start}-{end}/{}\r\n",
                            body.len()
                        );
                        if state.truncate_body.load(Ordering::SeqCst) {
                            let head = format!(
                                "HTTP/1.1 206 Partial Content\r\nConnection: close\r\nContent-Length: {}\r\n{extra}\r\n",
                                slice.len()
                            );
                            let _ = stream.write_all(head.as_bytes());
                            let half = slice.len() / 2;
                            let _ = stream.write_all(&slice[..half]);
                            let _ = stream.flush();
                            // Deliberately stop here: the socket closes with
                            // fewer bytes than Content-Length promised.
                        } else {
                            write_status(&mut stream, 206, "Partial Content", &extra, slice);
                        }
                    }
                    None => write_status(&mut stream, 416, "Range Not Satisfiable", "", b""),
                }
            }
            _ => {
                // No Range header, or Range support disabled for this test:
                // answer with the full body under a plain 200.
                write_status(&mut stream, 200, "OK", "", body);
            }
        },
        _ => write_status(&mut stream, 405, "Method Not Allowed", "", b""),
    }
}

fn test_data_bytes() -> Vec<u8> {
    std::fs::read(fixture_path("test_data.parquet")).expect("read test_data.parquet fixture")
}

fn nested_data_bytes() -> Vec<u8> {
    std::fs::read(nested_local_fixture_path()).expect("read nested_data.parquet fixture")
}

/// Start a server exposing both fixtures pq's http tests need.
fn start_default_server() -> TestHttpServer {
    let mut files = HashMap::new();
    files.insert("/test_data.parquet".to_string(), test_data_bytes());
    files.insert("/nested_data.parquet".to_string(), nested_data_bytes());
    TestHttpServer::start(files)
}

/// Assert the request shape that proves pq actually used ranged reads
/// against this server, rather than downloading the whole file in one GET
/// (which would make the Range-handling code in the test server, and the
/// thing it's meant to cover in pq, entirely unexercised).
fn assert_used_ranged_reads(server: &TestHttpServer) {
    let reqs = server.requests();
    assert!(!reqs.is_empty(), "server received no requests at all");
    assert!(
        reqs.iter().any(|r| r.method == "HEAD"),
        "expected a HEAD request for object size/metadata; got {reqs:?}"
    );
    assert!(
        reqs.iter().any(|r| r.range.is_some()),
        "expected at least one ranged GET (pq must read remote parquet via \
         HTTP Range, not a full download); got {reqs:?}"
    );
    assert!(
        !reqs.iter().any(|r| r.method == "GET" && r.range.is_none()),
        "pq issued a full, unranged GET — the range mechanism this test \
         exists to cover was never exercised; got {reqs:?}"
    );
}

// -------------------------------------------------------------------------
// HTTP tests (in-process server, no Docker required)
// -------------------------------------------------------------------------

#[test]
fn test_http_info() {
    let server = start_default_server();
    pq().args(["info", &server.url_for("test_data.parquet"), "-f", "json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"num_rows\": 100"));
    assert_used_ranged_reads(&server);
}

#[test]
fn test_http_schema() {
    let server = start_default_server();
    pq().args(["schema", &server.url_for("test_data.parquet"), "-f", "table"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Schema (6 columns)"));
    assert_used_ranged_reads(&server);
}

#[test]
fn test_http_head() {
    let server = start_default_server();
    pq().args([
        "head",
        &server.url_for("test_data.parquet"),
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
    assert_used_ranged_reads(&server);
}

#[test]
fn test_http_tail() {
    let server = start_default_server();
    pq().args([
        "tail",
        &server.url_for("test_data.parquet"),
        "-n",
        "2",
        "-f",
        "jsonl",
    ])
    .assert()
    .success()
    .stdout(predicate::str::contains("\"id\":98"))
    .stdout(predicate::str::contains("\"id\":99"));
    assert_used_ranged_reads(&server);
}

#[test]
fn test_http_count() {
    let server = start_default_server();
    pq().args(["count", &server.url_for("test_data.parquet"), "-f", "json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"count\": 100"));
    assert_used_ranged_reads(&server);
}

#[test]
fn test_http_cat_with_columns() {
    let server = start_default_server();
    pq().args([
        "cat",
        &server.url_for("test_data.parquet"),
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
    assert_used_ranged_reads(&server);
}

#[test]
fn test_http_stats() {
    let server = start_default_server();
    pq().args(["stats", &server.url_for("test_data.parquet"), "-f", "json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"column_name\": \"id\""));
    assert_used_ranged_reads(&server);
}

#[test]
fn test_http_layout() {
    let server = start_default_server();
    pq().args(["layout", &server.url_for("test_data.parquet"), "-f", "json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"num_row_groups\": 1"));
    assert_used_ranged_reads(&server);
}

#[test]
fn test_http_jq() {
    let server = start_default_server();
    pq().args([
        "jq",
        &server.url_for("test_data.parquet"),
        "{id, city}",
        "-f",
        "jsonl",
    ])
    .assert()
    .success()
    .stdout(predicate::str::contains("\"city\":\"New York\""));
    assert_used_ranged_reads(&server);
}

#[test]
fn test_http_nested() {
    let server = start_default_server();
    // Checking only for the top-level "address" key would pass even if a
    // struct-nested-in-struct or list-nested-in-struct got flattened or
    // silently dropped while decoding the remote-fetched Arrow schema —
    // exactly the failure class covered locally by
    // pq-transform::schema_inference::tests::list_nested_in_struct_is_not_dropped.
    // Assert on the doubly-nested `address.geo.lat` value instead, so a
    // regression in remote nested-type handling actually fails this test.
    pq().args([
        "head",
        &server.url_for("nested_data.parquet"),
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
    assert_used_ranged_reads(&server);
}

// -------------------------------------------------------------------------
// HTTP error-path tests: these must fail *cleanly*, not panic or hang.
// -------------------------------------------------------------------------

#[test]
fn test_http_404_produces_clear_error() {
    let server = start_default_server();
    pq().args([
        "info",
        &server.url_for("does-not-exist.parquet"),
        "-f",
        "json",
    ])
    .assert()
    .failure()
    .stderr(predicate::str::contains("panicked").not());
    // The 404 must actually have been served by our handler, not skipped.
    assert!(
        server.requests().iter().any(|r| r.path == "/does-not-exist.parquet"),
        "server never saw the request for the missing file: {:?}",
        server.requests()
    );
}

#[test]
fn test_http_server_without_range_support_fails_clearly() {
    // This is the direct proof that the happy-path tests above are not
    // vacuous: object_store treats a 200 response to a ranged request as an
    // error (`RangeNotSupported`) rather than silently accepting it as the
    // whole file, so pq must fail here, not succeed with truncated/garbage
    // data and not hang.
    let server = start_default_server();
    server.set_disable_range(true);
    pq().args(["info", &server.url_for("test_data.parquet"), "-f", "json"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("panicked").not());
    // The server must actually have received a ranged request that it then
    // refused to honor — otherwise this would just be proving pq fails on
    // an unrelated broken server, not on lost Range support specifically.
    assert!(
        server.requests().iter().any(|r| r.range.is_some()),
        "pq never even attempted a ranged GET: {:?}",
        server.requests()
    );
}

#[test]
fn test_http_truncated_response_produces_clear_error() {
    // Simulates an interrupted network transfer: the server promises N
    // bytes via Content-Length, a 206 status and a Content-Range header,
    // then closes the connection after writing only half of them.
    let server = start_default_server();
    server.set_truncate_body(true);
    pq().args(["cat", &server.url_for("test_data.parquet"), "-f", "jsonl"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("panicked").not());
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
