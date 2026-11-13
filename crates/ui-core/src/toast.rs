//! Toasts: what just ended — a long job, a file written — said on a small card in the window's
//! corner for a few seconds, beside the message area's line (SolidCraft's and FieldCraft's
//! toasts). The window takes each away after [`SECONDS`], or when its × is pressed.

/// How long a toast stays (seconds).
pub const SECONDS: u64 = 4;

/// Most toasts shown at once: an older one gives way.
const MOST: usize = 3;

/// Most characters a toast says.
const LONGEST: usize = 160;

/// A toast: its text, whether it says a failure, and its number (to take it away by).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Toast {
    pub text: String,
    pub error: bool,
    pub id: u64,
}

/// The toasts shown, oldest first.
///
/// ```
/// use emcraft_ui_core::toast::Toasts;
/// let toasts = Toasts::default().with("Solved.", false);
/// let id = toasts.newest().unwrap();
/// // The same words again are not said twice while they show.
/// let toasts = toasts.with("Solved.", false);
/// assert_eq!(toasts.shown().len(), 1);
/// // Four in a row keep the last three.
/// let toasts = toasts.with("a", false).with("b", true).with("c", false);
/// assert_eq!(toasts.shown().iter().map(|t| t.text.as_str()).collect::<Vec<_>>(), ["a", "b", "c"]);
/// assert!(toasts.newest() != Some(id) && toasts.without(id).shown().len() == 3);
/// ```
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Toasts {
    shown: Vec<Toast>,
    next: u64,
}

impl Toasts {
    /// With `text` said last (`error`: a failure), unless the newest says it already; the
    /// oldest given way past [`MOST`].
    pub fn with(self, text: &str, error: bool) -> Toasts {
        if text.trim().is_empty() || self.shown.last().is_some_and(|t| t.text == text && t.error == error) {
            return self;
        }
        let toast = Toast { text: emcraft_engine::text::clip(text, LONGEST), error, id: self.next };
        let shown: Vec<Toast> = self.shown.into_iter().chain(std::iter::once(toast)).collect();
        let extra = shown.len().saturating_sub(MOST);
        Toasts { shown: shown.into_iter().skip(extra).collect(), next: self.next.wrapping_add(1) }
    }

    /// Without toast `id`.
    pub fn without(self, id: u64) -> Toasts {
        Toasts { shown: self.shown.into_iter().filter(|t| t.id != id).collect(), ..self }
    }

    /// The toasts, oldest first.
    pub fn shown(&self) -> &[Toast] {
        &self.shown
    }

    /// The newest toast's number.
    pub fn newest(&self) -> Option<u64> {
        self.shown.last().map(|t| t.id)
    }
}

/// Whether command `id`, answered `reply`, wrote a file: a reply naming the `path` written, by
/// a command other than those that open a project.
///
/// ```
/// use emcraft_ui_core::toast::wrote;
/// let reply = serde_json::json!({"path": "/tmp/a.emcraft"});
/// assert!(wrote("file.save", &reply) && wrote("view.field", &reply));
/// assert!(!wrote("file.open", &reply) && !wrote("file.save", &serde_json::json!({})));
/// ```
pub fn wrote(id: &str, reply: &serde_json::Value) -> bool {
    reply.get("path").is_some_and(serde_json::Value::is_string)
        && !matches!(id, "file.open" | "file.sample" | "file.new" | "geometry.import" | "results.import")
}

/// Whether a job running command `id` says how it ended on a toast: not one that opens or
/// begins a project (the window shows it).
///
/// ```
/// use emcraft_ui_core::toast::ends_aloud;
/// assert!(ends_aloud("solve.frequency") && ends_aloud("mesh.generate"));
/// assert!(!ends_aloud("file.sample") && !ends_aloud("file.open"));
/// ```
pub fn ends_aloud(id: &str) -> bool {
    !matches!(id, "file.open" | "file.sample" | "file.new" | "pcb.sample")
}
