//! Speaking replies on the desktop (piper | aplay).

use std::io::Write;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};

pub trait Speaker: Send + Sync {
    /// Speaks [text], replacing any narration already playing.
    fn speak(&self, text: &str);
    fn stop(&self);
}

/// Speaks replies on the desktop, one at a time, and says through [on_speaking] when it starts and stops.
///
/// Each narration is a piper | aplay pipeline in its own process group, so stopping it stops both.
pub struct Narrator {
    pipeline: String,
    on_speaking: Arc<dyn Fn(bool) + Send + Sync>,
    /// The process group speaking now, and which narration it is.
    speech: Arc<Mutex<Option<(i32, u64)>>>,
    narrations: Mutex<u64>,
}

fn quote(word: &str) -> String {
    format!("'{}'", word.replace('\'', r"'\''"))
}

impl Narrator {
    pub fn new(piper: &Path, voice: &Path, aplay: &str, on_speaking: impl Fn(bool) + Send + Sync + 'static) -> Narrator {
        let pipeline = format!("{} -m {} --output-raw | {} -r 22050 -f S16_LE -q",
                               quote(&piper.to_string_lossy()), quote(&voice.to_string_lossy()), quote(aplay));
        Narrator { pipeline, on_speaking: Arc::new(on_speaking), speech: Arc::default(), narrations: Mutex::new(0) }
    }

    fn stop_locked(&self, speech: &mut Option<(i32, u64)>) {
        if let Some((group, _)) = speech.take() {
            // SAFETY: a plain syscall; the group may have ended already, which is fine.
            if unsafe { libc::killpg(group, libc::SIGTERM) } == 0 {
                log::info!("Stopped narration");
            }
            (self.on_speaking)(false);
        }
    }
}

impl Speaker for Narrator {
    fn speak(&self, text: &str) {
        let mut speech = self.speech.lock().unwrap();
        self.stop_locked(&mut speech);
        // The text goes in on stdin, so the shell never sees it.
        let child = Command::new("sh").arg("-c").arg(&self.pipeline).stdin(Stdio::piped()).process_group(0).spawn();
        let mut child = match child {
            Ok(child) => child,
            Err(e) => return log::error!("Can't speak: {e}"),
        };
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(text.as_bytes()); // a pipeline that's gone already has nothing to say
        }
        let narration = {
            let mut narrations = self.narrations.lock().unwrap();
            *narrations += 1;
            *narrations
        };
        *speech = Some((child.id() as i32, narration));
        (self.on_speaking)(true);
        let (current, on_speaking) = (self.speech.clone(), self.on_speaking.clone());
        std::thread::spawn(move || {
            let _ = child.wait();
            let mut speech = current.lock().unwrap();
            if speech.is_some_and(|(_, n)| n == narration) {
                // not already stopped or replaced
                *speech = None;
                on_speaking(false);
            }
        });
    }

    fn stop(&self) {
        self.stop_locked(&mut self.speech.lock().unwrap());
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::{Duration, Instant};

    use super::*;
    use crate::server::agent::tests::fake;

    struct Spoken {
        dir: tempfile::TempDir,
        narrator: Narrator,
        speaking: Arc<Mutex<Vec<bool>>>,
    }

    impl Spoken {
        fn new() -> Spoken {
            let dir = tempfile::tempdir().unwrap();
            let speaking = Arc::new(Mutex::new(Vec::new()));
            let said = speaking.clone();
            let narrator = Narrator::new(&PathBuf::from(fake("piper")), &dir.path().join("voice.onnx"), &fake("aplay"),
                                         move |on| said.lock().unwrap().push(on));
            Spoken { dir, narrator, speaking }
        }

        fn text(&self) -> String {
            std::fs::read_to_string(self.dir.path().join("spoken.txt")).unwrap_or_default()
        }

        fn piper(&self) -> i32 {
            std::fs::read_to_string(self.dir.path().join("piper.pid")).unwrap().trim().parse().unwrap()
        }

        fn speaking(&self) -> Vec<bool> {
            self.speaking.lock().unwrap().clone()
        }
    }

    impl Drop for Spoken {
        fn drop(&mut self) {
            self.narrator.stop();
        }
    }

    fn wait_until(condition: impl Fn() -> bool) -> bool {
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if condition() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        false
    }

    fn running(pid: i32) -> bool {
        std::fs::read_to_string(format!("/proc/{pid}/stat")).is_ok_and(|stat| stat.split_whitespace().nth(2) != Some("Z"))
    }

    #[test]
    fn a_reply_is_spoken_verbatim_without_running_shell_syntax() {
        let spoken = Spoken::new();
        let marker = spoken.dir.path().join("pwned");
        let text = format!("It costs $5 `touch {0}` $(touch {0}) \"quoted\" it's\nsecond line", marker.display());

        spoken.narrator.speak(&text);

        assert!(wait_until(|| spoken.text() == text));
        assert!(!marker.exists());
    }

    #[test]
    fn it_says_when_it_starts_and_stops_speaking() {
        let spoken = Spoken::new();

        spoken.narrator.speak("short"); // the fake piper finishes straight away

        assert!(wait_until(|| spoken.speaking() == [true, false]));
    }

    #[test]
    fn a_new_reply_cuts_off_the_one_playing() {
        let spoken = Spoken::new();
        std::fs::write(spoken.dir.path().join("speech_seconds"), "30").unwrap();
        spoken.narrator.speak("first");
        assert!(wait_until(|| spoken.text() == "first"));
        let first = spoken.piper();

        spoken.narrator.speak("second");

        assert!(wait_until(|| !running(first)));
        assert!(wait_until(|| spoken.text() == "second"));
        assert_eq!(spoken.speaking(), [true, false, true]);
    }

    #[test]
    fn stop_ends_the_narration() {
        let spoken = Spoken::new();
        std::fs::write(spoken.dir.path().join("speech_seconds"), "30").unwrap();
        spoken.narrator.speak("hello");
        assert!(wait_until(|| spoken.text() == "hello"));

        spoken.narrator.stop();

        assert!(wait_until(|| !running(spoken.piper())));
        assert_eq!(spoken.speaking(), [true, false]);
    }

    #[test]
    fn stop_with_nothing_playing() {
        let spoken = Spoken::new();

        spoken.narrator.stop();

        assert!(spoken.speaking().is_empty());
    }
}
