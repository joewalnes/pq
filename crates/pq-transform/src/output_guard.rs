//! Single choke point for writing a user-named output file.
//!
//! Every command that materialises a file the user named with `-o` must go
//! through [`with_atomic_output`]. Writing directly with `File::create(dest)`
//! truncates `dest` *before* the input readers have run, which destroys the
//! user's data whenever the output path resolves to one of the inputs
//! (`pq select a.parquet -c id -o a.parquet`). Staging into a sibling temp
//! file and renaming over the destination on success removes the whole class:
//! the input keeps its inode and its bytes until the operation has succeeded,
//! and a failure part-way through leaves the destination exactly as it was.
//!
//! This is deliberately *not* a "refuse if input == output" check. Path
//! equality is not decidable cheaply or portably — `./a.parquet` vs
//! `a.parquet`, symlinks, hardlinks and case-insensitive filesystems all
//! defeat string comparison, and even a dev/inode comparison would only turn
//! a useful in-place operation into an error. See `DIARY.md`.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static COUNTER: AtomicU64 = AtomicU64::new(0);

/// Guard that removes the staging file unless it has been committed.
struct TempFile {
    path: PathBuf,
    committed: bool,
}

impl Drop for TempFile {
    fn drop(&mut self) {
        if !self.committed {
            let _ = fs::remove_file(&self.path);
        }
    }
}

/// Build the staging path for `dest`, keeping the original extension so that
/// callers which sniff the output format from the file extension still see the
/// format the user asked for.
fn staging_path(dest: &Path, unique: u64) -> Option<PathBuf> {
    let parent = dest.parent()?;
    let name = dest.file_name()?.to_str()?;
    let pid = std::process::id();
    // The separator is a hyphen, not a dot, so that an extensionless
    // destination yields an extensionless staging name. Callers such as
    // `write_batches_to_file` pick the output format from the extension, and
    // the staging file must sniff to exactly what the destination sniffs to.
    let staged = match dest.extension().and_then(|e| e.to_str()) {
        Some(ext) => format!(".{name}-pq-tmp-{pid}-{unique}.{ext}"),
        None => format!(".{name}-pq-tmp-{pid}-{unique}"),
    };
    Some(parent.join(staged))
}

/// True when we can safely stage-and-rename onto `dest`.
///
/// We refuse to stage when the destination already exists as something other
/// than a regular file — a fifo, a character device such as `/dev/stdout`, or
/// a directory. Renaming over those would replace the special file with a
/// regular one, which is not what the caller asked for. In those cases we hand
/// the caller the destination path directly, preserving the old behaviour.
fn can_stage(dest: &Path, parent: &Path) -> bool {
    if !parent.as_os_str().is_empty() && !parent.is_dir() {
        // Parent directory does not exist (or is not a directory). Let the
        // caller fail on the real path so the error message names it.
        return false;
    }
    match fs::metadata(dest) {
        Ok(md) => md.is_file(),
        Err(_) => true, // does not exist yet
    }
}

