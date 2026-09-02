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
//!
//! # Two corrections to the first version of this module
//!
//! 1. **Stage-and-rename walks through `chmod 444`.** `rename(2)` needs write
//!    permission on the *directory*; the `File::create(dest)` it replaced
//!    needed write permission on the *file*. So the first version happily
//!    replaced a read-only destination and — because it also copied the old
//!    mode onto the replacement — left `ls -l` still showing `-r--r--r--`.
//!    [`ensure_writable`] restores the old contract by probing the
//!    destination the way the kernel would.
//!
//! 2. **A failure to create the staging file fell back to the destructive
//!    path.** The original `Err(_) => return write(dest_path)` handed the
//!    caller the real destination, which is exactly the `File::create(dest)`
//!    behaviour this module exists to prevent — reachable through a read-only
//!    parent directory, a destination name long enough that the staging name
//!    exceeds `NAME_MAX`, or stale staging litter from a killed run. All three
//!    now fail loudly with the destination untouched, and the staging name is
//!    drawn from a 64-bit random token instead of the pid, so litter from a
//!    dead process cannot arm a trap for a later one.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static COUNTER: AtomicU64 = AtomicU64::new(0);

/// POSIX `NAME_MAX`. The staging name is built to fit inside this so that a
/// destination with a legal-but-long file name cannot push it over the limit.
const NAME_MAX: usize = 255;
/// How much of the destination's own name we keep in the staging name. Purely
/// so a human (or `lsof`) can tell what a leftover staging file belonged to.
const NAME_STEM_BUDGET: usize = 40;
/// An "extension" longer than this is not a format anybody sniffs; drop it
/// rather than let it eat the whole name budget.
const EXT_BUDGET: usize = 180;
const MARKER: &str = "-pq-tmp-";
/// Fresh random names to try before giving up. Each collision is a ~2^-64
/// event, so anything past the first attempt means something pathological.
const STAGING_ATTEMPTS: usize = 8;

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

/// A 64-bit token with no relationship to the pid.
///
/// `RandomState` is seeded from the OS at process start, so two processes —
/// including two processes that happen to share a recycled pid — draw from
/// different streams. The counter and the clock keep successive calls within
/// one process distinct even in the (impossible in std today) case of a fixed
/// seed. This is a temp-file name, not a secret; combined with the `O_EXCL`
/// retry loop below it is sufficient.
fn random_token() -> u64 {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};

    let mut hasher = RandomState::new().build_hasher();
    hasher.write_u64(COUNTER.fetch_add(1, Ordering::Relaxed));
    hasher.write_u32(std::process::id());
    hasher.write_u128(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0),
    );
    hasher.finish()
}

