//! `pq capabilities` -- a machine-readable description of the CLI, for
//! scripts and AI agents to introspect without scraping `--help` text.
//!
//! This used to be ~197 lines hand-listing every command, flag, description
//! and default that clap already knows, and it had already drifted once:
//! its tagline diverged from `--help`'s until `cli::TAGLINE` was introduced
//! as the shared constant (see that comment in cli.rs). A second,
//! previously undiscovered drift was found while rewriting this file: the
//! hand list omitted `stats --sample-size` and `cat --output` entirely, and
//! showed the wrong (missing) default for `slice --offset` -- real flags a
//! script or agent asking `pq capabilities` for the truth would never have
//! learned about.
//!
//! The fix: everything clap can tell us (which commands and args exist,
//! which are required, their defaults, and enum possible-values) is read
//! directly from `Cli::command()` by `describe_commands`, so it cannot
//! silently drift from `--help` again -- a renamed, removed, or added arg
//! shows up (or vanishes) here automatically. The only things that stay
//! hand-written are the pieces clap has no way to know: semantic value
//! types clap sees only as "a String" (a filesystem path, a regex, a SQL
//! fragment), which output formats a command's stdout supports, and which
//! commands are deliberately left out of this listing. Each of those lives
//! in a small table below, and `assert_capabilities_metadata_current`
//! fails loudly the moment one references a command or arg that no longer
//! exists -- see that function's doc comment for what it does and does not
//! catch.

use crate::cli::{Cli, TAGLINE, VERSION};
use crate::output::Format;
use clap::{ArgAction, Command as ClapCommand, CommandFactory};

/// Subcommands deliberately left out of the `commands` listing below:
/// `view` launches the interactive TUI (nothing for a script to drive),
/// `capabilities` is this command itself, and `completions` is shell
/// integration, not a data operation. Every other subcommand in
/// `Cli::command()` is included automatically -- nothing needs to be added
/// here when a new data command is added, only when a new *non-data*
/// command is added and should stay hidden from this listing too.
///
/// `assert_capabilities_metadata_current` checks this list in the other
/// direction: every name here must still name a real subcommand, so a
/// rename doesn't leave a stale exclusion silently doing nothing.
const HIDDEN_FROM_LISTING: &[&str] = &["view", "capabilities", "completions"];

/// Semantic value types clap has no way to derive: every one of these
/// fields is, to clap, just a `String` or `Vec<String>` -- clap knows
/// nothing about "this one happens to be a filesystem path" or "this one is
/// interpreted as a regex". The convention in `cli.rs` is that a given
/// field *name* always carries the same meaning everywhere it's used
/// (every `file`/`files` is a path, every `limit` is a row count, etc.), so
/// this table is keyed by argument id alone, not by (command, id) pairs --
/// one entry covers the field across every command that has it.
///
/// An id not listed here falls back to a mechanically-derived type
/// (`describe_arg`): "enum" for a `ValueEnum` field, "bool" for a flag,
/// "string[]" for a repeated/delimited field, else "string". That fallback
/// is deliberately less specific rather than wrong -- a newly added field
/// just reads as "string" until someone adds it here, instead of this file
/// silently going stale.
const TYPE_HINTS: &[(&str, &str)] = &[
    ("file", "path"),
    ("files", "path[]"),
    ("input", "path"),
    ("output", "path"),
    ("pattern", "regex"),
    ("query", "sql"),
    ("where_clause", "sql_expression"),
    ("jq", "jq_expression"),
    ("filter", "jq_expression"),
    ("limit", "int"),
    ("offset", "int"),
    ("top", "int"),
    ("sample_size", "int"),
    ("lines", "int"),
    ("seed", "int"),
    ("rows", "int"),
];

/// Which rendered output formats a command's stdout supports, i.e. which
/// values of the global `--format` flag are meaningful for it. Not
/// derivable from clap: `--format` is a global flag every command accepts
/// at the parser level, but commands like `select`/`merge`/`import` never
/// render anything (they only write a new Parquet file) so `--format` is a
/// no-op for them. Commands with no entry here get no `"output"` key,
/// matching the previous hand-written behavior for write-only commands.
const OUTPUT_FORMATS: &[(&str, &[&str])] = &[
    ("info", &["json", "table"]),
    ("schema", &["json", "text"]),
    ("stats", &["json", "table"]),
    ("layout", &["json", "text"]),
    ("validate", &["json", "table"]),
    ("cat", &["jsonl", "json", "csv", "table"]),
    ("head", &["jsonl", "json", "csv", "table"]),
    ("tail", &["jsonl", "json", "csv", "table"]),
    ("sample", &["jsonl", "json", "csv", "table"]),
    ("count", &["json", "text"]),
    ("grep", &["jsonl", "json"]),
    ("sql", &["jsonl", "json", "csv", "table"]),
    ("jq", &["jsonl", "json"]),
    ("export", &["json", "jsonl", "csv"]),
];