/// Run `write` against a staging path, then atomically move the result into
/// place at `dest`.
///
/// `write` receives the path it should create and returns the value that
/// [`with_atomic_output`] returns. If `write` returns an error (or panics) the
/// staging file is removed and `dest` is left untouched.
///
/// Note on remote destinations: none of the writers in this workspace can emit
/// to `s3://`/`http://` URLs — they all go through `std::fs`. A URL-shaped
/// `dest` has no existing parent directory, so `can_stage` returns false and
/// the caller writes to the literal path exactly as it did before, producing
/// the same error it always produced.
pub fn with_atomic_output<T, E, F>(dest: &str, write: F) -> std::result::Result<T, E>
where
    F: FnOnce(&Path) -> std::result::Result<T, E>,
    E: From<std::io::Error>,
{
    let dest_path = Path::new(dest);

    // If the destination is a symlink, rename onto the file it points at so
    // the symlink itself survives the write.
    let final_path = match fs::canonicalize(dest_path) {
        Ok(resolved) => resolved,
        Err(_) => dest_path.to_path_buf(),
    };

    let parent = final_path.parent().unwrap_or_else(|| Path::new(""));
    if !can_stage(&final_path, parent) {
        return write(dest_path);
    }

    let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
    let Some(staged) = staging_path(&final_path, unique) else {
        return write(dest_path);
    };

    // Reserve the staging name so two concurrent pq processes cannot collide.
    match fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&staged)
    {
        Ok(_) => {}
        Err(_) => return write(dest_path),
    }
    let mut guard = TempFile {
        path: staged.clone(),
        committed: false,
    };

    let value = write(&staged)?;

    // Preserve the destination's permissions; a fresh staging file would
    // otherwise silently widen (or narrow) them on rename.
    if let Ok(md) = fs::metadata(&final_path) {
        let _ = fs::set_permissions(&staged, md.permissions());
    }

    fs::rename(&staged, &final_path).map_err(E::from)?;
    guard.committed = true;
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn tmpdir() -> tempfile::TempDir {
        tempfile::TempDir::new().unwrap()
    }

    #[test]
    fn writes_through_to_destination() {
        let dir = tmpdir();
        let dest = dir.path().join("out.txt");
        let n: usize = with_atomic_output::<_, std::io::Error, _>(dest.to_str().unwrap(), |p| {
            let mut f = fs::File::create(p)?;
            f.write_all(b"hello")?;
            Ok(5)
        })
        .unwrap();
        assert_eq!(n, 5);
        assert_eq!(fs::read_to_string(&dest).unwrap(), "hello");
    }

    #[test]
    fn destination_is_untouched_while_writer_runs() {
        let dir = tmpdir();
        let dest = dir.path().join("out.txt");
        fs::write(&dest, "original").unwrap();
        with_atomic_output::<_, std::io::Error, _>(dest.to_str().unwrap(), |p| {
            let mut f = fs::File::create(p)?;
            f.write_all(b"new")?;
            // The user's file must still be readable, in full, at this point.
            assert_eq!(fs::read_to_string(&dest).unwrap(), "original");
            Ok(())
        })
        .unwrap();
        assert_eq!(fs::read_to_string(&dest).unwrap(), "new");
    }

    #[test]
    fn failed_write_leaves_destination_intact_and_no_litter() {
        let dir = tmpdir();
        let dest = dir.path().join("out.txt");
        fs::write(&dest, "original").unwrap();
        let res: std::result::Result<(), std::io::Error> =
            with_atomic_output(dest.to_str().unwrap(), |p| {
                let mut f = fs::File::create(p)?;
                f.write_all(b"partial")?;
                Err(std::io::Error::other("boom"))
            });
        assert!(res.is_err());
        assert_eq!(fs::read_to_string(&dest).unwrap(), "original");
        let leftovers: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
            .filter(|n| n != "out.txt")
            .collect();
        assert!(
            leftovers.is_empty(),
            "staging litter left behind: {leftovers:?}"
        );
    }

    #[test]
    fn staging_path_preserves_extension() {
        let p = staging_path(Path::new("/x/data.parquet"), 7).unwrap();
        assert_eq!(p.extension().unwrap(), "parquet");
        assert!(p.file_name().unwrap().to_str().unwrap().starts_with('.'));

        let p = staging_path(Path::new("/x/data.csv"), 7).unwrap();
        assert_eq!(p.extension().unwrap(), "csv");

        let p = staging_path(Path::new("/x/data"), 7).unwrap();
        assert!(p.extension().is_none());
    }

    #[test]
    fn symlinked_destination_stays_a_symlink() {
        let dir = tmpdir();
        let real = dir.path().join("real.txt");
        fs::write(&real, "original").unwrap();
        let link = dir.path().join("link.txt");
        std::os::unix::fs::symlink(&real, &link).unwrap();

        with_atomic_output::<_, std::io::Error, _>(link.to_str().unwrap(), |p| {
            fs::write(p, b"new")?;
            Ok(())
        })
        .unwrap();

        assert!(fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink());
        assert_eq!(fs::read_to_string(&real).unwrap(), "new");
    }

    #[test]
    fn nonexistent_parent_falls_back_to_direct_write() {
        // e.g. an s3:// URL, or a typo'd directory: the caller must see the
        // same error it always saw, naming the path the user typed.
        let res: std::result::Result<(), std::io::Error> =
            with_atomic_output("s3://bucket/key.parquet", |p| {
                assert_eq!(p, Path::new("s3://bucket/key.parquet"));
                fs::File::create(p)?;
                Ok(())
            });
        assert!(res.is_err());
    }
}