/// Truncate `s` to at most `max_bytes`, never splitting a `char`.
fn truncate_on_char_boundary(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

/// Build a fresh staging path for `dest`.
///
/// The name is bounded by [`NAME_MAX`]: a 254-character destination name used
/// to produce a 270-character staging name, `create_new` failed with
/// `ENAMETOOLONG`, and the old code then wrote straight to the destination.
///
/// **The extension here is cosmetic and must not be used to resolve a format.**
/// An earlier version of this comment claimed the extension was preserved "so
/// that callers which sniff the output format from the file extension still
/// see the format the user asked for". That was wrong, and it cost a real bug:
/// `dest` here is already the *resolved symlink target*, so for
/// `-o link.parquet` where `link.parquet -> target.csv` the staging name ends
/// in `.csv`. A caller that sniffed it wrote CSV under a `.parquet` name with
/// exit 0. The fix is not to make this name mimic the destination — the two
/// will drift again — but for the caller to resolve the format **once**, from
/// the path the user typed, and pass it down; see
/// `pq_cli::commands::write_output::write_batches_as`. The extension survives
/// only so a leftover staging file is recognisable to a human.
///
/// Returns `None` only for paths that have no parent or no file name (`/`,
/// `..`), which are not writable destinations under any code path.
fn staging_path(dest: &Path) -> Option<PathBuf> {
    let parent = dest.parent()?;
    let name = dest.file_name()?.to_str()?;
    let token = format!("{:016x}", random_token());
    // The separator is a hyphen, not a dot, so that an extensionless
    // destination yields an extensionless staging name.
    let ext_part = match dest.extension().and_then(|e| e.to_str()) {
        Some(ext) if !ext.is_empty() && ext.len() <= EXT_BUDGET => format!(".{ext}"),
        _ => String::new(),
    };
    let fixed = 1 + MARKER.len() + token.len() + ext_part.len();
    let budget = NAME_MAX.checked_sub(fixed)?.min(NAME_STEM_BUDGET);
    let stem = truncate_on_char_boundary(name, budget);
    Some(parent.join(format!(".{stem}{MARKER}{token}{ext_part}")))
}

/// Resolve a symlink chain by hand.
///
/// `fs::canonicalize` cannot do this job: it fails outright on a *dangling*
/// symlink, so the first version of this module staged next to the link and
/// renamed over it, replacing the user's symlink with a regular file. The old
/// `File::create` followed the link and created its target. This restores that.
fn resolve_symlinks(path: &Path) -> PathBuf {
    let mut current = path.to_path_buf();
    // Same bound the kernel uses for `ELOOP`; a cycle must terminate.
    for _ in 0..40 {
        let is_link = fs::symlink_metadata(&current)
            .map(|md| md.file_type().is_symlink())
            .unwrap_or(false);
        if !is_link {
            return current;
        }
        let Ok(target) = fs::read_link(&current) else {
            return current;
        };
        current = if target.is_absolute() {
            target
        } else {
            match current.parent() {
                Some(parent) => parent.join(target),
                None => target,
            }
        };
    }
    current
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

/// Refuse to replace a destination the caller could not have opened for
/// writing.
///
/// **Why an open probe and not a mode check.** `rename(2)` does not consult the
/// destination file's permissions at all, so stage-and-rename silently defeats
/// `chmod 444` — the one mechanism users reach for to protect a file. Comparing
/// `md.permissions().mode()` against the caller's uid/gid would be wrong in
/// three separate ways: root bypasses the mode bits entirely, POSIX ACLs and
/// macOS `chflags uchg` can deny access the mode bits advertise, and
/// reimplementing the uid/gid/other selection is a well-known source of bugs.
/// Opening the file `O_WRONLY` (no `O_TRUNC`, no `O_CREAT`) asks the kernel the
/// exact question the old `File::create(dest)` asked, and answers it with the
/// same rules — ACLs, flags, read-only mounts and root's override included.
///
/// **Limits, honestly.** (1) It is a TOCTOU probe: permissions can change
/// between the probe and the `rename`, so this restores the old *contract*, not
/// a guarantee. (2) It only speaks for the destination file; the parent
/// directory's writability is exercised separately, by creating the staging
/// file. (3) It costs one `open`/`close` on the destination — harmless for a
/// regular file, which is why it runs only after `can_stage` has confirmed the
/// destination is one.
fn ensure_writable(dest: &str, final_path: &Path) -> io::Result<()> {
    match fs::OpenOptions::new().write(true).open(final_path) {
        Ok(_) => Ok(()),
        // Raced away between the metadata call and here; creating it is fine.
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(io::Error::new(
            e.kind(),
            format!("cannot write to {dest}: {e}"),
        )),
    }
}

fn staging_failure(dest: &str, err: &io::Error) -> io::Error {
    io::Error::new(
        err.kind(),
        format!(
            "cannot create a temporary file next to {dest}: {err}. \
             Refusing to write {dest} in place: if it is also an input, \
             writing it directly would destroy it."
        ),
    )
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
    with_atomic_output_named(dest, staging_path, write)
}

/// [`with_atomic_output`] with the staging-name generator injected, so that the
/// collision path can be tested deterministically. `name_for` is called once
/// per attempt and must return a *fresh* name each time.
fn with_atomic_output_named<T, E, F, N>(
    dest: &str,
    mut name_for: N,
    write: F,
) -> std::result::Result<T, E>
where
    F: FnOnce(&Path) -> std::result::Result<T, E>,
    N: FnMut(&Path) -> Option<PathBuf>,
    E: From<std::io::Error>,
{
    let dest_path = Path::new(dest);

    // If the destination is a symlink, rename onto the file it points at so
    // the symlink itself survives the write. Dangling links resolve too.
    let final_path = resolve_symlinks(dest_path);

    let parent = final_path.parent().unwrap_or_else(|| Path::new(""));
    if !can_stage(&final_path, parent) {
        return write(dest_path);
    }

    // `rename` ignores the destination's permissions; ask the kernel the
    // question `File::create(dest)` used to ask, before touching anything.
    if fs::metadata(&final_path).is_ok() {
        ensure_writable(dest, &final_path).map_err(E::from)?;
    }

    // Reserve the staging name so two concurrent pq processes cannot collide.
    // A name that cannot be built at all belongs to a path (`/`, `..`) that no
    // writer could create anyway; let the caller produce the familiar error.
    let Some(first) = name_for(&final_path) else {
        return write(dest_path);
    };

    let mut candidate = Some(first);
    let mut staged: Option<PathBuf> = None;
    let mut last_err: Option<io::Error> = None;
    for _ in 0..STAGING_ATTEMPTS {
        let Some(path) = candidate.take() else { break };
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(_) => {
                staged = Some(path);
                break;
            }
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
                // Stale litter from a killed run, or a genuine 1-in-2^64
                // collision. Draw a new name; never fall through to the
                // destination, which is what the first version of this module
                // did and what made the whole class reachable again.
                last_err = Some(e);
                candidate = name_for(&final_path);
            }
            Err(e) => {
                // Read-only parent directory, name too long, no space, ...
                return Err(E::from(staging_failure(dest, &e)));
            }
        }
    }
    let Some(staged) = staged else {
        let err = last_err.unwrap_or_else(|| io::Error::other("no staging name available"));
        return Err(E::from(staging_failure(dest, &err)));
    };

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
        let p = staging_path(Path::new("/x/data.parquet")).unwrap();
        assert_eq!(p.extension().unwrap(), "parquet");
        assert!(p.file_name().unwrap().to_str().unwrap().starts_with('.'));

        let p = staging_path(Path::new("/x/data.csv")).unwrap();
        assert_eq!(p.extension().unwrap(), "csv");

        let p = staging_path(Path::new("/x/data")).unwrap();
        assert!(p.extension().is_none());
    }

    // -----------------------------------------------------------------------
    // Staging-name uniqueness. The first version used `pid-counter`; macOS pids
    // wrap at 99999 and the counter always starts at 0, so the first staging
    // name of every process came from a ~100k space and a killed run could
    // leave litter that a later run would collide with.
    // -----------------------------------------------------------------------

    #[test]
    fn staging_names_are_unique_and_carry_no_pid() {
        let dest = Path::new("/x/data.parquet");
        let mut seen = std::collections::HashSet::new();
        for _ in 0..2000 {
            assert!(
                seen.insert(staging_path(dest).unwrap()),
                "staging name repeated within one process"
            );
        }
        let pid = std::process::id().to_string();
        let name = staging_path(dest).unwrap();
        let name = name.file_name().unwrap().to_str().unwrap().to_string();
        assert!(
            !name.contains(&format!("{MARKER}{pid}")),
            "staging name is still derived from the pid: {name}"
        );
    }

    #[test]
    fn staging_name_fits_name_max_for_a_legal_long_destination() {
        // 254 chars: legal on every filesystem pq supports. The old scheme
        // appended ~16 chars to the *whole* name and blew past NAME_MAX.
        let stem = "x".repeat(246);
        let dest = PathBuf::from("/x").join(format!("{stem}.parquet"));
        assert_eq!(dest.file_name().unwrap().len(), 254);
        let staged = staging_path(&dest).unwrap();
        let len = staged.file_name().unwrap().len();
        assert!(
            len <= NAME_MAX,
            "staging name is {len} bytes, over NAME_MAX"
        );
        assert_eq!(staged.extension().unwrap(), "parquet");
    }

    #[test]
    fn staging_name_is_bounded_for_a_pathological_extension() {
        let dest = PathBuf::from("/x").join(format!("a.{}", "e".repeat(400)));
        let staged = staging_path(&dest).unwrap();
        assert!(staged.file_name().unwrap().len() <= NAME_MAX);
    }

    #[test]
    fn multibyte_destination_name_is_not_split_mid_char() {
        let dest = PathBuf::from("/x").join(format!("{}.parquet", "é".repeat(200)));
        let staged = staging_path(&dest).unwrap();
        // Would have panicked on a non-boundary slice; also proves it is UTF-8.
        assert!(staged.file_name().unwrap().to_str().is_some());
        assert!(staged.file_name().unwrap().len() <= NAME_MAX);
    }

    // -----------------------------------------------------------------------
    // The destructive fallback. Every one of these used to end in
    // `write(dest_path)` — i.e. `File::create` on the user's file.
    // -----------------------------------------------------------------------

    #[test]
    fn occupied_staging_name_never_writes_the_destination() {
        // Deterministic stand-in for "stale staging litter from a SIGKILLed
        // run, hit by a later process". The generator always hands back the
        // same, already-existing name, so every attempt collides.
        let dir = tmpdir();
        let dest = dir.path().join("dest.parquet");
        fs::write(&dest, "USER DATA").unwrap();
        let squatted = dir.path().join(".squatted-pq-tmp-0.parquet");
        fs::write(&squatted, "litter").unwrap();

        let mut writer_ran = false;
        let res: std::result::Result<(), std::io::Error> = with_atomic_output_named(
            dest.to_str().unwrap(),
            |_| Some(squatted.clone()),
            |p| {
                writer_ran = true;
                fs::write(p, b"clobbered")?;
                Ok(())
            },
        );

        assert!(res.is_err(), "collision must fail loudly, not fall back");
        assert!(
            !writer_ran,
            "the writer was handed a path despite the failure"
        );
        assert_eq!(
            fs::read_to_string(&dest).unwrap(),
            "USER DATA",
            "destination was replaced after a staging collision"
        );
        assert_eq!(fs::read_to_string(&squatted).unwrap(), "litter");
    }

    #[test]
    fn staging_collision_is_retried_with_a_fresh_name() {
        // The generator collides once, then yields a usable name. The write
        // must go through: retry, not surrender.
        let dir = tmpdir();
        let dest = dir.path().join("dest.txt");
        fs::write(&dest, "original").unwrap();
        let taken = dir.path().join(".taken-pq-tmp");
        fs::write(&taken, "litter").unwrap();

        let mut calls = 0;
        with_atomic_output_named::<_, std::io::Error, _, _>(
            dest.to_str().unwrap(),
            |p| {
                calls += 1;
                if calls == 1 {
                    Some(taken.clone())
                } else {
                    staging_path(p)
                }
            },
            |p| {
                fs::write(p, b"new")?;
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(fs::read_to_string(&dest).unwrap(), "new");
    }

    #[test]
    fn unwritable_destination_is_not_replaced() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tmpdir();
        let dest = dir.path().join("precious.txt");
        fs::write(&dest, "PRECIOUS").unwrap();
        fs::set_permissions(&dest, fs::Permissions::from_mode(0o444)).unwrap();

        let mut writer_ran = false;
        let res: std::result::Result<(), std::io::Error> =
            with_atomic_output(dest.to_str().unwrap(), |p| {
                writer_ran = true;
                fs::write(p, b"clobbered")?;
                Ok(())
            });

        fs::set_permissions(&dest, fs::Permissions::from_mode(0o644)).unwrap();
        assert!(res.is_err(), "a read-only destination was replaced");
        assert!(!writer_ran);
        assert_eq!(fs::read_to_string(&dest).unwrap(), "PRECIOUS");
    }

    #[test]
    fn unwritable_parent_directory_is_not_worked_around() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tmpdir();
        let dest = dir.path().join("dest.txt");
        fs::write(&dest, "USER DATA").unwrap();
        fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o555)).unwrap();

        let res: std::result::Result<(), std::io::Error> =
            with_atomic_output(dest.to_str().unwrap(), |p| {
                fs::write(p, b"clobbered")?;
                Ok(())
            });

        fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o755)).unwrap();
        assert!(
            res.is_err(),
            "a read-only directory must not be worked around by writing in place"
        );
        assert_eq!(fs::read_to_string(&dest).unwrap(), "USER DATA");
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
    fn dangling_symlink_destination_is_followed_not_replaced() {
        let dir = tmpdir();
        let target = dir.path().join("target.txt");
        let link = dir.path().join("link.txt");
        std::os::unix::fs::symlink(&target, &link).unwrap();

        with_atomic_output::<_, std::io::Error, _>(link.to_str().unwrap(), |p| {
            fs::write(p, b"new")?;
            Ok(())
        })
        .unwrap();

        assert!(
            fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink(),
            "the dangling symlink was replaced by a regular file"
        );
        assert_eq!(fs::read_to_string(&target).unwrap(), "new");
    }

    #[test]
    fn relative_symlink_target_resolves_against_the_link() {
        let dir = tmpdir();
        let real = dir.path().join("real.txt");
        fs::write(&real, "original").unwrap();
        let link = dir.path().join("link.txt");
        std::os::unix::fs::symlink("real.txt", &link).unwrap();

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
