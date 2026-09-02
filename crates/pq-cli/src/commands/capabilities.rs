use crate::cli::{TAGLINE, VERSION};
use crate::output::Format;

pub fn run(format: Format) -> anyhow::Result<()> {
    let capabilities = serde_json::json!({
        "tool": "pq",
        "version": VERSION,
        "description": TAGLINE,
        "commands": [
            {
                "name": "info",
                "description": "Display file summary (size, rows, schema, compression, metadata)",
                "args": [{"name": "file", "type": "path", "required": true}],
                "output": ["json", "table"]
            },
            {
                "name": "schema",
                "description": "Display schema in various formats (tree, json, json-schema, arrow, ddl, pyarrow)",
                "args": [
                    {"name": "file", "type": "path", "required": true},
                    {"name": "--style", "type": "enum", "values": ["tree", "json", "json-schema", "arrow", "ddl", "pyarrow"]}
                ],
                "output": ["json", "text"]
            },
            {
                "name": "stats",
                "description": "Display column statistics (min, max, nulls, distinct); use --describe for data-level stats",
                "args": [
                    {"name": "file", "type": "path", "required": true},
                    {"name": "--describe", "type": "bool"},
                    {"name": "--top", "type": "int", "default": 5}
                ],
                "output": ["json", "table"]
            },
            {
                "name": "layout",
                "description": "Display physical layout (row groups, column chunks)",
                "args": [{"name": "file", "type": "path", "required": true}],
                "output": ["json", "text"]
            },
            {
                "name": "validate",
                "description": "Validate Parquet file integrity (footer, schema, statistics)",
                "args": [{"name": "file", "type": "path", "required": true}],
                "output": ["json", "table"]
            },
            {
                "name": "cat",
                "description": "Dump rows with optional filtering and projection",
                "args": [
                    {"name": "file", "type": "path", "required": true},
                    {"name": "--limit", "type": "int"},
                    {"name": "--offset", "type": "int"},
                    {"name": "--columns", "type": "string[]"},
                    {"name": "--where", "type": "sql_expression"},
                    {"name": "--jq", "type": "jq_expression"}
                ],
                "output": ["jsonl", "json", "csv", "table"]
            },
            {
                "name": "head",
                "description": "Show first N rows",
                "args": [
                    {"name": "file", "type": "path", "required": true},
                    {"name": "-n", "type": "int", "default": 10}
                ],
                "output": ["jsonl", "json", "csv", "table"]
            },
            {
                "name": "tail",
                "description": "Show last N rows",
                "args": [
                    {"name": "file", "type": "path", "required": true},
                    {"name": "-n", "type": "int", "default": 10}
                ],
                "output": ["jsonl", "json", "csv", "table"]
            },
            {
                "name": "sample",
                "description": "Show random N rows",
                "args": [
                    {"name": "file", "type": "path", "required": true},
                    {"name": "-n", "type": "int", "default": 10},
                    {"name": "--seed", "type": "int"}
                ],
                "output": ["jsonl", "json", "csv", "table"]
            },
            {
                "name": "count",
                "description": "Fast row count (metadata-only when possible)",
                "args": [{"name": "files", "type": "path[]", "required": true}],
                "output": ["json", "text"]
            },
            {
                "name": "grep",
                "description": "Search rows matching a regex pattern across all columns",
                "args": [
                    {"name": "file", "type": "path", "required": true},
                    {"name": "pattern", "type": "regex", "required": true},
                    {"name": "--columns", "type": "string[]"},
                    {"name": "--limit", "type": "int"},
                    {"name": "--ignore-case", "type": "bool"}
                ],
                "output": ["jsonl", "json"]
            },
            {
                "name": "sql",
                "description": "Execute SQL query via DataFusion (files referenced in FROM clause)",
                "args": [{"name": "query", "type": "sql", "required": true}],
                "output": ["jsonl", "json", "csv", "table"]
            },
            {
                "name": "jq",
                "description": "Apply jq expressions to Parquet data",
                "args": [
                    {"name": "file", "type": "path", "required": true},
                    {"name": "filter", "type": "jq_expression", "required": true},
                    {"name": "--slurp", "type": "bool"},
                    {"name": "--raw-output", "type": "bool"}
                ],
                "output": ["jsonl", "json"]
            },
            {
                "name": "select",
                "description": "Project columns into a new Parquet file",
                "args": [
                    {"name": "file", "type": "path", "required": true},
                    {"name": "--columns", "type": "string[]", "required": true},
                    {"name": "--output", "type": "path", "required": true}
                ]
            },
            {
                "name": "slice",
                "description": "Extract row range into a new Parquet file",
                "args": [
                    {"name": "file", "type": "path", "required": true},
                    {"name": "--offset", "type": "int"},
                    {"name": "--limit", "type": "int", "required": true},
                    {"name": "--output", "type": "path", "required": true}
                ]
            },
            {
                "name": "merge",
                "description": "Combine multiple Parquet files into one",
                "args": [
                    {"name": "files", "type": "path[]", "required": true},
                    {"name": "--output", "type": "path", "required": true},
                    {"name": "--schema-mode", "type": "enum", "values": ["strict", "union", "intersect"]}
                ]
            },
            {
                "name": "split",
                "description": "Split a Parquet file into multiple files by row count or partition columns",
                "args": [
                    {"name": "file", "type": "path", "required": true},
                    {"name": "--rows", "type": "int"},
                    {"name": "--partition-by", "type": "string[]"},
                    {"name": "--output", "type": "path", "required": true}
                ]
            },
            {
                "name": "import",
                "description": "Import CSV/JSON/JSONL into Parquet format",
                "args": [
                    {"name": "input", "type": "path", "required": true},
                    {"name": "--output", "type": "path", "required": true},
                    {"name": "--input-format", "type": "enum", "values": ["json", "jsonl", "csv"]}
                ]
            },
            {
                "name": "export",
                "description": "Export Parquet data to CSV, JSON, or JSONL (default: stdout)",
                "args": [
                    {"name": "file", "type": "path", "required": true},
                    {"name": "--output", "type": "path"},
                    {"name": "--limit", "type": "int"}
                ]
            }
        ],
        "global_options": [
            {"name": "--format", "short": "-f", "type": "enum", "values": ["json", "jsonl", "csv", "table", "plain"]},
            {"name": "--color", "type": "enum", "values": ["auto", "always", "never"]},
            {"name": "--quiet", "short": "-q", "type": "bool"},
            {"name": "--verbose", "short": "-v", "type": "bool"}
        ],
        "features": {
            "sql_engine": "Apache Arrow DataFusion",
            "jq_engine": "jaq (pure Rust jq)",
            "output_auto_detection": "TTY=table, pipe=jsonl",
            "respects_no_color": true
        }
    });

    let stdout = std::io::stdout();
    let mut writer = stdout.lock();
    crate::output::render_value(&mut writer, &capabilities, format)?;
    Ok(())
}
