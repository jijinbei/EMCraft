//! The window's messages as every window keeps them: what each command did or why it was
//! refused, newest last, a message repeated in a row counted rather than repeated, the oldest let
//! go past [`MAX`].

/// A message: its text, whether it is an error, and how many times in a row it came.
#[derive(Clone, Debug, PartialEq)]
pub struct Line {
    pub text: String,
    pub error: bool,
    pub count: u32,
}

impl Line {
    /// The line as shown: its text, with how many times it came when more than once.
    pub fn shown(&self) -> String {
        if self.count > 1 { format!("{} (×{})", self.text, self.count) } else { self.text.clone() }
    }
}

/// Messages kept, and the longest a message is kept (characters).
pub const MAX: usize = 200;
const LONGEST: usize = 2000;

/// The messages, oldest first.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Log {
    lines: Vec<Line>,
}

impl Log {
    /// The log with `text` said last (`error`: as an error): counted on the last line when it
    /// says the same.
    ///
    /// ```
    /// use emcraft_ui_core::messages::Log;
    /// let log = Log::default().with("Meshed.", false).with("Meshed.", false).with("no port", true);
    /// let shown: Vec<String> = log.lines().iter().map(|l| l.shown()).collect();
    /// assert_eq!(shown, ["Meshed. (×2)", "no port"]);
    /// assert!(log.last().is_some_and(|l| l.error));
    /// ```
    pub fn with(self, text: &str, error: bool) -> Log {
        let text = emcraft_engine::text::clip(text, LONGEST);
        let mut lines = self.lines;
        match lines.last_mut() {
            Some(last) if last.text == text && last.error == error => last.count = last.count.saturating_add(1),
            _ => lines.push(Line { text, error, count: 1 }),
        }
        let extra = lines.len().saturating_sub(MAX);
        lines.drain(..extra);
        Log { lines }
    }

    /// The log with `more` added to its last line's text (a job's time, once it is known).
    pub fn amended(self, more: &str) -> Log {
        let mut lines = self.lines;
        if let Some(last) = lines.last_mut() {
            last.text = emcraft_engine::text::clip(&format!("{}{more}", last.text), LONGEST);
        }
        Log { lines }
    }

    pub fn lines(&self) -> &[Line] {
        &self.lines
    }

    pub fn last(&self) -> Option<&Line> {
        self.lines.last()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_log_keeps_the_newest_and_clips_long_messages() {
        let log = (0..MAX + 5).fold(Log::default(), |log, i| log.with(&i.to_string(), false));
        assert_eq!((log.lines().len(), log.lines()[0].text.as_str()), (MAX, "5"));
        let long = Log::default().with(&"x".repeat(5000), true).amended(" (2 s)");
        assert!(long.last().is_some_and(|l| l.text.chars().count() == LONGEST));
        // A message said again after another starts its own line.
        let log = Log::default().with("a", false).with("b", false).with("a", false);
        assert_eq!(log.lines().len(), 3);
        // The same text as an error is another message.
        assert_eq!(Log::default().with("a", false).with("a", true).lines().len(), 2);
    }
}
