use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;

use arrow::array::RecordBatch;
use arrow::datatypes::{Field, Schema, SchemaRef};
use datafusion::datasource::MemTable;
use datafusion::prelude::*;
use thiserror::Error;
use url::Url;

use pq_core::reader::ReadOptions;
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
        register_parquet_source(ctx, table_name, location, location).await
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

    register_parquet_source(ctx, table_name, location, location).await
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
            // Canonicalize to resolve relative paths before checking existence
            let path = Path::new(path_str);
            let resolved = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
            if resolved.exists() {
                let resolved_str = resolved.to_str().unwrap_or(path_str);
                register_parquet_source(ctx, path_str, resolved_str, path_str).await?;
            }
        }
    }

    Ok(())
}

fn is_parquet_ref(s: &str) -> bool {
    let lower = s.to_lowercase();
    lower.ends_with(".parquet") || lower.ends_with(".parq") || lower.ends_with(".pq")
}

// ---------------------------------------------------------------------------
// Duplicate top-level column names
// ---------------------------------------------------------------------------
//
// Parquet permits two top-level columns with the same name; SQL has no way to
// address the second one. DataFusion used to resolve that conflict by losing
// it, silently, before pq saw anything.
//
// Measured, not guessed. A probe printed the schema at each hop for a file
// whose two `int64` columns are both named `id`:
//
//   STEP1 arrow-rs file schema fields = ["id", "id"]
//   STEP2 TableProvider schema fields = ["id"]      <-- lost here
//   STEP3 DFSchema fields             = ["id"]
//   STEP7 DFSchema::try_from(file schema) OK fields = ["id", "id"]
//
// So `DFSchema` is *not* the culprit — it holds duplicates happily (STEP7).
// The loss is inside `SessionContext::register_parquet`, which builds a
// `ListingTable` whose schema comes from `ParquetFormat::infer_schema`, and
// that ends in `Schema::try_merge`
// (datafusion-44.0.0/src/datasource/file_format/parquet.rs:363). `try_merge`
// folds every field through `SchemaBuilder::try_merge`, which finds an
// existing field *by name* and merges into it
// (arrow-schema-53/src/schema.rs:98-113) — two `id` fields become one before
// any logical plan, projection or writer exists.
//
// The obvious repair — hand `register_parquet` a pre-disambiguated schema via
// `ParquetReadOptions::schema` — was tried and rejected on evidence. The
// provider then reports the right names, but the file reader still matches
// columns by name, finds no `id:1` in the file, and substitutes nulls:
//
//   STEP5 provider schema = ["id", "id:1"]
//   STEP5 collect ERROR: Column 'id:1' is declared as non-nullable but
//                        contains null values
//
// It only errored because the fixture's fields are non-nullable. On a
// nullable column it would have silently produced a full column of NULLs —
// a fix that looks like it preserves data while destroying it.
//
// What is done instead: when (and only when) a file's top-level names are not
// unique, read it and register it under unique names. The rename is
// deliberately *visible*, with a note on stderr. Restoring the original names
// on the way out would hand back a result set whose column names cannot be
// fed into a query — the exact trap that made the second column unreachable
// — and it is not reversible in general, since a file may legitimately
// contain both `id` and `id_1`.
//
// The cost, stated plainly: a duplicate-named file is read through
// `MemTable`, so it is materialized in memory and gets no predicate or
// projection pushdown. Files with unique names — everything anyone has unless
// they went looking — take the unchanged `register_parquet` path.

