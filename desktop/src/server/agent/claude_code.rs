//! The Claude Code harness: `claude -p` in the workspace.
//!
//! Claude Code finds the sudo / rm -rf guard in .claude/settings.json because it runs with the workspace
//! as its cwd. It only looks for skills in .claude/skills, so that is kept a link to skills_dir. It reads
//! only those settings, not the user's own (their skills, plugins, MCP servers, memory), which also turn
//! off what Claude Code brings for developers (CLAUDE.md files, its bundled skills and plugins).

use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::server::BoxFuture;
use crate::server::agent::{Assistant, Event, EventHandler, Harness, TIMEOUT_REPLY, why_it_failed};

const TOOLS: &str = "Bash,Skill,WebSearch";

pub struct ClaudeCode {
    pub base: Harness,
}

impl ClaudeCode {
    /// Fails if the workspace's .claude/skills is a real folder rather than the link to [skills_dir].
    pub fn new(base: Harness, skills_dir: &Path) -> Result<ClaudeCode, String> {
        link_skills(&base.workspace, skills_dir)?;
        Ok(ClaudeCode { base })
    }

    async fn ask_locked(&self, text: &str, on_event: EventHandler<'_>) -> String {
        let session = self.base.sessions.load();
        let mut outcome = self.run(text, session.as_deref(), on_event).await;
        if outcome.is_err() && session.is_some() {
            // e.g. the saved session no longer exists (or is another harness's): start fresh rather than fail.
            log::warn!("Resume failed, retrying with a new session");
            self.base.sessions.clear();
            outcome = self.run(text, None, on_event).await;
        }
        outcome.unwrap_or_else(|why| format!("Sorry, something went wrong: {why}"))
    }

    /// The reply, or why it failed.
    async fn run(&self, text: &str, session: Option<&str>, on_event: EventHandler<'_>) -> Result<String, String> {
        let effort = if self.base.effort == "off" { "low" } else { &self.base.effort }; // Claude Code can't turn it off
        let mut cmd: Vec<String> = [
            &self.base.binary, "-p", text,
            "--output-format", "stream-json",
            "--model", &self.base.model,
            "--effort", effort,
            "--tools", TOOLS, // the only ones it has
            "--allowedTools", TOOLS, // and they run without asking
            "--setting-sources", "project",
            "--strict-mcp-config",
            "--system-prompt", &self.base.system_prompt(),
            "--verbose",
        ].map(String::from).into();
        if let Some(session) = session {
            log::info!("Resuming session {session}");
            cmd.extend(["--resume".to_string(), session.to_string()]);
        }

        let mut reply = String::new();
        let mut succeeded = false;
        let run = self.base.run(&cmd, &mut |event: &Value| match event["type"].as_str() {
            Some("assistant") => {
                for block in event["message"]["content"].as_array().into_iter().flatten() {
                    match block["type"].as_str() {
                        Some("text") => {
                            let said = block["text"].as_str().unwrap_or_default().trim();
                            if !said.is_empty() {
                                on_event(Event::Text(said.to_string()));
                            }
                            reply = said.to_string();
                        }
                        Some("tool_use") => {
                            let tool = block["name"].as_str().unwrap_or("tool");
                            on_event(Event::ToolUse(match tool {
                                "Skill" => format!("skill {}", block["input"]["skill"].as_str().unwrap_or_default()).trim().to_string(),
                                _ => tool.to_string(),
                            }));
                        }
                        _ => {}
                    }
                }
            }
            Some("result") if !event["is_error"].as_bool().unwrap_or(false) => {
                succeeded = true;
                let result = event["result"].as_str().unwrap_or_default().trim();
                if !result.is_empty() {
                    reply = result.to_string();
                }
                if let Some(session) = event["session_id"].as_str() {
                    self.base.sessions.save(session);
                }
            }
            _ => {}
        }).await;
        if run.timed_out {
            return Ok(TIMEOUT_REPLY.to_string());
        }
        if succeeded && run.code == Some(0) {
            return Ok(reply);
        }
        Err(why_it_failed("claude", &run)) // e.g. not logged in: claude says why on stderr
    }
}

impl Assistant for ClaudeCode {
    fn ask<'a>(&'a self, text: &'a str, on_event: EventHandler<'a>) -> BoxFuture<'a, String> {
        Box::pin(async move {
            let _one_at_a_time = self.base.lock.lock().await;
            self.ask_locked(text, on_event).await
        })
    }

    fn reset(&self) {
        self.base.sessions.clear();
    }

    fn has_session(&self) -> bool {
        self.base.sessions.load().is_some()
    }

    fn busy(&self) -> bool {
        self.base.busy()
    }
}

