use std::any::Any;
use std::collections::HashSet;
use std::path::Path;
use std::sync::{Arc, OnceLock};

use arrow::array::RecordBatch;
use arrow::datatypes::{Field, Schema, SchemaRef};
use async_trait::async_trait;
use datafusion::catalog::{Session, TableProvider};
use datafusion::datasource::listing::ListingTableUrl;
use datafusion::datasource::MemTable;
use datafusion::error::DataFusionError;
use datafusion::logical_expr::{Expr, TableType};
use datafusion::physical_plan::ExecutionPlan;
use datafusion::prelude::*;
use futures::TryStreamExt;
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
// The cost, stated plainly: a duplicate-named file that is scanned is read
// through `MemTable`, so it is materialized in memory and gets no predicate
// or projection pushdown. The read is deferred to the first scan
// (`RenamedDuplicatesTable`), so a path that merely appears as a string
// literal costs nothing beyond its footer. Files with unique names —
// everything anyone has unless they went looking — take the unchanged
// `register_parquet` path.
//
// A local *directory* is a third case and is handled separately: duplicates
// there cannot be repaired, only refused. See
// `reject_directory_with_duplicate_columns`.

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

fn announce_renames(display: &str, original: &Schema, renamed: &Schema) {
    let changed: Vec<String> = original
        .fields()
        .iter()
        .zip(renamed.fields())
        .enumerate()
        .filter(|(_, (a, b))| a.name() != b.name())
        .map(|(i, (a, b))| format!("column {} '{}' -> '{}'", i + 1, a.name(), b.name()))
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

/// What kind of thing a table location is, because the three kinds need three
/// different treatments and conflating them is what produced the two defects
/// this module has now had.
enum Target {
    /// `s3://`, `https://`, ... — a single remote object.
    Remote,
    /// A local regular file.
    File,
    /// A local *directory*: a legitimate DataFusion table, where
    /// `ListingTable` reads every parquet file under it and merges their
    /// schemas. Reading such a path as a single parquet file fails with "Is a
    /// directory (os error 21)", so the first version of the duplicate check
    /// applied it unconditionally and turned `SELECT * FROM 'somedir.parquet'`
    /// — which worked before — into an error.
    Directory,
    /// Anything else, including a path that does not exist: left entirely to
    /// DataFusion, exactly as before this module grew a duplicate check.
    Other,
}

fn classify(location: &str) -> Target {
    if source::is_url(location) {
        return Target::Remote;
    }
    match std::fs::metadata(location) {
        Ok(m) if m.is_file() => Target::File,
        Ok(m) if m.is_dir() => Target::Directory,
        _ => Target::Other,
    }
}

/// The parquet files DataFusion will actually read for the directory table
/// `location`, as absolute-ish object-store paths paired with the store that
/// can read them.
///
/// Obtained from DataFusion's own `ListingTableUrl::list_all_files` rather
/// than a hand-rolled walk, because the file set is not obvious and a
/// hand-rolled walk gets it wrong. Measured against the release binary on a
/// directory containing `top.parquet`, `nested/deep.parquet` and
/// `k=1/hive.parquet`: the query returns rows from `top.parquet` and
/// `k=1/hive.parquet` only. `listing_table_ignore_subdirectory` defaults to
/// **true**, so plain subdirectories are skipped, while segments containing
/// `=` (Hive partitions) are kept. A recursive `read_dir` would have refused
/// on a file DataFusion never opens; a flat one would have missed the Hive
/// partition. Asking DataFusion keeps the two sets identical by construction.
type ListedFiles = (
    Arc<dyn object_store::ObjectStore>,
    Vec<(String, object_store::ObjectMeta)>,
);

async fn listed_parquet_files(
    ctx: &SessionContext,
    location: &str,
) -> std::result::Result<ListedFiles, SqlError> {
    let url = ListingTableUrl::parse(location)?;
    let store = ctx.runtime_env().object_store(&url)?;
    let state = ctx.state();
    let metas: Vec<object_store::ObjectMeta> = url
        .list_all_files(&state, store.as_ref(), ".parquet")
        .await?
        .try_collect()
        .await?;

    let prefix = url.prefix().as_ref().to_string();
    let named = metas
        .into_iter()
        .map(|meta| {
            let full = meta.location.as_ref();
            let rel = full
                .strip_prefix(&prefix)
                .unwrap_or(full)
                .trim_start_matches('/')
                .to_string();
            (rel, meta)
        })
        .collect();
    Ok((store, named))
}

/// Refuse a directory table whose files carry duplicate top-level column
/// names, instead of letting DataFusion answer it wrongly.
///
/// This is a deliberate refusal rather than a repair. Three reasons, in order
/// of weight:
///
/// 1. There is no correct answer to repair *to*. `ListingTable` merges the
///    files of a directory **by name**; a duplicate name has no unambiguous
///    counterpart in a sibling file. Given `[id, id, x]` in one file and
///    `[id, x, id]` in another, "the second `id`" is a different column in
///    each, and any rule pq picked would be a guess presented as data.
/// 2. The only mechanism pq has for preserving duplicates is reading the file
///    and re-registering it under new names (see `RenamedDuplicatesTable`),
///    which materializes it. A directory table is precisely the case that
///    does not fit in memory. Trading silent wrong data for a silent OOM is
///    not a fix.
/// 3. The obvious lazy alternative — hand `ListingTable` a pre-disambiguated
///    schema — was already tried and rejected on evidence for the single-file
///    case (see the block comment above): the parquet reader still matches
///    columns by name and substitutes NULLs. That failure is a property of
///    the reader, so it applies here unchanged.
///
/// pyarrow, used as the independent instrument, refuses the same input:
/// `pyarrow.lib.ArrowInvalid: Can't unify schema with duplicate field names`.
/// pq was answering, with exit 0, where the reference implementation declines.
///
/// Cost, stated plainly: one footer read per file in the directory, on every
/// directory-table registration, including directories that turn out to be
/// fine. That is the same order of work `ListingTable` does to infer the
/// schema, and it is metadata only — no column data is read.
async fn reject_directory_with_duplicate_columns(
    ctx: &SessionContext,
    location: &str,
    display: &str,
) -> std::result::Result<(), SqlError> {
    let (store, files) = listed_parquet_files(ctx, location).await?;

    for (rel, meta) in files {
        let reader = parquet::arrow::async_reader::ParquetObjectReader::new(store.clone(), meta);
        let builder = parquet::arrow::ParquetRecordBatchStreamBuilder::new(reader)
            .await
            .map_err(|e| SqlError::Other(format!("failed to read '{rel}' in '{display}': {e}")))?;
        let schema = builder.schema();
        if disambiguated_names(schema).is_none() {
            continue;
        }
        let mut seen: HashSet<&str> = HashSet::new();
        let mut dups: Vec<&str> = Vec::new();
        for field in schema.fields() {
            if !seen.insert(field.name()) && !dups.contains(&field.name().as_str()) {
                dups.push(field.name());
            }
        }
        return Err(SqlError::Other(format!(
            "'{display}' is a directory of parquet files, and '{rel}' inside it has duplicate \
             top-level column names ({}). DataFusion merges a directory's files by column name, \
             which drops all but one of them and returns the surviving column under the wrong \
             data — silently. pq refuses to answer rather than answer wrongly. Query the file \
             on its own instead (`FROM '{display}/{rel}'`), where pq renames the duplicates so \
             both are addressable, or rewrite the file with unique column names.",
            dups.join(", "),
        )));
    }
    Ok(())
}

/// A parquet file with duplicate top-level column names, registered under
/// unique ones, read **only when the table is actually scanned**.
///
/// Laziness is the point. `register_files_from_query` registers every
/// `.parquet` string literal it finds anywhere in the query, including ones
/// that are only ever compared against — `WHERE src = '.../fat.parquet'`.
/// When this was a plain `MemTable` built at registration time, such a
/// literal was read into memory in full and a rename note was printed about a
/// file the query never selects from. Measured with `/usr/bin/time -l` on a
/// 132 MB file: 168,591,912 bytes peak footprint and 698,284,988 instructions
/// against 6,832,512 / 66,052,831 for a unique-named twin — 24x memory for a
/// file that is not a table in that query.
///
/// The note moves with the read, so it is now printed if and only if the
/// table is scanned. That is also what makes the guard against this
/// measurable from a test: no note means no materialization, by construction.
///
/// Not fixed by this: a table that *is* scanned is still materialized whole,
/// so a duplicate-named file still loses predicate and projection pushdown
/// and still reads everything for `LIMIT 1`. That cost is accepted and
/// recorded in TODO.md; deferring it does not remove it.
struct RenamedDuplicatesTable {
    read_from: String,
    display: String,
    original: SchemaRef,
    renamed: SchemaRef,
    loaded: OnceLock<Arc<MemTable>>,
}

impl std::fmt::Debug for RenamedDuplicatesTable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RenamedDuplicatesTable")
            .field("display", &self.display)
            .field("renamed", &self.renamed)
            .field("materialized", &self.loaded.get().is_some())
            .finish()
    }
}

