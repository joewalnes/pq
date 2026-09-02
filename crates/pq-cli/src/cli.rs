use clap::{Parser, Subcommand, ValueEnum};

/// The one-line description of what `pq` is, shared verbatim between
/// `pq --help` (via clap's `about`) and `pq capabilities` (machine-readable
/// output). Previously hand-duplicated in both places and had drifted (one
/// copy used an em-dash, the other an ASCII hyphen) — keep this the single
/// source so it can't drift again.
pub const TAGLINE: &str =
    "A Parquet Swiss Army Knife - inspect, query, transform, and view Parquet files";

#[derive(Parser)]
#[command(
    name = "pq",
    about = TAGLINE,
    version = env!("PQ_VERSION"),
    after_help = "Examples:\n  pq data.parquet                              # open in TUI viewer\n  pq info data.parquet\n  pq cat data.parquet --limit 100\n  pq sql \"SELECT count(*) FROM 'data.parquet'\"\n  pq jq data.parquet '.name'",
    help_template = "\
{about}

Usage: {usage}

Viewer:
  view          Interactive TUI data viewer (default)

Metadata:
  info          Display file summary (size, rows, schema, compression)
  schema        Display schema (tree, json-schema, arrow, ddl, pyarrow)
  stats         Display column statistics (min, max, nulls, distinct)
  layout        Display physical layout (row groups, pages)
  validate      Validate file integrity

Data:
  cat           Dump rows
  head          Show first N rows
  tail          Show last N rows
  sample        Show random N rows
  count         Fast row count
  grep          Search rows by regex

Query:
  sql           Execute SQL via DataFusion
  jq            Apply jq expressions

Transform:
  select        Project columns
  slice         Extract row range
  merge         Combine files
  split         Split file

I/O:
  import        Import CSV/JSON/JSONL to Parquet
  export        Export Parquet to CSV/JSON/JSONL

{options}

{after-help}
"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,

    /// Output format (table, json, jsonl, csv, plain)
    #[arg(short = 'f', long = "format", global = true)]
    pub output_format: Option<OutputFormat>,

    /// Color output
    #[arg(long, global = true, default_value = "auto")]
    pub color: ColorMode,

    /// Suppress non-essential output
    #[arg(short, long, global = true)]
    pub quiet: bool,

    /// Increase verbosity
    #[arg(short, long, global = true)]
    pub verbose: bool,

    /// Log HTTP requests to stderr (ranges, sizes, timings)
    #[arg(long, global = true, hide = true)]
    pub debug: bool,
}

#[derive(Clone, ValueEnum)]
pub enum OutputFormat {
    Json,
    Jsonl,
    Csv,
    Table,
    Plain,
}

#[derive(Clone, ValueEnum)]
pub enum ColorMode {
    Auto,
    Always,
    Never,
}

#[derive(Subcommand)]
pub enum Command {
    // ── Viewer ───────────────────────────────────────────────────────────
    /// Interactive TUI data viewer (default when a file is given without a subcommand)
    View {
        /// Parquet file path
        file: String,
    },

    // ── Metadata ─────────────────────────────────────────────────────────
    /// Display file summary (size, rows, schema, compression, metadata)
    #[command(arg_required_else_help = true)]
    Info {
        /// Parquet file path(s)
        #[arg(required = true)]
        files: Vec<String>,
    },