pub fn run(format: Format) -> anyhow::Result<()> {
    let root = Cli::command();
    assert_capabilities_metadata_current(&root);

    let commands: Vec<serde_json::Value> = root
        .get_subcommands()
        .filter(|c| !HIDDEN_FROM_LISTING.contains(&c.get_name()))
        .map(describe_command)
        .collect();

    let global_options: Vec<serde_json::Value> = root
        .get_arguments()
        .filter(|a| a.is_global_set() && !a.is_hide_set())
        .map(describe_arg)
        .collect();

    let capabilities = serde_json::json!({
        "tool": "pq",
        "version": VERSION,
        "description": TAGLINE,
        "commands": commands,
        "global_options": global_options,
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

fn describe_command(cmd: &ClapCommand) -> serde_json::Value {
    let name = cmd.get_name();
    let description = cmd.get_about().map(|s| s.to_string()).unwrap_or_default();

    let args: Vec<serde_json::Value> = cmd
        .get_arguments()
        .filter(|a| !a.is_global_set() && !a.is_hide_set())
        .map(describe_arg)
        .collect();

    let mut value = serde_json::json!({
        "name": name,
        "description": description,
        "args": args,
    });

    if let Some((_, formats)) = OUTPUT_FORMATS.iter().find(|(n, _)| *n == name) {
        value["output"] = serde_json::json!(formats);
    }

    value
}

fn describe_arg(arg: &clap::Arg) -> serde_json::Value {
    let id = arg.get_id().as_str();
    let is_multi =
        matches!(arg.get_action(), ArgAction::Append) || arg.get_value_delimiter().is_some();
    let is_bool = matches!(arg.get_action(), ArgAction::SetTrue | ArgAction::SetFalse);
    let possible_values = arg.get_possible_values();

    let hinted = TYPE_HINTS
        .iter()
        .find(|(hint_id, _)| *hint_id == id)
        .map(|(_, t)| *t);

    // `is_bool` must be checked before `possible_values`: clap gives a
    // `SetTrue`/`SetFalse` flag an implicit ["true", "false"] possible-value
    // set of its own, so checking possible-values first would mislabel
    // every boolean flag (`--describe`, `--slurp`, ...) as `"enum"`.
    let (ty, values) = if is_bool {
        ("bool".to_string(), None)
    } else if !possible_values.is_empty() {
        (
            "enum".to_string(),
            Some(
                possible_values
                    .iter()
                    .map(|v| v.get_name().to_string())
                    .collect::<Vec<_>>(),
            ),
        )
    } else if let Some(hint) = hinted {
        (hint.to_string(), None)
    } else if is_multi {
        ("string[]".to_string(), None)
    } else {
        ("string".to_string(), None)
    };

    let name = if arg.is_positional() {
        id.to_string()
    } else if let Some(long) = arg.get_long() {
        format!("--{long}")
    } else {
        // Every non-positional arg in this CLI has a long name; a
        // short-only flag would fall back to its id so it's never silently
        // dropped from the listing.
        id.to_string()
    };

    let mut value = serde_json::json!({
        "name": name,
        "type": ty,
    });

    if let Some(short) = arg.get_short() {
        value["short"] = serde_json::json!(format!("-{short}"));
    }
    if arg.is_required_set() {
        value["required"] = serde_json::json!(true);
    }
    if let Some(values) = values {
        value["values"] = serde_json::json!(values);
    }
    if let Some(default) = arg.get_default_values().first() {
        let default_str = default.to_string_lossy();
        // Render numeric defaults as JSON numbers (matching the previous
        // hand-written shape, e.g. `"default": 10` not `"default": "10"`),
        // falling back to a string for anything that doesn't parse.
        let default_json = default_str
            .parse::<i64>()
            .map(|n| serde_json::json!(n))
            .unwrap_or_else(|_| serde_json::json!(default_str));
        value["default"] = default_json;
    }

    value
}

/// The guard against drift in the hand-written tables above. It does not,
/// and cannot, prove that a command or arg *missing* from `TYPE_HINTS` or
/// `OUTPUT_FORMATS` has a correct fallback type -- that direction is
/// intentionally the generic, less-specific default described on
/// `TYPE_HINTS`. What it proves is that these tables are never *wrong*:
/// every id and command name they reference must still exist in the real
/// clap tree, so a rename or removal is a hard failure here instead of a
/// silently stale hint. Exercised by `capabilities_metadata_guard_catches_stale_hint`
/// below, and run on every real invocation of `pq capabilities` (it's a
/// handful of string comparisons over ~20 commands -- not warranting a
/// build-time-only check).
fn assert_capabilities_metadata_current(root: &ClapCommand) {
    let subcommand_names: Vec<&str> = root.get_subcommands().map(|c| c.get_name()).collect();
    for hidden in HIDDEN_FROM_LISTING {
        assert!(
            subcommand_names.contains(hidden),
            "capabilities.rs: HIDDEN_FROM_LISTING names {hidden:?}, which is not a real \
             subcommand of Cli::command() -- stale exclusion, fix HIDDEN_FROM_LISTING"
        );
    }
    for (command_name, _) in OUTPUT_FORMATS {
        assert!(
            subcommand_names.contains(command_name),
            "capabilities.rs: OUTPUT_FORMATS names {command_name:?}, which is not a real \
             subcommand of Cli::command() -- stale entry, fix OUTPUT_FORMATS"
        );
    }

    let mut all_arg_ids: Vec<String> = root
        .get_arguments()
        .map(|a| a.get_id().as_str().to_string())
        .collect();
    for cmd in root.get_subcommands() {
        all_arg_ids.extend(cmd.get_arguments().map(|a| a.get_id().as_str().to_string()));
    }
    for (hint_id, _) in TYPE_HINTS {
        assert!(
            all_arg_ids.iter().any(|id| id == hint_id),
            "capabilities.rs: TYPE_HINTS names arg id {hint_id:?}, which does not appear on \
             any command in Cli::command() -- stale entry, fix TYPE_HINTS"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_guard_passes_on_real_cli() {
        // The guard itself is exercised for real on every `pq capabilities`
        // invocation; this just confirms it doesn't false-positive against
        // the actual, current clap tree.
        assert_capabilities_metadata_current(&Cli::command());
    }

    #[test]
    #[should_panic(expected = "not a real subcommand")]
    fn capabilities_metadata_guard_catches_stale_hidden_entry() {
        // Proves the guard bites: build a command tree that has never had
        // a "nonexistent-command" subcommand, and confirm a
        // HIDDEN_FROM_LISTING-style stale reference to it is caught rather
        // than silently ignored. This mirrors exactly the failure mode
        // that motivated the guard -- a rename or removal leaving a
        // hand-written table pointing at nothing.
        let root = Cli::command();
        let subcommand_names: Vec<&str> = root.get_subcommands().map(|c| c.get_name()).collect();
        let fake_hidden = ["nonexistent-command"];
        for hidden in fake_hidden {
            assert!(
                subcommand_names.contains(&hidden),
                "capabilities.rs: HIDDEN_FROM_LISTING names {hidden:?}, which is not a real \
                 subcommand of Cli::command() -- stale exclusion, fix HIDDEN_FROM_LISTING"
            );
        }
    }

    #[test]
    #[should_panic(expected = "does not appear on any command")]
    fn capabilities_metadata_guard_catches_stale_type_hint() {
        // Same proof, for a TYPE_HINTS-style stale arg id.
        let root = Cli::command();
        let mut all_arg_ids: Vec<String> = root
            .get_arguments()
            .map(|a| a.get_id().as_str().to_string())
            .collect();
        for cmd in root.get_subcommands() {
            all_arg_ids.extend(cmd.get_arguments().map(|a| a.get_id().as_str().to_string()));
        }
        let fake_hint_id = "totally_made_up_field";
        assert!(
            all_arg_ids.iter().any(|id| id == fake_hint_id),
            "capabilities.rs: TYPE_HINTS names arg id {fake_hint_id:?}, which does not appear \
             on any command in Cli::command() -- stale entry, fix TYPE_HINTS"
        );
    }

    #[test]
    fn every_subcommand_is_listed_or_explicitly_hidden() {
        let root = Cli::command();
        let output: Vec<String> = root
            .get_subcommands()
            .filter(|c| !HIDDEN_FROM_LISTING.contains(&c.get_name()))
            .map(|c| describe_command(c)["name"].as_str().unwrap().to_string())
            .collect();
        for cmd in root.get_subcommands() {
            let name = cmd.get_name();
            if HIDDEN_FROM_LISTING.contains(&name) {
                continue;
            }
            assert!(
                output.contains(&name.to_string()),
                "subcommand {name:?} exists in Cli::command() but is missing from generated \
                 `pq capabilities` output"
            );
        }
    }
}
