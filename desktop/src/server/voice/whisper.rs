//! Speech to text with whisper.cpp, a run of its CLI for each recording: the model takes memory only
//! while it's transcribing.

use std::path::{Path, PathBuf};
use std::process::Stdio;

use tokio::process::Command;

use crate::server::BoxFuture;

pub trait Transcriber: Send + Sync {
    /// What's said in the recording at [audio] (WAV, or anything whisper.cpp reads).
    fn transcribe<'a>(&'a self, audio: &'a Path) -> BoxFuture<'a, Result<String, String>>;
}

pub struct Whisper {
    pub binary: PathBuf,
    pub model: PathBuf,
}

/// Most of the cores: more than about 8 doesn't make it faster.
fn threads() -> usize {
    std::thread::available_parallelism().map_or(4, usize::from).min(8)
}

impl Transcriber for Whisper {
    fn transcribe<'a>(&'a self, audio: &'a Path) -> BoxFuture<'a, Result<String, String>> {
        Box::pin(async move {
            let output = Command::new(&self.binary)
                .arg("-m").arg(&self.model)
                .arg("-f").arg(audio)
                .args(["-l", "en", "-t", &threads().to_string(), "--no-timestamps", "--no-prints"])
                .stdin(Stdio::null())
                .kill_on_drop(true)
                .output()
                .await
                .map_err(|e| format!("can't run {}: {e}", self.binary.display()))?;
            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                return Err(stderr.lines().rfind(|line| !line.trim().is_empty()).unwrap_or("whisper failed").trim().to_string());
            }
            let text = String::from_utf8_lossy(&output.stdout);
            Ok(text.split_whitespace().collect::<Vec<_>>().join(" "))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::agent::tests::fake;

    fn whisper(dir: &tempfile::TempDir) -> Whisper {
        Whisper { binary: fake("whisper-cli").into(), model: dir.path().join("model.bin") }
    }

    #[tokio::test]
    async fn the_recording_is_transcribed_in_english_as_one_line() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("transcript.txt"), "\n Play some\n  music. \n").unwrap();

        let text = whisper(&dir).transcribe(Path::new("/recordings/hotkey.wav")).await;

        assert_eq!(text.as_deref(), Ok("Play some music."));
        let args = std::fs::read_to_string(dir.path().join("args.txt")).unwrap();
        assert!(args.contains(&format!("-m {}", dir.path().join("model.bin").display())), "{args}");
        assert!(args.contains("-f /recordings/hotkey.wav") && args.contains("-l en"), "{args}");
    }

    #[tokio::test]
    async fn a_recording_it_cant_read_says_why() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("fail"), "").unwrap();

        let text = whisper(&dir).transcribe(Path::new("/recordings/garbage.wav")).await;

        assert_eq!(text, Err("error: failed to read audio file '/recordings/garbage.wav'".into()));
    }

    #[tokio::test]
    async fn without_whisper_it_says_so() {
        let dir = tempfile::tempdir().unwrap();
        let whisper = Whisper { binary: "/nowhere/whisper-cli".into(), model: dir.path().join("model.bin") };

        assert!(whisper.transcribe(Path::new("a.wav")).await.unwrap_err().starts_with("can't run /nowhere/whisper-cli"));
    }
}
