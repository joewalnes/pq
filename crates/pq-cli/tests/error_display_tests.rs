//! Guards for the "pq prints the same error sentence twice" class of bug
//! (TODO.md P2). `PqError`'s `Display` used to interpolate its own
//! `source`'s text into its message *and* implement `source()` (via
//! `#[source]`/`#[from]`), so `pq-cli/src/main.rs`'s `eprintln!("Error:
//! {e:#}")` — which uses `anyhow`'s alternate `Display` to walk the full
//! source chain — printed the same text twice, e.g.:
//!
//!   Error: Failed to read parquet file 'x.parquet': EOF: file size of 0 is
//!   less than footer: EOF: file size of 0 is less than footer
//!
//! `crates/pq-core/src/error.rs` has unit tests that check this one hop at a
//! time (a variant's `Display` vs. its immediate `source()`). This file
//! checks the *end-to-end* shape instead: run real failing commands through
//! the actual built binary and assert the printed message never contains a
//! sentence immediately followed by a `": "`-joined repeat of itself. That
//! is call-site- and wording-agnostic — it would catch this bug shape in any
//! command, present or future, not just the ones exercised below.

use assert_cmd::Command;
use std::fs;
use tempfile::TempDir;

fn pq() -> Command {
    assert_cmd::cargo_bin_cmd!("pq")
}

/// Finds a `": "`-joined immediate self-repeat in `s`: a substring `T` (at
/// least `MIN_LEN` bytes, to avoid false positives on short incidental
/// repeats like a shared word) that is directly followed by `": "` and then
/// `T` again. This is exactly the shape `anyhow`'s `{:#}` produces when a
/// `PqError` variant's own `Display` already embeds its source's full text:
/// the source's text ends up printed once by the variant, then a second time
/// by the chain walk, joined by `anyhow`'s `": "` separator.
fn find_doubled_sentence(s: &str) -> Option<&str> {
    const MIN_LEN: usize = 8;
    let n = s.len();
    for i in 0..n {
        // Only try splits on char boundaries; skip otherwise.
        if !s.is_char_boundary(i) {
            continue;
        }
        let max_j = n.saturating_sub(2); // need room for ": " + repeat after j
        let mut j = i + MIN_LEN;
        while j <= max_j {
            if !s.is_char_boundary(j) {
                j += 1;
                continue;
            }
            let candidate = &s[i..j];
            if s[j..].starts_with(": ") {
                let rest = &s[j + 2..];
                if rest.starts_with(candidate) {
                    return Some(candidate);
                }
            }
            j += 1;
        }
    }
    None
}

/// A run of this checker against a string that never reached a real failure
/// (e.g. empty stderr from a subject that silently exited 0) must not read
/// as "no doubling found" == pass. Callers therefore always assert stderr is
/// non-empty and starts with "Error:" before calling this, so a broken
/// harness fails loudly instead of vacuously passing.
fn assert_error_shape_ok(stderr: &str, must_contain: &[&str]) {
    assert!(
        stderr.starts_with("Error:"),
        "expected a top-level `Error: ...` line, got: {stderr:?}"
    );
    if let Some(doubled) = find_doubled_sentence(stderr) {
        panic!(
            "error message repeats the same sentence twice (joined by \": \"): {doubled:?}\n\
             full message: {stderr:?}"
        );
    }
    for needle in must_contain {
        assert!(
            stderr.contains(needle),
            "expected context {needle:?} to survive in the error message: {stderr:?}"
        );
    }
}

fn stderr_of(cmd: &mut Command) -> String {
    let output = cmd.output().expect("failed to run pq");
    assert!(
        !output.status.success(),
        "expected this command to fail, but it exited 0"
    );
    String::from_utf8(output.stderr).expect("stderr was not valid UTF-8")
}

fn tmp() -> TempDir {
    TempDir::new().expect("failed to create temp dir")
}

#[test]
fn missing_file_error_is_not_doubled() {
    let dir = tmp();
    let missing = dir.path().join("nope.parquet");
    for sub in ["info", "schema", "head", "count"] {
        let stderr = stderr_of(pq().arg(sub).arg(&missing));
        assert_error_shape_ok(&stderr, &["nope.parquet"]);
    }
}

#[test]
fn zero_byte_parquet_error_is_not_doubled() {
    let dir = tmp();
    let path = dir.path().join("zero.parquet");
    fs::write(&path, b"").unwrap();
    let stderr = stderr_of(pq().arg("info").arg(&path));
    assert_error_shape_ok(&stderr, &["zero.parquet"]);
}

#[test]
fn truncated_parquet_error_is_not_doubled() {
    let dir = tmp();
    let path = dir.path().join("garbage.parquet");
    // Not a valid parquet footer at any size: triggers a corrupt-footer
    // ParquetError rather than the too-small-for-footer EOF error, giving
    // coverage of a second distinct ParquetRead source shape.
    fs::write(&path, vec![0u8; 64]).unwrap();
    let stderr = stderr_of(pq().arg("info").arg(&path));
    assert_error_shape_ok(&stderr, &["garbage.parquet"]);
}

