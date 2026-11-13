//! The agent harness that runs commands: an agent CLI (Claude Code or pi) in the workspace, one command
//! at a time, continuing its session across commands until it expires or is reset.

pub mod claude_code;
pub mod pi;
pub mod process;
pub mod sessions;

use std::path::PathBuf;
use std::time::Duration;

use serde_json::Value;

use crate::server::BoxFuture;
use process::RunResult;
use sessions::SessionStore;

/// What the agent is doing as it works.
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    /// Something it says, on the way or as its reply.
    Text(String),
    /// A tool it runs, e.g. "Bash" or "skill spotify".
    ToolUse(String),
}

pub type EventHandler<'a> = &'a (dyn Fn(Event) + Send + Sync);

pub trait Assistant: Send + Sync {
    /// Runs [text] as a command: the agent's reply. Tells [on_event] what it's doing as it works.
    fn ask<'a>(&'a self, text: &'a str, on_event: EventHandler<'a>) -> BoxFuture<'a, String>;
    /// Forgets the conversation.
    fn reset(&self);
    fn has_session(&self) -> bool;
    /// Whether a command is running right now.
    fn busy(&self) -> bool;
}

pub const TIMEOUT_REPLY: &str = "Sorry, that took too long. Please try again.";

/// What both harnesses share.
pub struct Harness {
    /// For logs, e.g. "pi".
    pub name: &'static str,
    pub binary: String,
    pub workspace: PathBuf,
    pub system_prompt_file: PathBuf,
    /// The $names the system prompt can use, and their values.
    pub prompt_vars: Vec<(String, String)>,
    pub model: String,
    /// "off", "low", "medium" or "high".
    pub effort: String,
    pub sessions: SessionStore,
    pub timeout: Duration,
    /// One command at a time: concurrent runs would write to the same session.
    pub lock: tokio::sync::Mutex<()>,
}

impl Harness {
    /// The prompt file (read each time, so edits apply at once) with $names filled in.
    pub fn system_prompt(&self) -> String {
        let template = std::fs::read_to_string(&self.system_prompt_file).unwrap_or_else(|e| {
            log::error!("Can't read the system prompt {}: {e}", self.system_prompt_file.display());
            String::new()
        });
        substitute(template.trim(), &self.prompt_vars)
    }

    pub fn busy(&self) -> bool {
        self.lock.try_lock().is_err()
    }

    /// Runs [cmd] in the workspace, passing each JSON event to [handle]. A CLI that can't be run
    /// fails as one that exits saying why.
    pub async fn run(&self, cmd: &[String], handle: &mut (dyn FnMut(&Value) + Send)) -> RunResult {
        let run = process::run_json_events(cmd, &self.workspace, self.timeout, handle).await.unwrap_or_else(|e| RunResult {
            code: None,
            stderr: format!("can't run {}: {e}", self.binary),
            timed_out: false,
        });
        if run.timed_out {
            log::error!("{} timed out after {}s", self.name, self.timeout.as_secs());
        } else if run.code != Some(0) {
            log::error!("{} exited {:?}: {}", self.name, run.code, run.stderr);
        }
        run
    }
}

/// [template] with each $name or ${name} in [vars] filled in, and $$ as $; other $s are left as they are.
pub fn substitute(template: &str, vars: &[(String, String)]) -> String {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(at) = rest.find('$') {
        out.push_str(&rest[..at]);
        rest = &rest[at + 1..];
        if let Some(after) = rest.strip_prefix('$') {
            out.push('$');
            rest = after;
            continue;
        }
        let braced = rest.starts_with('{');
        let name_start = usize::from(braced);
        let name_len = rest[name_start..].find(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).unwrap_or(rest.len() - name_start);
        let name = &rest[name_start..name_start + name_len];
        let end = name_start + name_len + usize::from(braced);
        match vars.iter().find(|(var, _)| var == name) {
            Some((_, value)) if !braced || rest[name_start + name_len..].starts_with('}') => {
                out.push_str(value);
                rest = &rest[end..];
            }
            _ => out.push('$'),
        }
    }
    out.push_str(rest);
    out
}

/// The first line of [stderr] that says something, else that [name] exited with [code].
pub fn why_it_failed(name: &str, run: &RunResult) -> String {
    run.stderr.lines().find(|line| !line.trim().is_empty()).map(str::to_string)
        .unwrap_or_else(|| format!("{name} exited with code {}", run.code.map_or("none".to_string(), |c| c.to_string())))
}

