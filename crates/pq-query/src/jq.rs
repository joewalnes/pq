use jaq_interpret::{Ctx, FilterT, ParseCtx, RcIter, Val};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum JqError {
    #[error("Failed to parse jq filter: {0}")]
    Parse(String),

    #[error("Failed to execute jq filter: {0}")]
    Execute(String),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
}

/// Apply a jq filter to a sequence of JSON values
pub fn apply_jq_filter(
    filter_str: &str,
    inputs: Vec<serde_json::Value>,
    slurp: bool,
) -> std::result::Result<Vec<serde_json::Value>, JqError> {
    // Parse the filter
    let (main, errs) = jaq_parse::parse(filter_str, jaq_parse::main());
    if !errs.is_empty() {
        return Err(JqError::Parse(
            errs.into_iter()
                .map(|e| format!("{e:?}"))
                .collect::<Vec<_>>()
                .join(", "),
        ));
    }
    let main = main.ok_or_else(|| JqError::Parse("empty filter".to_string()))?;

    // Build context with standard library
    let mut defs = ParseCtx::new(Vec::new());
    defs.insert_natives(jaq_core::core());
    defs.insert_defs(jaq_std::std());

    let filter = defs.compile(main);

    let inputs_to_process: Vec<Val> = if slurp {
        let arr = serde_json::Value::Array(inputs);
        vec![Val::from(arr)]
    } else {
        inputs.into_iter().map(Val::from).collect()
    };

    let mut results = Vec::new();
    let empty_iter = RcIter::new(core::iter::empty());

    for input in inputs_to_process {
        let out = filter.run((Ctx::new([], &empty_iter), input));
        for val_result in out {
            match val_result {
                Ok(val) => {
                    let json: serde_json::Value = val.into();
                    results.push(json);
                }
                Err(e) => return Err(JqError::Execute(format!("{e:?}"))),
            }
        }
    }

    Ok(results)
}