#[test]
fn malformed_json_import_error_is_not_doubled() {
    let dir = tmp();
    let src = dir.path().join("bad.jsonl");
    fs::write(&src, b"{not valid json\n").unwrap();
    let out = dir.path().join("out.parquet");
    let stderr = stderr_of(pq().arg("import").arg(&src).arg("-o").arg(&out));
    assert_error_shape_ok(&stderr, &[]);
}

#[test]
fn import_to_a_directory_destination_error_is_not_doubled() {
    // Deterministic stand-in for TODO.md P2's named repro
    // (`import x.jsonl -o /dev/stdout > out.bin`): both hit the exact same
    // bug, `PqError::Io`'s `#[from]` doubling a plain `std::io::Error`'s
    // text. `-o` pointing at an existing directory reaches it via a plain,
    // OS-level `EISDIR` with no filesystem-race surface, unlike
    // `/dev/stdout` (see `dev_stdout_repro_is_flaky_under_concurrent_load_see_comment`
    // below for why that one isn't used as the load-bearing guard).
    let dir = tmp();
    let src = dir.path().join("x.jsonl");
    fs::write(&src, b"{\"a\":1}\n").unwrap();
    let existing_dir = dir.path().join("adir");
    fs::create_dir(&existing_dir).unwrap();
    let stderr = stderr_of(pq().arg("import").arg(&src).arg("-o").arg(&existing_dir));
    assert_error_shape_ok(&stderr, &[]);
}

#[test]
#[ignore = "flaky under concurrent test load, see comment; run manually with --ignored"]
fn dev_stdout_repro_is_flaky_under_concurrent_load_see_comment() {
    // This is the literal repro from TODO.md's P2 entry:
    // `pq import x.jsonl -o /dev/stdout > out.bin`. It reliably doubles on
    // unfixed code and reliably doesn't on fixed code *in isolation*, but it
    // depends on `/dev/stdout` (a symlink to `/dev/fd/1`) resolving through
    // `fs::metadata` to whatever fd 1 currently is — a real regular file
    // here, via `Stdio::from`. `output_guard.rs::can_stage` then sees
    // `md.is_file() == true` and tries to stage a sibling temp file inside
    // the *parent* of `/dev/fd/1`, i.e. the synthetic `/dev/fd` directory,
    // which fails with ENOENT — that failure is what supplies the IO error
    // this test is really trying to reach.
    //
    // Under heavy concurrent load (many other `pq-cli` test processes
    // forking at once — exactly what `cargo test --workspace` does), this
    // was observed to intermittently exit 0 instead: ~1 run in 5 under
    // `cargo test -p pq-cli`, 0 in 9 in isolation. That flip lives in
    // `output_guard.rs`'s `/dev/fd` handling, which is out of this change's
    // file scope (`crates/pq-transform/**` is not touched here) and is
    // unrelated to the doubled-message bug this file otherwise guards — so
    // rather than ship a gate that reports the state of the dice, this test
    // is kept for manual reproduction and `#[ignore]`d for CI. The
    // load-bearing automated guard for this exact `PqError::Io` shape is
    // `import_to_a_directory_destination_error_is_not_doubled` above.
    let dir = tmp();
    let src = dir.path().join("x.jsonl");
    fs::write(&src, b"{\"a\":1}\n").unwrap();
    let out_file = dir.path().join("captured_stdout.bin");
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_pq"))
        .arg("import")
        .arg(&src)
        .arg("-o")
        .arg("/dev/stdout")
        .stdout(std::process::Stdio::from(
            fs::File::create(&out_file).unwrap(),
        ))
        .output()
        .expect("failed to run pq");
    assert!(
        !output.status.success(),
        "expected this command to fail, but it exited 0 (see the flakiness \
         note above — rerun in isolation if this was under concurrent load)"
    );
    let stderr = String::from_utf8(output.stderr).expect("stderr was not valid UTF-8");
    assert_error_shape_ok(&stderr, &["/dev/stdout"]);
}

#[test]
fn detector_catches_a_known_doubled_string() {
    // Guards the guard: if `find_doubled_sentence` regresses (e.g. someone
    // "simplifies" it into a no-op), this fails loudly rather than letting
    // the tests above pass vacuously.
    let doubled = "Error: Failed to read parquet file 'x.parquet': EOF: file size of 0 is less than footer: EOF: file size of 0 is less than footer";
    assert_eq!(
        find_doubled_sentence(doubled),
        Some("EOF: file size of 0 is less than footer")
    );
    let fine =
        "Error: Failed to read parquet file 'x.parquet': EOF: file size of 0 is less than footer";
    assert_eq!(find_doubled_sentence(fine), None);
}
