use clap::{Parser, Subcommand, ValueEnum};

#[derive(Parser)]
#[command(
    name = "pq",
    about = "A Parquet Swiss Army Knife — inspect, query, transform, and view Parquet files",
    version,
    after_help = "Examples:\n  pq data.parquet                              # open in TUI viewer\n  pq info data.parquet\n  pq cat data.parquet --limit 100\n  pq sql \"SELECT count(*) FROM 'data.parquet'\"\n  pq jq data.parquet '.name'"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,

    /// Force output format
    #[arg(short = 'O', long = "output-format", global = true)]
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
    /// Display file summary (size, rows, schema, compression, metadata)
    Info {
        /// Parquet file path
        file: String,
    },

    /// Display schema (tree, json-schema, arrow, ddl)
    Schema {
        /// Parquet file path
        file: String,

        /// Schema output format
        #[arg(long, default_value = "tree")]
        format: SchemaFormat,
    },

    /// Display column statistics (min, max, nulls, distinct)
    Stats {
        /// Parquet file path
        file: String,
    },

    /// Display physical layout (row groups, column chunks, pages)
    Layout {
        /// Parquet file path
        file: String,
    },

    /// Dump rows from a Parquet file
    Cat {
        /// Parquet file path
        file: String,

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
    },

    /// Show first N rows (default 10)
    Head {
        /// Parquet file path
        file: String,

        /// Number of rows to show
        #[arg(short = 'n', long, default_value = "10")]
        lines: usize,

        /// Columns to include (comma-separated)
        #[arg(short, long, value_delimiter = ',')]
        columns: Option<Vec<String>>,
    },

    /// Show last N rows (default 10)
    Tail {
        /// Parquet file path
        file: String,

        /// Number of rows to show
        #[arg(short = 'n', long, default_value = "10")]
        lines: usize,

        /// Columns to include (comma-separated)
        #[arg(short, long, value_delimiter = ',')]
        columns: Option<Vec<String>>,
    },

    /// Show random N rows
    Sample {
        /// Parquet file path
        file: String,

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

    /// Execute SQL query via DataFusion
    Sql {
        /// SQL query (files can be referenced directly in FROM clause)
        query: String,
    },

    /// Interactive TUI data viewer (default when a file is given without a subcommand)
    View {
        /// Parquet file path
        file: String,
    },

    /// Project columns and filter rows into a new Parquet file
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

    /// Convert JSON/CSV to Parquet
    Convert {
        /// Input file (JSON, JSONL, or CSV)
        input: String,

        /// Output Parquet file path
        #[arg(short, long, required = true)]
        output: String,

        /// Input format (auto-detected from extension if not specified)
        #[arg(short, long)]
        format: Option<InputFormatArg>,
    },

    /// Apply jq expressions to Parquet data
    Jq {
        /// Parquet file path
        file: String,

        /// jq filter expression
        filter: String,

        /// Read all rows into an array before filtering
        #[arg(short, long)]
        slurp: bool,

        /// Output raw strings without JSON quoting
        #[arg(short = 'r', long)]
        raw_output: bool,
    },

    /// Machine-readable tool description for AI agents
    Capabilities,

    /// Generate shell completions
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
    Ddl,
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
