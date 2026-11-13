//! Runs an agent CLI that streams JSON events on stdout, with a hard timeout.

use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};
use tokio::process::Command;

#[derive(Debug, PartialEq)]
pub struct RunResult {
    /// None if it was killed.
    pub code: Option<i32>,
    pub stderr: String,
    pub timed_out: bool,
}

/// Runs [cmd] in [cwd], calling [on_event] with each JSON line it prints (other lines are skipped).
/// Kills it after [timeout].
pub async fn run_json_events(cmd: &[String], cwd: &Path, timeout: Duration, on_event: &mut (dyn FnMut(&Value) + Send))
                             -> std::io::Result<RunResult> {
    let mut child = Command::new(&cmd[0])
        .args(&cmd[1..])
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;
    // Read alongside stdout, so a chatty stderr can't block the agent.
    let mut stderr = child.stderr.take().expect("its stderr");
    let stderr = tokio::spawn(async move {
        let mut text = String::new();
        let _ = stderr.read_to_string(&mut text).await;
        text
    });
    let mut lines = BufReader::new(child.stdout.take().expect("its stdout")).lines();
    let read = async {
        while let Some(line) = lines.next_line().await? {
            let line = line.trim();
            if let Ok(event) = serde_json::from_str::<Value>(line) {
                log::debug!("STREAM raw: {}", line.chars().take(500).collect::<String>());
                on_event(&event);
            }
        }
        std::io::Result::Ok(())
    };
    let timed_out = tokio::time::timeout(timeout, read).await.is_err();
    if timed_out {
        child.start_kill()?;
    }
    let status = child.wait().await?;
    // What it left running (a tool it started) could keep stderr open: don't wait for that.
    let stderr = tokio::time::timeout(Duration::from_secs(1), stderr).await.ok().and_then(Result::ok).unwrap_or_default();
    Ok(RunResult { code: status.code(), stderr: stderr.trim().to_string(), timed_out })
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn run(script: &str, timeout: Duration) -> (Vec<Value>, RunResult) {
        let mut events = Vec::new();
        let cmd = ["sh".to_string(), "-c".to_string(), script.to_string()];
        let result = run_json_events(&cmd, Path::new("/"), timeout, &mut |e| events.push(e.clone())).await.unwrap();
        (events, result)
    }

    #[tokio::test]
    async fn each_json_line_is_an_event() {
        let (events, result) = run(r#"echo '{"type": "a"}'; echo 'not json'; echo; echo '{"type": "b"}'; echo oops >&2; exit 3"#,
                                   Duration::from_secs(10)).await;

        assert_eq!(events, [serde_json::json!({"type": "a"}), serde_json::json!({"type": "b"})]);
        assert_eq!(result, RunResult { code: Some(3), stderr: "oops".into(), timed_out: false });
    }

    #[tokio::test]
    async fn a_hung_agent_is_killed_after_the_timeout() {
        let (events, result) = run(r#"echo '{"type": "a"}'; exec sleep 300"#, Duration::from_millis(300)).await;

        assert_eq!(events.len(), 1);
        assert!(result.timed_out && result.code.is_none());
    }

    #[tokio::test]
    async fn it_runs_where_its_told() {
        let dir = tempfile::tempdir().unwrap();
        let mut events = Vec::new();
        let cmd = ["sh".to_string(), "-c".to_string(), r#"printf '{"cwd": "%s"}\n' "$PWD""#.to_string()];

        run_json_events(&cmd, dir.path(), Duration::from_secs(10), &mut |e| events.push(e.clone())).await.unwrap();

        assert_eq!(events[0]["cwd"], dir.path().to_str().unwrap());
    }
}