/// New names for `schema`'s top-level fields, or `None` if they are already
/// unique. The first occurrence keeps its name; later ones get `_1`, `_2`,
/// ..., skipping any candidate that collides with a name already present in
/// the file, so a rename can never shadow a real column.
fn disambiguated_names(schema: &Schema) -> Option<Vec<String>> {
    let mut taken: HashSet<String> = schema
        .fields()
        .iter()
        .map(|f| f.name().to_string())
        .collect();
    if taken.len() == schema.fields().len() {
        return None;
    }

    let mut seen: HashSet<String> = HashSet::new();
    let mut names = Vec::with_capacity(schema.fields().len());
    for field in schema.fields() {
        let original = field.name();
        if seen.insert(original.to_string()) {
            names.push(original.to_string());
            continue;
        }
        let mut n = 1usize;
        let mut candidate = format!("{original}_{n}");
        // `insert` returns false when the name is already taken, which is
        // exactly the collision case; when it returns true we have both
        // found a free name and reserved it.
        while !taken.insert(candidate.clone()) {
            n += 1;
            candidate = format!("{original}_{n}");
        }
        names.push(candidate);
    }
    Some(names)
}

/// `schema` with its top-level fields renamed to `names`, keeping every other
/// property (type, nullability, per-field and schema metadata) intact.
fn schema_with_names(schema: &Schema, names: &[String]) -> SchemaRef {
    let fields: Vec<Field> = schema
        .fields()
        .iter()
        .zip(names)
        .map(|(f, n)| f.as_ref().clone().with_name(n.clone()))
        .collect();
    Arc::new(Schema::new_with_metadata(fields, schema.metadata().clone()))
}

fn announce_renames(display: &str, schema: &Schema, names: &[String]) {
    let changed: Vec<String> = schema
        .fields()
        .iter()
        .zip(names)
        .enumerate()
        .filter(|(_, (f, n))| f.name() != *n)
        .map(|(i, (f, n))| format!("column {} '{}' -> '{}'", i + 1, f.name(), n))
        .collect();
    eprintln!(
        "note: '{display}' has duplicate column names, which SQL cannot address; \
         renamed for this query: {}. `pq cat` and `pq export` keep the original names.",
        changed.join(", ")
    );
}

/// The file's own Arrow schema, read from metadata only (no data read).
async fn arrow_schema_of(location: &str) -> std::result::Result<SchemaRef, SqlError> {
    if source::is_url(location) {
        let schema = pq_core::async_reader::read_arrow_schema(location)
            .await
            .map_err(|e| SqlError::Other(e.to_string()))?;
        Ok(Arc::new(schema))
    } else {
        let (schema, _rows) = pq_core::reader::read_schema_and_row_count(Path::new(location))
            .map_err(|e| SqlError::Other(e.to_string()))?;
        Ok(schema)
    }
}

/// Whether the duplicate-name check can be applied to `location` at all.
///
/// A local *directory* is a legitimate DataFusion table: `ListingTable` reads
/// every parquet file under it and merges their schemas. Reading such a path
/// as a single parquet file fails with "Is a directory (os error 21)", so an
/// unconditional check turned `SELECT * FROM 'somedir.parquet'` — which
/// worked before — into an error. Found by driving the fixed binary against a
/// directory and comparing with a pre-fix build; see DIARY.md.
///
/// This is keyed on a structural property (not a regular file), never on the
/// check having failed. An unreadable *file* still propagates its error, as
/// it did before, rather than falling back to the behaviour this code exists
/// to remove. The consequence, stated so it is not mistaken for coverage: a
/// directory whose parquet files carry duplicate column names is still
/// collapsed by DataFusion, exactly as before. Logged in TODO.md.
fn is_checkable(location: &str) -> bool {
    if source::is_url(location) {
        return true;
    }
    std::fs::metadata(location)
        .map(|m| m.is_file())
        .unwrap_or(false)
}

