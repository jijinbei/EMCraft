//! The window's keys, in one table: what each does (a command's id, or the window's own action)
//! and its keys as a person presses them, so the bar's buttons, the command tips and the search
//! say the same keys the window answers to. On macOS Ctrl is ⌘.

/// What a key does and its keys (`+` between them), Ctrl written for the platform's command key.
pub const KEYS: &[(&str, &str)] = &[
    ("file.new", "Ctrl+N"),
    ("file.open", "Ctrl+O"),
    ("file.save", "Ctrl+S"),
    ("file.save_as", "Ctrl+Shift+S"),
    ("edit.undo", "Ctrl+Z"),
    ("edit.redo", "Ctrl+Y"),
    ("search", "/"),
    ("view.standard", "Ctrl+1–7"),
];

/// The keys of `what` (a command's id or the window's action), each as the platform writes it;
/// none when no key does it.
///
/// ```
/// use emcraft_ui_core::shortcuts::keys;
/// assert_eq!(keys("search"), Some(vec!["/".to_owned()]));
/// assert!(keys("file.save").is_some_and(|k| k.len() == 2 && k[1] == "S"));
/// assert_eq!(keys("mesh.generate"), None);
/// ```
pub fn keys(what: &str) -> Option<Vec<String>> {
    let (_, keys) = KEYS.iter().find(|(id, _)| *id == what)?;
    Some(keys.split('+').map(|k| if k == "Ctrl" { COMMAND.to_owned() } else { k.to_owned() }).collect())
}

/// The command key's name: ⌘ on macOS, Ctrl elsewhere.
const COMMAND: &str = if cfg!(target_os = "macos") { "⌘" } else { "Ctrl" };