impl RenamedDuplicatesTable {
    /// Read and relabel the file, once. Concurrent scans may both do the work
    /// but only one result is kept, so every scan sees the same batches.
    fn materialize(&self) -> std::result::Result<Arc<MemTable>, SqlError> {
        if let Some(table) = self.loaded.get() {
            return Ok(table.clone());
        }

        announce_renames(&self.display, &self.original, &self.renamed);

        let (batches, _) = pq_core::reader::open_batches(&self.read_from, &ReadOptions::default())
            .map_err(|e| SqlError::Other(e.to_string()))?;
        let relabelled = batches
            .into_iter()
            .map(|b| RecordBatch::try_new(self.renamed.clone(), b.columns().to_vec()))
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| {
                SqlError::Other(format!(
                    "failed to relabel the duplicate columns of '{}': {e}",
                    self.display
                ))
            })?;

        let table = Arc::new(MemTable::try_new(self.renamed.clone(), vec![relabelled])?);
        let _ = self.loaded.set(table);
        Ok(self
            .loaded
            .get()
            .expect("OnceLock is set above or was already set")
            .clone())
    }
}

#[async_trait]
impl TableProvider for RenamedDuplicatesTable {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn schema(&self) -> SchemaRef {
        self.renamed.clone()
    }

    fn table_type(&self) -> TableType {
        TableType::Base
    }

