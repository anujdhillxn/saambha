//! The chat history: each command and Saambha's reply, and where a new conversation started, one JSON
//! object per line.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// A command and its reply, or where a new conversation started (the agent forgot what came before).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    pub time: f64,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub said: String,
    /// What was spoken.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub reply: String,
    /// Only shown in the chat.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub details: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub new_conversation: bool,
}

impl Entry {
    pub fn exchange(time: f64, said: &str, reply: &str, details: &str) -> Entry {
        Entry { time, said: said.into(), reply: reply.into(), details: details.into(), new_conversation: false }
    }

    pub fn new_conversation(time: f64) -> Entry {
        Entry { time, said: String::new(), reply: String::new(), details: String::new(), new_conversation: true }
    }
}

pub struct ChatHistory {
    path: PathBuf,
    clock: Box<dyn Fn() -> f64 + Send + Sync>,
}

impl ChatHistory {
    pub fn new(path: PathBuf, clock: impl Fn() -> f64 + Send + Sync + 'static) -> ChatHistory {
        ChatHistory { path, clock: Box::new(clock) }
    }

    pub fn add(&self, said: &str, reply: &str, details: &str) {
        self.write(&Entry::exchange((self.clock)(), said, reply, details));
    }

    /// Marks a new conversation, after the last exchange (if any, and if not marked already).
    pub fn start_conversation(&self) {
        if self.recent(1).iter().any(|entry| !entry.new_conversation) {
            self.write(&Entry::new_conversation((self.clock)()));
        }
    }

    /// The last [limit] entries, oldest first.
    pub fn recent(&self, limit: usize) -> Vec<Entry> {
        let Ok(text) = std::fs::read_to_string(&self.path) else { return Vec::new() };
        let lines: Vec<&str> = text.lines().collect();
        lines[lines.len().saturating_sub(limit)..].iter().filter_map(|line| read(line)).collect()
    }

    fn write(&self, entry: &Entry) {
        let written = (|| {
            if let Some(dir) = self.path.parent() {
                std::fs::create_dir_all(dir)?;
            }
            let mut file = OpenOptions::new().create(true).append(true).open(&self.path)?;
            writeln!(file, "{}", serde_json::to_string(entry).expect("an entry as JSON"))?;
            file.sync_all() // on disk before, say, the command that shuts the PC down
        })();
        if let Err(e) = written {
            log::error!("Can't write the chat history to {}: {e}", self.path.display());
        }
    }
}

/// The entry on [line], or None if it can't be read. A crash mid-write can leave a line padded with
/// NUL bytes, ahead of the entry.
fn read(line: &str) -> Option<Entry> {
    match serde_json::from_str(line.trim_matches('\0')) {
        Ok(entry) => Some(entry),
        Err(_) => {
            log::warn!("Leaving out a line of the chat history that can't be read: {:?}", line.chars().take(200).collect::<String>());
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;

    /// A history whose clock is moved on by hand, and the clock.
    fn history(dir: &tempfile::TempDir) -> (ChatHistory, Arc<AtomicU64>) {
        let now = Arc::new(AtomicU64::new(1000));
        let clock = now.clone();
        (ChatHistory::new(dir.path().join("saambha/history.jsonl"), move || clock.load(Ordering::SeqCst) as f64), now)
    }

    #[test]
    fn nothing_said_yet() {
        let dir = tempfile::tempdir().unwrap();

        assert_eq!(history(&dir).0.recent(10), []);
    }

    #[test]
    fn keeps_what_was_said_and_the_reply() {
        let dir = tempfile::tempdir().unwrap();
        let (history, now) = history(&dir);

        history.add("play music", "Playing Enter Hallownest", "");
        now.store(1060, Ordering::SeqCst);
        history.add("say \"hi\"\nthen stop", "Hi", "It said hi.");

        assert_eq!(history.recent(10), [
            Entry::exchange(1000.0, "play music", "Playing Enter Hallownest", ""),
            Entry::exchange(1060.0, "say \"hi\"\nthen stop", "Hi", "It said hi."),
        ]);
    }

    #[test]
    fn reads_the_lines_the_python_server_wrote() {
        let dir = tempfile::tempdir().unwrap();
        let (history, _) = history(&dir);
        std::fs::create_dir_all(dir.path().join("saambha")).unwrap();
        std::fs::write(dir.path().join("saambha/history.jsonl"), concat!(
            r#"{"time": 1.5, "said": "play music", "reply": "Playing"}"#, "\n",
            r#"{"time": 2.5, "said": "merge?", "reply": "Merge.", "details": "More."}"#, "\n",
            r#"{"time": 3.5, "new_conversation": true}"#, "\n",
        )).unwrap();

        assert_eq!(history.recent(10), [
            Entry::exchange(1.5, "play music", "Playing", ""),
            Entry::exchange(2.5, "merge?", "Merge.", "More."),
            Entry::new_conversation(3.5),
        ]);
    }

    #[test]
    fn recent_is_the_last_few() {
        let dir = tempfile::tempdir().unwrap();
        let (history, _) = history(&dir);
        for n in 0..5 {
            history.add(&format!("command {n}"), &format!("reply {n}"), "");
        }

        assert_eq!(history.recent(2).iter().map(|e| e.said.as_str()).collect::<Vec<_>>(), ["command 3", "command 4"]);
    }

    #[test]
    fn a_new_conversation_is_marked_once_and_only_after_something_was_said() {
        let dir = tempfile::tempdir().unwrap();
        let (history, now) = history(&dir);
        history.start_conversation();
        history.add("play music", "Playing", "");
        now.store(1060, Ordering::SeqCst);

        history.start_conversation();
        history.start_conversation();

        assert_eq!(history.recent(10), [Entry::exchange(1000.0, "play music", "Playing", ""), Entry::new_conversation(1060.0)]);
    }

    #[test]
    fn an_entry_a_crash_left_padded_with_nothing_is_read() {
        let dir = tempfile::tempdir().unwrap();
        let (history, _) = history(&dir);
        history.add("play music", "Playing", "");
        let path = dir.path().join("saambha/history.jsonl");
        std::fs::write(&path, "\0".repeat(125) + &std::fs::read_to_string(&path).unwrap()).unwrap();

        assert_eq!(history.recent(10), [Entry::exchange(1000.0, "play music", "Playing", "")]);
    }

    #[test]
    fn a_line_that_cant_be_read_is_left_out() {
        let dir = tempfile::tempdir().unwrap();
        let (history, _) = history(&dir);
        history.add("play music", "Playing", "");
        let path = dir.path().join("saambha/history.jsonl");
        std::fs::write(&path, std::fs::read_to_string(&path).unwrap() + "{\"time\": 1, \"sa\n").unwrap();
        history.add("louder", "Done", "");

        assert_eq!(history.recent(10).iter().map(|e| e.said.as_str()).collect::<Vec<_>>(), ["play music", "louder"]);
    }
}