#[cfg(test)]
pub mod tests {
    use std::path::Path;

    use super::*;

    /// The real agent workspace, in the repo.
    pub fn real_workspace() -> PathBuf {
        crate::server::settings::root().parent().unwrap().join("agent")
    }

    /// A fake agent CLI from tests/fakes (see each one's header).
    pub fn fake(name: &str) -> String {
        crate::server::settings::root().join("tests/fakes").join(name).to_string_lossy().into_owned()
    }

    /// A throwaway workspace, with the sample skills in workspace/skills, where the fake CLIs log their
    /// calls and read what to do.
    pub struct Fakes {
        pub dir: tempfile::TempDir,
        pub workspace: PathBuf,
    }

    #[derive(Debug, serde::Deserialize)]
    pub struct Call {
        pub args: Vec<String>,
        pub cwd: PathBuf,
    }

    impl Call {
        /// The argument after [flag].
        pub fn after(&self, flag: &str) -> &str {
            let at = self.args.iter().position(|a| a == flag).unwrap_or_else(|| panic!("no {flag} in {:?}", self.args));
            &self.args[at + 1]
        }
    }

    impl Default for Fakes {
        fn default() -> Fakes {
            let dir = tempfile::tempdir().unwrap();
            let workspace = dir.path().join("workspace");
            copy_dir(&crate::server::settings::root().join("tests/fixtures/skills"), &workspace.join("skills"));
            std::fs::write(dir.path().join("system-prompt.md"), "Be Saambha in $workspace.\n").unwrap();
            Fakes { dir, workspace }
        }
    }

    impl Fakes {

        /// What the fake [cli] does from now on.
        pub fn set_mode(&self, cli: &str, mode: &str) {
            std::fs::write(self.workspace.join(format!("{cli}_mode")), mode).unwrap();
        }

        pub fn calls(&self, cli: &str) -> Vec<Call> {
            let log = std::fs::read_to_string(self.workspace.join(format!("{cli}_calls.jsonl"))).unwrap_or_default();
            log.lines().map(|line| serde_json::from_str(line).unwrap()).collect()
        }
    }

    fn copy_dir(from: &Path, to: &Path) {
        std::fs::create_dir_all(to).unwrap();
        for entry in std::fs::read_dir(from).unwrap() {
            let entry = entry.unwrap();
            if entry.file_type().unwrap().is_dir() {
                copy_dir(&entry.path(), &to.join(entry.file_name()));
            } else {
                std::fs::copy(entry.path(), to.join(entry.file_name())).unwrap();
            }
        }
    }

    /// A harness for the fake [cli], with its sessions in the throwaway folder.
    pub fn harness(fakes: &Fakes, cli: &'static str, model: &str, effort: &str) -> Harness {
        let now = || std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs_f64();
        Harness {
            name: cli,
            binary: fake(cli),
            workspace: fakes.workspace.clone(),
            system_prompt_file: fakes.dir.path().join("system-prompt.md"),
            prompt_vars: vec![("workspace".into(), "/workspace".into())],
            model: model.into(),
            effort: effort.into(),
            sessions: SessionStore::new(fakes.dir.path().join("session_id"), 3600.0, now),
            timeout: Duration::from_secs(5),
            lock: tokio::sync::Mutex::new(()),
        }
    }

    fn vars() -> Vec<(String, String)> {
        vec![("workspace".into(), "/repo/agent".into()), ("skills_dir".into(), "/home/me/skills".into())]
    }

    #[test]
    fn the_prompts_names_are_filled_in() {
        assert_eq!(substitute("in $workspace/src, skills in ${skills_dir}/x", &vars()), "in /repo/agent/src, skills in /home/me/skills/x");
    }

    #[test]
    fn other_dollars_are_left_alone() {
        assert_eq!(substitute("costs $5, $HOME, $$workspace, ${nope}, end $", &vars()), "costs $5, $HOME, $workspace, ${nope}, end $");
    }

    #[test]
    fn why_it_failed_is_its_first_words_on_stderr() {
        let run = RunResult { code: Some(1), stderr: "\nNot logged in\nmore".into(), timed_out: false };

        assert_eq!(why_it_failed("claude", &run), "Not logged in");
        assert_eq!(why_it_failed("claude", &RunResult { stderr: String::new(), ..run }), "claude exited with code 1");
    }
}
