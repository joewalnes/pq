use arrow::array::RecordBatch;
use datafusion::prelude::*;
use std::path::Path;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum SqlError {
    #[error("DataFusion error: {0}")]
    DataFusion(#[from] datafusion::error::DataFusionError),

    #[error("No results returned")]
    NoResults,
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
    ctx.register_parquet(table_name, path, ParquetReadOptions::default())
        .await?;

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
    ctx.register_parquet("data", path, ParquetReadOptions::default())
        .await?;

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
        let path = Path::new(path_str);
        let is_parquet = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e == "parquet" || e == "parq" || e == "pq")
            .unwrap_or(false);
        if is_parquet && path.exists() {
            ctx.register_parquet(path_str, path_str, ParquetReadOptions::default())
                .await?;
        }
    }

    Ok(())
}
