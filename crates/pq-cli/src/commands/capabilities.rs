use crate::output::Format;

pub fn run(format: Format) -> anyhow::Result<()> {
    let capabilities = serde_json::json!({
        "tool": "pq",
        "version": env!("CARGO_PKG_VERSION"),
        "description": "A Parquet Swiss Army Knife — inspect, query, transform, and view Parquet files",
        "commands": [
            {
                "name": "info",
                "description": "Display file summary (size, rows, schema, compression, metadata)",
                "args": [{"name": "file", "type": "path", "required": true}],
                "output": ["json", "table"]
            },
            {
                "name": "schema",
                "description": "Display schema in various formats (tree, json, json-schema, arrow, ddl)",
                "args": [
                    {"name": "file", "type": "path", "required": true},
                    {"name": "--format", "type": "enum", "values": ["tree", "json", "json-schema", "arrow", "ddl"]}
                ],
                "output": ["json", "text"]
            },
            {
                "name": "stats",
                "description": "Display column statistics (min, max, nulls, distinct)",
                "args": [{"name": "file", "type": "path", "required": true}],
                "output": ["json", "table"]
            },
            {
                "name": "layout",
                "description": "Display physical layout (row groups, column chunks)",
                "args": [{"name": "file", "type": "path", "required": true}],
                "output": ["json", "text"]
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
                "name": "convert",
                "description": "Convert JSON/CSV to Parquet",
                "args": [
                    {"name": "input", "type": "path", "required": true},
                    {"name": "--output", "type": "path", "required": true},
                    {"name": "--format", "type": "enum", "values": ["json", "jsonl", "csv"]}
                ]
            }
        ],
        "global_options": [
            {"name": "--output-format", "short": "-O", "type": "enum", "values": ["json", "jsonl", "csv", "table", "plain"]},
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
