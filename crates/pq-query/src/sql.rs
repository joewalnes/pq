use std::path::Path;

use arrow::array::RecordBatch;
use datafusion::prelude::*;
use thiserror::Error;
use url::Url;

use pq_core::source;

#[derive(Error, Debug)]
pub enum SqlError {
    #[error("DataFusion error: {0}")]
    DataFusion(#[from] datafusion::error::DataFusionError),

    #[error("No results returned")]
    NoResults,

    #[error("{0}")]
    Other(String),
}

pub async fn execute_sql(query: &str) -> std::result::Result<Vec<RecordBatch>, SqlError> {
    let ctx = SessionContext::new();

    // Auto-register parquet files found in the query
    register_files_from_query(&ctx, query).await?;

    let df = ctx.sql(query).await?;
    let batches = df.collect().await?;
    Ok(batches)
}

pub async fn execute_sql_on_file(
    path: &str,
    table_name: &str,
    query: &str,
) -> std::result::Result<Vec<RecordBatch>, SqlError> {
    let ctx = SessionContext::new();
    register_location(&ctx, table_name, path).await?;

    let df = ctx.sql(query).await?;
    let batches = df.collect().await?;
    Ok(batches)
}

/// Build a SQL query from a WHERE clause applied to a file
pub async fn query_with_where(
    path: &str,
    columns: Option<&[String]>,
    where_clause: &str,
    limit: Option<usize>,
    offset: Option<usize>,
) -> std::result::Result<Vec<RecordBatch>, SqlError> {
    let ctx = SessionContext::new();
    register_location(&ctx, "data", path).await?;

    let cols = match columns {
        Some(cols) => cols.join(", "),
        None => "*".to_string(),
    };

    let mut sql = format!("SELECT {cols} FROM data WHERE {where_clause}");

    if let Some(limit) = limit {
        sql.push_str(&format!(" LIMIT {limit}"));
    }
    if let Some(offset) = offset {
        sql.push_str(&format!(" OFFSET {offset}"));
    }

    let df = ctx.sql(&sql).await?;
    let batches = df.collect().await?;
    Ok(batches)
}

/// Register a local path or remote URL as a parquet table in the DataFusion context.
async fn register_location(
    ctx: &SessionContext,
    table_name: &str,
    location: &str,
) -> std::result::Result<(), SqlError> {
    if source::is_url(location) {
        register_remote_parquet(ctx, table_name, location).await
    } else {
        ctx.register_parquet(table_name, location, ParquetReadOptions::default())
            .await?;
        Ok(())
    }
}

/// Register a remote URL as a parquet table, setting up the object store first.
async fn register_remote_parquet(
    ctx: &SessionContext,
    table_name: &str,
    location: &str,
) -> std::result::Result<(), SqlError> {
    let (store, _path) = source::parse_url(location).map_err(|e| SqlError::Other(e.to_string()))?;
    let url = Url::parse(location).map_err(|e| SqlError::Other(e.to_string()))?;

    // Register the object store with DataFusion using the base URL
    let base_url = base_url_for_registration(&url);
    ctx.register_object_store(&base_url, store);

    ctx.register_parquet(table_name, location, ParquetReadOptions::default())
        .await?;
    Ok(())
}

/// Build the base URL that DataFusion uses for object store lookup.
fn base_url_for_registration(url: &Url) -> Url {
    match url.scheme() {
        "s3" => {
            let bucket = url.host_str().unwrap_or("");
            Url::parse(&format!("s3://{bucket}")).unwrap()
        }
        _ => {
            let port_suffix = url.port().map(|p| format!(":{p}")).unwrap_or_default();
            let host = url.host_str().unwrap_or("");
            Url::parse(&format!("{}://{host}{port_suffix}", url.scheme())).unwrap()
        }
    }
}

/// Find file paths in a SQL query and register them as tables.
/// Looks for single-quoted strings that end with .parquet, .parq, or .pq
async fn register_files_from_query(
    ctx: &SessionContext,
    query: &str,
) -> std::result::Result<(), SqlError> {
    let mut in_quote = false;
    let mut current = String::new();
    let mut paths = Vec::new();

    for ch in query.chars() {
        if ch == '\'' {
            if in_quote {
                paths.push(current.clone());
                current.clear();
            }
            in_quote = !in_quote;
        } else if in_quote {
            current.push(ch);
        }
    }

    for path_str in &paths {
        let is_parquet = is_parquet_ref(path_str);
        if !is_parquet {
            continue;
        }

        if source::is_url(path_str) {
            register_remote_parquet(ctx, path_str, path_str).await?;
        } else {
            let path = Path::new(path_str);
            if path.exists() {
                ctx.register_parquet(path_str, path_str, ParquetReadOptions::default())
                    .await?;
            }
        }
    }

    Ok(())
}

fn is_parquet_ref(s: &str) -> bool {
    let lower = s.to_lowercase();
    lower.ends_with(".parquet") || lower.ends_with(".parq") || lower.ends_with(".pq")
}
