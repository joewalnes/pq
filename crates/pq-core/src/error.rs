use thiserror::Error;

#[derive(Error, Debug)]
pub enum PqError {
    // `Display` intentionally renders only this variant's own message, not
    // `source`'s text. `#[source]`/`#[from]` already makes `source()` return
    // the inner error, and every printer that walks the chain (notably
    // `anyhow`'s `{:#}`, used in `pq-cli`'s `main.rs`) appends the source's
    // `Display` itself. Interpolating the source into this message as well
    // used to print it twice, e.g. `Failed to read parquet file 'x': EOF:
    // <msg>: EOF: <msg>`. See DIARY.md for the longer version.
    #[error("Failed to open file '{path}'")]
    FileOpen {
        path: String,
        #[source]
        source: std::io::Error,
    },

    #[error("Failed to read parquet file '{path}'")]
    ParquetRead {
        path: String,
        #[source]
        source: parquet::errors::ParquetError,
    },

    #[error("Parquet error")]
    Parquet(#[from] parquet::errors::ParquetError),

    #[error("Arrow error")]
    Arrow(#[from] arrow::error::ArrowError),

    #[error("IO error")]
    Io(#[from] std::io::Error),

    #[error("JSON error")]
    Json(#[from] serde_json::Error),

    #[error("Column '{name}' not found in schema")]
    ColumnNotFound { name: String },

    #[error("Invalid row range: offset {offset} exceeds total rows {total}")]
    InvalidRowRange { offset: usize, total: usize },

    #[error("Object store error: {0}")]
    ObjectStore(String),

    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, PqError>;

#[cfg(test)]
mod tests {
    use super::*;

    /// Guards the *class* of bug (P2 in TODO.md), not one call site: a
    /// `PqError` variant's own `Display` must never embed the full text of
    /// its `source()`. `pq-cli/src/main.rs` prints top-level errors with
    /// `anyhow`'s `{:#}`, which walks `source()` and appends every level's
    /// `Display` after the top message. A variant whose own message *also*
    /// interpolates the source therefore prints the same sentence twice,
    /// e.g. the bug this guards against:
    /// `Failed to read parquet file 'x': EOF: <msg>: EOF: <msg>`.
    ///
    /// This only inspects one hop (a variant vs. its immediate source), but
    /// that is exactly the shape the historical bug took, and it is
    /// call-site agnostic: it fails for *any* future variant built the same
    /// broken way, not just the ones enumerated in the tests below.
    fn assert_wont_double_under_anyhow_chain(err: &dyn std::error::Error) {
        let own = err.to_string();
        let Some(source) = err.source() else {
            return;
        };
        let source_text = source.to_string();
        // Skip trivially short sources: a real doubled sentence is always
        // the bulk of the message, not a couple of stray characters that
        // might coincidentally recur (e.g. a bare digit).
        if source_text.len() > 3 {
            assert!(
                !own.contains(&source_text),
                "variant's own Display embeds its source's full text verbatim; \
                 anyhow's {{:#}} chain walk would print it a second time:\n  \
                 own:    {own:?}\n  source: {source_text:?}"
            );
        }
    }

    /// End-to-end version of the same guard: renders the error exactly the
    /// way `main.rs` does (`anyhow::Error` + `{:#}`) and asserts the source's
    /// text does not appear twice in the final string a user sees.
    fn assert_anyhow_rendering_has_no_doubled_sentence(err: PqError) {
        let source_text = std::error::Error::source(&err).map(|s| s.to_string());
        let rendered = format!("{:#}", anyhow::Error::new(err));
        if let Some(text) = source_text {
            if text.len() > 3 {
                let occurrences = rendered.matches(&text).count();
                assert_eq!(
                    occurrences, 1,
                    "expected the source's text to appear exactly once in the \
                     anyhow-rendered error, found {occurrences}:\n  rendered: {rendered:?}\n  source:   {text:?}"
                );
            }
        }
    }

    #[test]
    fn file_open_does_not_double() {
        let err = PqError::FileOpen {
            path: "x.parquet".to_string(),
            source: std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "No such file or directory (os error 2)",
            ),
        };
        assert_wont_double_under_anyhow_chain(&err);
        let rendered = err.to_string();
        assert!(
            rendered.contains("x.parquet"),
            "filename context must survive: {rendered:?}"
        );
        assert_anyhow_rendering_has_no_doubled_sentence(PqError::FileOpen {
            path: "x.parquet".to_string(),
            source: std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "No such file or directory (os error 2)",
            ),
        });
    }

    #[test]
    fn parquet_read_does_not_double() {
        let err = PqError::ParquetRead {
            path: "y.parquet".to_string(),
            source: parquet::errors::ParquetError::EOF(
                "file size of 0 is less than footer".to_string(),
            ),
        };
        assert_wont_double_under_anyhow_chain(&err);
        let rendered = err.to_string();
        assert!(
            rendered.contains("y.parquet"),
            "filename context must survive: {rendered:?}"
        );
        assert_anyhow_rendering_has_no_doubled_sentence(PqError::ParquetRead {
            path: "y.parquet".to_string(),
            source: parquet::errors::ParquetError::EOF(
                "file size of 0 is less than footer".to_string(),
            ),
        });
    }

    #[test]
    fn parquet_from_does_not_double() {
        let err: PqError =
            parquet::errors::ParquetError::General("corrupt footer detail".to_string()).into();
        assert_wont_double_under_anyhow_chain(&err);
        assert_anyhow_rendering_has_no_doubled_sentence(
            parquet::errors::ParquetError::General("corrupt footer detail".to_string()).into(),
        );
    }

    #[test]
    fn arrow_from_does_not_double() {
        let err: PqError =
            arrow::error::ArrowError::NotYetImplemented("some arrow detail".to_string()).into();
        assert_wont_double_under_anyhow_chain(&err);
        assert_anyhow_rendering_has_no_doubled_sentence(
            arrow::error::ArrowError::NotYetImplemented("some arrow detail".to_string()).into(),
        );
    }

    #[test]
    fn io_from_does_not_double() {
        let err: PqError = std::io::Error::other("some io detail message").into();
        assert_wont_double_under_anyhow_chain(&err);
        assert_anyhow_rendering_has_no_doubled_sentence(
            std::io::Error::other("some io detail message").into(),
        );
    }

    #[test]
    fn json_from_does_not_double() {
        let json_err = serde_json::from_str::<serde_json::Value>("{not valid").unwrap_err();
        let json_err2 = serde_json::from_str::<serde_json::Value>("{not valid").unwrap_err();
        let err: PqError = json_err.into();
        assert_wont_double_under_anyhow_chain(&err);
        assert_anyhow_rendering_has_no_doubled_sentence(json_err2.into());
    }

    /// Variants with no `source()` (no `#[from]`/`#[source]`) can't double
    /// under a chain walk; this documents that they are out of scope for the
    /// guard above, rather than leaving them silently unverified.
    #[test]
    fn variants_without_a_source_have_none() {
        assert!(std::error::Error::source(&PqError::Other("x".to_string())).is_none());
        assert!(std::error::Error::source(&PqError::ObjectStore("x".to_string())).is_none());
    }
}
