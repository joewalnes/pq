# Engineering Diary

Latest entries first. Record significant decisions, architecture changes, and non-obvious context — the *why* that isn't visible in commits.

---

## 2026-09-01 — Output aliasing: why stage-and-rename beat a path check

Five writers opened their destination with `File::create` before their input readers had produced a single row. When `-o` named an input, the input was gone: `merge`/`select`/`slice` left a 4-byte `PAR1` stub, `export` left zero bytes, and `pq import data.csv -o data.csv` was the worst of them — it truncated the CSV, then read zero rows from the file it had just emptied, wrote a valid empty parquet, printed "Converted 0 rows" and exited 0. Silent, total, and plausible enough that a user would not look twice.

The obvious fix is to refuse when output resolves to an input. It is the wrong fix, for two reasons. First, deciding whether two paths name the same file is not something you can do cheaply and correctly: `./a.parquet` vs `a.parquet`, symlinks, hardlinks, and a case-insensitive APFS all defeat string comparison, and dev+inode comparison — the only test that actually works — is Unix-only and still has to be threaded through every call site as an extra argument. Second, even a correct check only converts data loss into an error message, when what the user asked for is a perfectly reasonable operation.

So the whole class is handled by never truncating the destination in the first place. `pq_transform::output_guard::with_atomic_output` reserves a hidden sibling temp file in the destination's directory, hands that path to the writer, and `rename`s it over the destination once the writer has returned successfully. The input keeps its inode and its bytes for the entire duration of the read, so aliasing needs no detection at all — every disguise is handled by construction. It also buys atomicity for free: a merge that blows up mid-stream on a schema mismatch used to leave the destination as a stub, and now leaves it exactly as it was.

The costs are real but small. Staging resolves symlinked destinations first so the link survives the rename rather than being replaced by a regular file, and copies the destination's permissions onto the temp file so a rename cannot widen them. Destinations that already exist as something other than a regular file — a fifo, `/dev/stdout` — fall back to writing the path directly, because renaming over a character device is not what anyone means. Remote destinations are untouched by this: none of these writers can emit to `s3://` today (they all go through `std::fs`), and a URL-shaped path has no existing parent directory, so it takes the same fallback and produces the same error it always did. That is reasoned, not measured — `make test-integration` was not run.

The helper is a single choke point on purpose. The check being a property of one function rather than a line pasted into five files is what makes the guard a class assertion instead of five call-site assertions.

## 2026-09-01 — Project process setup

Adopted AI-assisted development conventions via `/project-setup`: created `CLAUDE.md` (working rules: test-first, atomic commits, pre-commit `make test`/`make lint`, changelog discipline), this diary, and `CHANGELOG.md`. The existing `TODO.md` stays as the bug/task tracker in its current sectioned format.

Context on where the project stands: core commands, TUI viewer, remote file access (S3/GCS/Azure/HTTPS), docs site with generated CLI reference and demo GIFs, automated binary releases plus npm/PyPI publishing, and hosted example data at `data.pqtool.dev` (Cloudflare R2). Open threads are in `TODO.md` — notably `diff`, `repack`, `sort`, `schema evolve`, and the missing TUI viewer demo GIF on the docs site.
