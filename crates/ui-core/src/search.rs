//! Command search, as every window offers it (`/`): the commands matching a query, best first,
//! and a query that names a command with its parameters (`command.id {json}`) to run as typed.

use emcraft_engine::CommandSpec;
use serde_json::Value;

/// Longest query (characters), and most rows shown.
pub const MAX_QUERY: usize = 4096;
pub const MAX_RESULTS: usize = 12;

/// The commands matching `query`, best first: the id itself, an id or label beginning with it,
/// then any containing it (the label and description in English and in the window's language).
///
/// ```
/// let list = emcraft_ui_core::search::matches("solve.freq");
/// assert_eq!(list.first().map(|c| c.id), Some("solve.frequency"));
/// assert!(emcraft_ui_core::search::matches("zzzz-nothing").is_empty());
/// ```
pub fn matches(query: &str) -> Vec<&'static CommandSpec> {
    let q = query.trim().to_lowercase();
    let q = q.split_whitespace().next().unwrap_or("");
    let mut scored: Vec<(u8, &'static CommandSpec)> = emcraft_engine::COMMANDS
        .iter()
        .filter_map(|c| {
            let (id, label, shown) = (c.id.to_lowercase(), c.label.to_lowercase(), crate::lang::command_label(c.id).to_lowercase());
            let doc = |d: &str| d.to_lowercase().contains(q);
            let score = if q.is_empty() {
                3
            } else if id == q {
                0
            } else if id.starts_with(q) || label.starts_with(q) || shown.starts_with(q) {
                1
            } else if id.contains(q) || label.contains(q) || shown.contains(q) || doc(c.doc) || doc(crate::lang::command_doc(c.id)) {
                2
            } else {
                return None;
            };
            Some((score, c))
        })
        .collect();
    scored.sort_by_key(|(s, _)| *s);
    scored.into_iter().map(|(_, c)| c).take(MAX_RESULTS).collect()
}

/// A query of the form `command.id {json}`: the command and its parameters, or why the JSON
/// does not read; `None` when the query is not of that form.
///
/// ```
/// use emcraft_ui_core::search::direct;
/// assert_eq!(direct(r#"units.set {"length": "mm"}"#).and_then(Result::ok).map(|(id, _)| id), Some("units.set".to_owned()));
/// assert!(direct("units.set {oops").is_some_and(|r| r.is_err()));
/// assert!(direct("units").is_none());
/// ```
pub fn direct(query: &str) -> Option<Result<(String, Value), String>> {
    let q = query.trim();
    let (id, rest) = q.split_once(char::is_whitespace)?;
    let rest = rest.trim();
    if !rest.starts_with('{') {
        return None;
    }
    emcraft_engine::find_command(id)?;
    Some(serde_json::from_str::<Value>(rest).map(|v| (id.to_owned(), v)).map_err(|e| format!("not JSON: {e}")))
}