/// Points the workspace's .claude/skills at [skills_dir], relative, so the folders can move together.
fn link_skills(workspace: &Path, skills_dir: &Path) -> Result<(), String> {
    let link = workspace.join(".claude/skills");
    let target = relative(skills_dir, &workspace.join(".claude"));
    if std::fs::read_link(&link).is_ok_and(|current| current == target) {
        return Ok(());
    }
    if link.is_dir() && !link.is_symlink() {
        return Err(format!("{} is a folder; move its skills to {}", link.display(), skills_dir.display()));
    }
    let linked = std::fs::create_dir_all(workspace.join(".claude"))
        .and_then(|_| match std::fs::remove_file(&link) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        })
        .and_then(|_| std::os::unix::fs::symlink(&target, &link));
    linked.map_err(|e| format!("Can't link {}: {e}", link.display()))?;
    log::info!("Linked {} -> {}", link.display(), target.display());
    Ok(())
}

/// [path] relative to [from], both absolute.
fn relative(path: &Path, from: &Path) -> PathBuf {
    let (path, from): (Vec<_>, Vec<_>) = (path.components().collect(), from.components().collect());
    let shared = path.iter().zip(&from).take_while(|(a, b)| a == b).count();
    std::iter::repeat_n(std::path::Component::ParentDir, from.len() - shared).chain(path[shared..].iter().copied()).collect()
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use super::*;
    use crate::server::agent::tests::{Fakes, harness};

    fn claude(fakes: &Fakes) -> ClaudeCode {
        ClaudeCode::new(harness(fakes, "claude", "test-model", "off"), &fakes.workspace.join("skills")).unwrap()
    }

    #[tokio::test]
    async fn returns_the_reply_and_streams_events() {
        let fakes = Fakes::default();
        let events = Mutex::new(Vec::new());

        let reply = claude(&fakes).ask("what time is it", &|e| events.lock().unwrap().push(e)).await;

        assert_eq!(reply, "You said: what time is it");
        assert_eq!(*events.lock().unwrap(), [Event::ToolUse("skill spotify".into()), Event::Text("You said: what time is it".into())]);
    }

    #[tokio::test]
    async fn runs_in_the_workspace_with_model_and_system_prompt() {
        let fakes = Fakes::default();

        claude(&fakes).ask("hi", &|_| {}).await;

        let [call] = fakes.calls("claude").try_into().unwrap();
        assert_eq!(call.cwd, fakes.workspace);
        assert_eq!(call.after("--model"), "test-model");
        assert_eq!(call.after("--effort"), "low"); // its least, for "off"
        assert_eq!(call.after("--system-prompt"), "Be Saambha in /workspace.");
        assert_eq!(call.after("--tools"), "Bash,Skill,WebSearch");
        assert_eq!(call.after("--allowedTools"), "Bash,Skill,WebSearch");
    }

    #[tokio::test]
    async fn none_of_the_users_own_claude_code_setup() {
        let fakes = Fakes::default();

        claude(&fakes).ask("hi", &|_| {}).await;

        let [call] = fakes.calls("claude").try_into().unwrap();
        assert_eq!(call.after("--setting-sources"), "project");
        assert!(call.args.iter().any(|a| a == "--strict-mcp-config"));
    }

    #[tokio::test]
    async fn a_follow_up_resumes_the_session() {
        let fakes = Fakes::default();
        let claude = claude(&fakes);

        claude.ask("first", &|_| {}).await;
        claude.ask("second", &|_| {}).await;

        let [first, second] = fakes.calls("claude").try_into().unwrap();
        assert!(!first.args.iter().any(|a| a == "--resume"));
        assert_eq!(second.after("--resume"), "sess-1");
        assert!(claude.has_session());
    }

    #[tokio::test]
    async fn reset_forgets_the_session() {
        let fakes = Fakes::default();
        let claude = claude(&fakes);

        claude.ask("first", &|_| {}).await;
        claude.reset();
        claude.ask("second", &|_| {}).await;

        assert!(!fakes.calls("claude").iter().any(|call| call.args.iter().any(|a| a == "--resume")));
    }

    #[tokio::test]
    async fn a_failed_resume_retries_with_a_new_session() {
        let fakes = Fakes::default();
        let claude = claude(&fakes);
        claude.ask("first", &|_| {}).await;
        fakes.set_mode("claude", "fail-resume");

        let reply = claude.ask("second", &|_| {}).await;

        assert_eq!(reply, "You said: second");
        let [_, failed, retried] = fakes.calls("claude").try_into().unwrap();
        assert!(failed.args.iter().any(|a| a == "--resume"));
        assert!(!retried.args.iter().any(|a| a == "--resume"));
        assert_eq!(claude.base.sessions.load().as_deref(), Some("sess-3")); // the new one, not the one that failed
    }

    #[tokio::test]
    async fn a_failure_says_why() {
        let fakes = Fakes::default();
        let claude = claude(&fakes);
        fakes.set_mode("claude", "fail");

        assert_eq!(claude.ask("hello", &|_| {}).await, "Sorry, something went wrong: Not logged in. Please run /login");
        assert!(!claude.has_session());
    }

    #[tokio::test]
    async fn a_cli_that_isnt_there_says_so() {
        let fakes = Fakes::default();
        let mut base = harness(&fakes, "claude", "m", "off");
        base.binary = "/nowhere/claude".into();
        let claude = ClaudeCode::new(base, &fakes.workspace.join("skills")).unwrap();

        assert!(claude.ask("hello", &|_| {}).await.starts_with("Sorry, something went wrong: can't run /nowhere/claude"));
    }

    #[tokio::test]
    async fn a_hung_claude_is_killed_after_the_timeout() {
        let fakes = Fakes::default();
        let mut base = harness(&fakes, "claude", "m", "off");
        base.timeout = Duration::from_secs(1);
        let claude = ClaudeCode::new(base, &fakes.workspace.join("skills")).unwrap();
        fakes.set_mode("claude", "hang");

        let reply = tokio::time::timeout(Duration::from_secs(5), claude.ask("hello", &|_| {})).await;

        assert_eq!(reply.as_deref(), Ok(TIMEOUT_REPLY));
    }

    #[tokio::test]
    async fn one_command_at_a_time() {
        let fakes = Fakes::default();
        let mut base = harness(&fakes, "claude", "m", "off");
        base.timeout = Duration::from_millis(500);
        let claude = Arc::new(ClaudeCode::new(base, &fakes.workspace.join("skills")).unwrap());
        fakes.set_mode("claude", "hang");

        let asking = tokio::spawn({
            let claude = claude.clone();
            async move { claude.ask("hello", &|_| {}).await }
        });
        tokio::time::sleep(Duration::from_millis(100)).await;

        assert!(claude.busy());
        asking.await.unwrap();
        assert!(!claude.busy());
    }

    #[test]
    fn the_skills_folder_is_linked_where_claude_code_looks() {
        let dir = tempfile::tempdir().unwrap();
        let skills = dir.path().join("elsewhere/skills");

        link_skills(dir.path(), &skills).unwrap();

        let link = dir.path().join(".claude/skills");
        assert_eq!(std::fs::read_link(&link).unwrap(), PathBuf::from("../elsewhere/skills")); // relative, so the folder can move
    }

    #[test]
    fn the_link_follows_a_changed_skills_setting() {
        let dir = tempfile::tempdir().unwrap();
        link_skills(dir.path(), &dir.path().join("old")).unwrap();

        link_skills(dir.path(), &dir.path().join("new")).unwrap();

        assert_eq!(std::fs::read_link(dir.path().join(".claude/skills")).unwrap(), PathBuf::from("../new"));
    }

    #[test]
    fn a_real_skills_folder_is_not_replaced() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".claude/skills")).unwrap();

        assert!(link_skills(dir.path(), &dir.path().join("skills")).unwrap_err().contains("is a folder"));
    }

    #[test]
    fn the_real_workspace_has_the_guard_and_none_of_claude_codes_developer_tooling() {
        let settings: Value = serde_json::from_str(&std::fs::read_to_string(crate::server::agent::tests::real_workspace().join(".claude/settings.json")).unwrap()).unwrap();

        assert!(settings.to_string().contains("sudo is not allowed"));
        assert_eq!((settings["autoMemoryEnabled"].as_bool(), settings["disableBundledSkills"].as_bool()), (Some(false), Some(true)));
        assert!(settings["claudeMdExcludes"].as_array().unwrap().iter().any(|p| p == "**/CLAUDE.md"));
        assert!(settings["enabledPlugins"].as_object().unwrap().values().all(|on| on == false));
    }
}
