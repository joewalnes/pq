use std::path::Path;

use pq_core::source;

/// Resolve a list of file arguments into concrete paths.
/// - URLs are passed through unchanged
/// - Glob patterns are expanded
/// - Plain paths are passed through unchanged
///
/// A literal path that exists on disk always wins over glob interpretation,
/// even when it contains `*`, `?`, or `[`. Without this check, a filename
/// like `data[1].parquet` was silently treated as a glob character class
/// (`[1]` matching the literal character `1`) and, if a *different* file
/// happened to match (e.g. `data1.parquet`), that other file's data was
/// returned at exit 0 — even though the user quoted the path precisely to
/// stop a shell from doing exactly this. Most tools that support both a
/// literal filename and pattern matching (e.g. git pathspecs) resolve this
/// the same way: an existing literal path is never reinterpreted as a
/// pattern out from under the user. See DIARY.md for the fuller rationale,
/// including what happens when *both* a literal match and other glob matches
/// exist.
pub fn resolve_files(inputs: &[String]) -> anyhow::Result<Vec<String>> {
    let mut result = Vec::new();
    for input in inputs {
        if source::is_url(input) || Path::new(input).exists() {
            result.push(input.clone());
        } else if input.contains('*') || input.contains('?') || input.contains('[') {
            let paths: Vec<_> = glob::glob(input)
                .map_err(|e| anyhow::anyhow!("Invalid glob pattern '{}': {}", input, e))?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| anyhow::anyhow!("Glob error: {}", e))?;
            if paths.is_empty() {
                anyhow::bail!("No files matched pattern '{}'", input);
            }
            for p in paths {
                result.push(p.display().to_string());
            }
        } else {
            result.push(input.clone());
        }
    }
    if result.is_empty() {
        anyhow::bail!("No input files specified");
    }
    Ok(result)
}