    /// Display schema (tree, json-schema, arrow, ddl, pyarrow)
    #[command(
        arg_required_else_help = true,
        long_about = "Display schema in various formats.\n\n\
            Styles:\n\
            \x20 tree        Indented tree (default)\n\
            \x20 json        JSON object\n\
            \x20 json-schema JSON Schema\n\
            \x20 arrow       Arrow type names\n\
            \x20 ddl         PostgreSQL-compatible CREATE TABLE\n\
            \x20 pyarrow     Python PyArrow schema constructor"
    )]
    Schema {
        /// Parquet file path(s)
        #[arg(required = true)]
        files: Vec<String>,

        /// Schema style
        #[arg(short = 's', long = "style", default_value = "tree")]
        style: SchemaFormat,
    },

    /// Display column statistics (min, max, nulls, distinct)
    #[command(arg_required_else_help = true)]
    Stats {
        /// Parquet file path(s)
        #[arg(required = true)]
        files: Vec<String>,

        /// Include data-level statistics (min, max, mean, stddev, distinct, top-K)
        #[arg(long)]
        describe: bool,

        /// Number of top frequent values to show per column (with --describe)
        #[arg(long, default_value = "5")]
        top: usize,

        /// Maximum rows to read for --describe (0 = all rows)
        #[arg(long, default_value = "100000")]
        sample_size: usize,
    },

    /// Display physical layout (row groups, column chunks, pages)
    #[command(arg_required_else_help = true)]
    Layout {
        /// Parquet file path(s)
        #[arg(required = true)]
        files: Vec<String>,
    },

    /// Validate Parquet file integrity (footer, schema, statistics)
    #[command(arg_required_else_help = true)]
    Validate {
        /// Parquet file path(s)
        #[arg(required = true)]
        files: Vec<String>,
    },

    // ── Data ─────────────────────────────────────────────────────────────
    /// Dump rows from a Parquet file
    #[command(arg_required_else_help = true)]
    Cat {
        /// Parquet file path(s)
        #[arg(required = true)]
        files: Vec<String>,

        /// Maximum number of rows to output
        #[arg(short, long)]
        limit: Option<usize>,

        /// Number of rows to skip
        #[arg(short, long)]
        offset: Option<usize>,

        /// Columns to include (comma-separated)
        #[arg(short, long, value_delimiter = ',')]
        columns: Option<Vec<String>>,

        /// SQL WHERE clause to filter rows
        #[arg(short = 'w', long = "where")]
        where_clause: Option<String>,

        /// jq expression to apply to each row
        #[arg(long)]
        jq: Option<String>,

        /// Write output to a file (format auto-detected from extension: .parquet, .json, .jsonl, .csv)
        #[arg(short = 'O', long)]
        output: Option<String>,
    },

    /// Show first N rows (default 10)
    #[command(arg_required_else_help = true)]
    Head {
        /// Parquet file path(s)
        #[arg(required = true)]
        files: Vec<String>,

        /// Number of rows to show
        #[arg(short = 'n', long, default_value = "10")]
        lines: usize,

        /// Columns to include (comma-separated)
        #[arg(short, long, value_delimiter = ',')]
        columns: Option<Vec<String>>,
    },

    /// Show last N rows (default 10)
    #[command(arg_required_else_help = true)]
    Tail {
        /// Parquet file path(s)
        #[arg(required = true)]
        files: Vec<String>,

        /// Number of rows to show
        #[arg(short = 'n', long, default_value = "10")]
        lines: usize,

        /// Columns to include (comma-separated)
        #[arg(short, long, value_delimiter = ',')]
        columns: Option<Vec<String>>,
    },

    /// Show random N rows
    #[command(arg_required_else_help = true)]
    Sample {
        /// Parquet file path(s)
        #[arg(required = true)]
        files: Vec<String>,

        /// Number of rows to sample
        #[arg(short = 'n', long, default_value = "10")]
        lines: usize,

        /// Random seed for reproducibility
        #[arg(long)]
        seed: Option<u64>,

        /// Columns to include (comma-separated)
        #[arg(short, long, value_delimiter = ',')]
        columns: Option<Vec<String>>,
    },

    /// Fast row count (metadata-only when possible)
    Count {
        /// Parquet file paths
        files: Vec<String>,
    },

    /// Search rows matching a regex pattern across all columns
    #[command(
        arg_required_else_help = true,
        long_about = "Search rows matching a regex pattern across all columns.\n\n\
            By default, searches all string-representable columns. Use -c to restrict\n\
            to specific columns. Returns matching rows in the output format.\n\n\
            Examples:\n\
            \x20 pq grep data.parquet 'error|warn'           # regex across all columns\n\
            \x20 pq grep data.parquet 'alice' -i             # case-insensitive\n\
            \x20 pq grep data.parquet '404' -c status,code   # search specific columns\n\
            \x20 pq grep data.parquet 'timeout' --limit 10   # first 10 matches"
    )]
    Grep {
        /// Parquet file path(s)
        #[arg(required = true)]
        files: Vec<String>,

        /// Regex pattern to search for
        pattern: String,

        /// Columns to search (comma-separated; default: all)
        #[arg(short, long, value_delimiter = ',')]
        columns: Option<Vec<String>>,

        /// Maximum number of matching rows to return
        #[arg(short, long)]
        limit: Option<usize>,

        /// Case-insensitive matching
        #[arg(short, long)]
        ignore_case: bool,
    },

    // ── Query ────────────────────────────────────────────────────────────
    /// Execute SQL query via DataFusion
    #[command(
        long_about = "Execute SQL queries on Parquet files using Apache DataFusion.\n\n\
            Files are referenced directly in the FROM clause using single-quoted paths.\n\
            Glob patterns (e.g., 'logs/*.parquet') are supported.\n\n\
            Examples:\n\
            \x20 pq sql \"SELECT * FROM 'data.parquet' LIMIT 10\"\n\
            \x20 pq sql \"SELECT city, count(*) FROM 'data.parquet' GROUP BY city\"\n\
            \x20 pq sql \"SELECT a.id, b.name FROM 'a.parquet' a JOIN 'b.parquet' b ON a.id = b.id\"\n\
            \x20 pq sql \"SELECT * FROM 'logs/*.parquet' WHERE level = 'ERROR'\"\n\n\
            SQL reference: https://datafusion.apache.org/user-guide/sql/index.html"
    )]
    Sql {
        /// SQL query (files can be referenced directly in FROM clause)
        query: Option<String>,

        /// Write output to a file (format auto-detected from extension: .parquet, .json, .jsonl, .csv)
        #[arg(short, long)]
        output: Option<String>,
    },

    /// Apply jq expressions to Parquet data
    #[command(
        arg_required_else_help = true,
        long_about = "Apply jq expressions to Parquet data.\n\n\
            Each row is processed as a JSON object. Use --slurp to collect all rows\n\
            into an array first.\n\n\
            Examples:\n\
            \x20 pq jq data.parquet '.name'                           # extract field\n\
            \x20 pq jq data.parquet '{name, age}' -r                  # construct objects\n\
            \x20 pq jq data.parquet 'select(.age > 30)'               # filter rows\n\
            \x20 pq jq data.parquet '[.orders[].price] | add'         # nested aggregation\n\
            \x20 pq jq data.parquet 'group_by(.city) | map({city: .[0].city, n: length})' -s\n\n\
            jq reference: https://jqlang.github.io/jq/manual/"
    )]
    Jq {
        /// Parquet file path(s)
        #[arg(required = true)]
        files: Vec<String>,

        /// jq filter expression
        filter: String,

        /// Read all rows into an array before filtering
        #[arg(short, long)]
        slurp: bool,

        /// Output raw strings without JSON quoting
        #[arg(short = 'r', long)]
        raw_output: bool,

        /// Write output to a file (format auto-detected from extension: .parquet, .json, .jsonl, .csv)
        #[arg(short, long)]
        output: Option<String>,
    },

    // ── Transform ────────────────────────────────────────────────────────
    /// Project columns into a new Parquet file
    #[command(
        arg_required_else_help = true,
        long_about = "Project columns into a new Parquet file.\n\n\
            Examples:\n\
            \x20 pq select data.parquet -c id,name -o subset.parquet\n\
            \x20 pq select data.parquet -c 'id,name,address' -o subset.parquet"
    )]
    Select {
        /// Input Parquet file
        file: String,

        /// Columns to include (comma-separated)
        #[arg(short, long, value_delimiter = ',', required = true)]
        columns: Vec<String>,

        /// Output file path
        #[arg(short, long, required = true)]
        output: String,
    },

    /// Extract a row range into a new Parquet file
    #[command(arg_required_else_help = true)]
    Slice {
        /// Input Parquet file
        file: String,

        /// Start offset
        #[arg(long, default_value = "0")]
        offset: usize,

        /// Number of rows to extract
        #[arg(long)]
        limit: usize,

        /// Output file path
        #[arg(short, long, required = true)]
        output: String,
    },

    /// Combine multiple Parquet files into one
    #[command(arg_required_else_help = true)]
    Merge {
        /// Input Parquet files
        files: Vec<String>,

        /// Output file path
        #[arg(short, long, required = true)]
        output: String,

        /// Schema reconciliation mode
        #[arg(long, default_value = "strict")]
        schema_mode: SchemaModeArg,
    },

    /// Split a Parquet file into multiple files
    #[command(arg_required_else_help = true)]
    Split {
        /// Input Parquet file
        file: String,

        /// Number of rows per output file
        #[arg(long)]
        rows: Option<usize>,

        /// Column(s) to partition by (comma-separated, Hive-style output)
        #[arg(long, value_delimiter = ',')]
        partition_by: Option<Vec<String>>,

        /// Output directory
        #[arg(short, long, required = true)]
        output: String,
    },

    // ── I/O ──────────────────────────────────────────────────────────────
    /// Import CSV/JSON/JSONL into Parquet format
    #[command(arg_required_else_help = true)]
    Import {
        /// Input file (JSON, JSONL, or CSV)
        input: String,

        /// Output Parquet file path
        #[arg(short, long, required = true)]
        output: String,

        /// Input format (auto-detected from extension if not specified)
        #[arg(short = 'F', long = "input-format")]
        input_format: Option<InputFormatArg>,
    },

    /// Export Parquet data to CSV, JSON, or JSONL
    #[command(arg_required_else_help = true)]
    Export {
        /// Parquet file path(s)
        #[arg(required = true)]
        files: Vec<String>,

        /// Output file path (default: stdout)
        #[arg(short, long)]
        output: Option<String>,

        /// Maximum number of rows to export
        #[arg(short, long)]
        limit: Option<usize>,
    },

    // ── Other ────────────────────────────────────────────────────────────
    /// Machine-readable tool description for AI agents
    #[command(hide = true)]
    Capabilities,

    /// Generate shell completions
    ///
    /// Add to your ~/.zshrc (zsh):
    ///   eval "$(pq completions zsh)"
    ///
    /// Add to your ~/.bashrc (bash):
    ///   eval "$(pq completions bash)"
    ///
    /// Add to your ~/.config/fish/config.fish (fish):
    ///   pq completions fish | source
    #[command(hide = true, verbatim_doc_comment)]
    Completions {
        /// Shell to generate completions for
        shell: clap_complete::Shell,
    },
}

#[derive(Clone, ValueEnum)]
pub enum SchemaFormat {
    Tree,
    Json,
    JsonSchema,
    Arrow,
    /// PostgreSQL-compatible DDL (CREATE TABLE)
    Ddl,
    Pyarrow,
}

#[derive(Clone, ValueEnum)]
pub enum SchemaModeArg {
    Strict,
    Union,
    Intersect,
}

#[derive(Clone, ValueEnum)]
pub enum InputFormatArg {
    Json,
    Jsonl,
    Csv,
}

#[derive(Clone, ValueEnum)]
pub enum ExportFormatArg {
    Json,
    Jsonl,
    Csv,
}