    async fn scan(
        &self,
        state: &dyn Session,
        projection: Option<&Vec<usize>>,
        filters: &[Expr],
        limit: Option<usize>,
    ) -> datafusion::error::Result<Arc<dyn ExecutionPlan>> {
        let table = self
            .materialize()
            .map_err(|e| DataFusionError::Execution(e.to_string()))?;
        table.scan(state, projection, filters, limit).await
    }
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
    match classify(read_from) {
        // A directory is either safe to hand to DataFusion untouched, or not
        // answerable at all. `reject_directory_with_duplicate_columns` decides
        // which; it never falls back to registering anyway.
        Target::Directory => {
            reject_directory_with_duplicate_columns(ctx, read_from, display).await?;
            ctx.register_parquet(table_ref, read_from, ParquetReadOptions::default())
                .await?;
            return Ok(());
        }
        Target::Other => {
            ctx.register_parquet(table_ref, read_from, ParquetReadOptions::default())
                .await?;
            return Ok(());
        }
        Target::Remote | Target::File => {}
    }

    let schema = arrow_schema_of(read_from).await?;
    let Some(names) = disambiguated_names(&schema) else {
        ctx.register_parquet(table_ref, read_from, ParquetReadOptions::default())
            .await?;
        return Ok(());
    };

    let table = RenamedDuplicatesTable {
        read_from: read_from.to_string(),
        display: display.to_string(),
        renamed: schema_with_names(&schema, &names),
        original: schema,
        loaded: OnceLock::new(),
    };
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