/// Register `read_from` under `table_ref`, disambiguating duplicate top-level
/// column names first if there are any. `display` is the location as the user
/// typed it, used only for diagnostics.
async fn register_parquet_source(
    ctx: &SessionContext,
    table_ref: &str,
    read_from: &str,
    display: &str,
) -> std::result::Result<(), SqlError> {
    if !is_checkable(read_from) {
        ctx.register_parquet(table_ref, read_from, ParquetReadOptions::default())
            .await?;
        return Ok(());
    }

    let schema = arrow_schema_of(read_from).await?;
    let Some(names) = disambiguated_names(&schema) else {
        ctx.register_parquet(table_ref, read_from, ParquetReadOptions::default())
            .await?;
        return Ok(());
    };

    announce_renames(display, &schema, &names);
    let renamed = schema_with_names(&schema, &names);

    let (batches, _) = pq_core::reader::open_batches(read_from, &ReadOptions::default())
        .map_err(|e| SqlError::Other(e.to_string()))?;
    let relabelled = batches
        .into_iter()
        .map(|b| RecordBatch::try_new(renamed.clone(), b.columns().to_vec()))
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|e| {
            SqlError::Other(format!(
                "failed to relabel the duplicate columns of '{display}': {e}"
            ))
        })?;

    let table = MemTable::try_new(renamed, vec![relabelled])?;
    let _ = ctx.register_table(table_ref, Arc::new(table))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use arrow::datatypes::DataType;

    fn schema_of(names: &[&str]) -> Schema {
        Schema::new(
            names
                .iter()
                .map(|n| Field::new(*n, DataType::Int64, false))
                .collect::<Vec<_>>(),
        )
    }

    #[test]
    fn unique_names_are_left_alone() {
        assert_eq!(disambiguated_names(&schema_of(&["a", "b", "c"])), None);
        assert_eq!(disambiguated_names(&schema_of(&[])), None);
        // Case matters: these are distinct Parquet columns and distinct
        // Arrow field names, so nothing is renamed.
        assert_eq!(disambiguated_names(&schema_of(&["id", "ID"])), None);
    }

    #[test]
    fn later_duplicates_are_suffixed_first_keeps_its_name() {
        assert_eq!(
            disambiguated_names(&schema_of(&["id", "id", "id"])),
            Some(vec!["id".into(), "id_1".into(), "id_2".into()])
        );
        assert_eq!(
            disambiguated_names(&schema_of(&["a", "id", "b", "id"])),
            Some(vec!["a".into(), "id".into(), "b".into(), "id_1".into()])
        );
    }

    #[test]
    fn a_generated_name_never_steals_a_real_column() {
        // `id_1` already exists further along in the file, so the renamed
        // duplicate must skip past it rather than shadow it.
        assert_eq!(
            disambiguated_names(&schema_of(&["id", "id", "id_1"])),
            Some(vec!["id".into(), "id_2".into(), "id_1".into()])
        );
        // ...and must keep skipping while candidates stay taken.
        assert_eq!(
            disambiguated_names(&schema_of(&["id", "id", "id_1", "id_2"])),
            Some(vec![
                "id".into(),
                "id_3".into(),
                "id_1".into(),
                "id_2".into()
            ])
        );
    }

    #[test]
    fn generated_names_are_unique_among_themselves() {
        // Two independent duplicate groups must not converge on one name.
        let names = disambiguated_names(&schema_of(&["x", "x", "x_1", "x_1"])).unwrap();
        let unique: HashSet<&String> = names.iter().collect();
        assert_eq!(
            unique.len(),
            names.len(),
            "generated names collided: {names:?}"
        );
        assert_eq!(names[0], "x");
        assert_eq!(names[2], "x_1");
    }

    #[test]
    fn renaming_preserves_type_and_nullability() {
        let schema = Schema::new(vec![
            Field::new("id", DataType::Int64, false),
            Field::new("id", DataType::Utf8, true),
        ]);
        let names = disambiguated_names(&schema).unwrap();
        let renamed = schema_with_names(&schema, &names);
        assert_eq!(renamed.field(0).name(), "id");
        assert_eq!(renamed.field(1).name(), "id_1");
        assert_eq!(renamed.field(0).data_type(), &DataType::Int64);
        assert_eq!(renamed.field(1).data_type(), &DataType::Utf8);
        assert!(!renamed.field(0).is_nullable());
        assert!(renamed.field(1).is_nullable());
    }
}
