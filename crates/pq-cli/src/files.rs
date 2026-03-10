use pq_core::source;

/// Resolve a list of file arguments into concrete paths.
/// - URLs are passed through unchanged
/// - Glob patterns are expanded
/// - Plain paths are passed through unchanged
pub fn resolve_files(inputs: &[String]) -> anyhow::Result<Vec<String>> {
    let mut result = Vec::new();
    for input in inputs {
        if source::is_url(input) {
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
