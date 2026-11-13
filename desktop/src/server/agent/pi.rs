//! The pi harness (https://pi.dev): `pi --mode json` in the workspace.
//!
//! pi gets saambha's prompt appended to its own, the skills in skills_dir (listed in its system prompt;
//! the model reads a skill's SKILL.md when it uses it), and the extensions given (.pi/guard.ts blocks
//! sudo and rm -rf). Saambha picks the session id, so a follow-up continues the same session.

use std::io::Read;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::server::BoxFuture;
use crate::server::agent::{Assistant, Event, EventHandler, Harness, TIMEOUT_REPLY, why_it_failed};

/// When the model ran something, then finished without a word.
const DONE_REPLY: &str = "Done.";
/// When it did nothing at all.
const SILENT_REPLY: &str = "Sorry, I didn't get anywhere with that. Please try again.";

pub struct Pi {
    pub base: Harness,
    pub skills_dir: PathBuf,
    pub extensions: Vec<PathBuf>,
    pub session_dir: PathBuf,
    pub new_session_id: Box<dyn Fn() -> String + Send + Sync>,
}

/// A random (version 4) UUID.
pub fn uuid() -> String {
    let mut b = [0u8; 16];
    std::fs::File::open("/dev/urandom").and_then(|mut f| f.read_exact(&mut b)).expect("random bytes");
    b[6] = b[6] & 0x0f | 0x40;
    b[8] = b[8] & 0x3f | 0x80;
    let hex: String = b.iter().map(|byte| format!("{byte:02x}")).collect();
    format!("{}-{}-{}-{}-{}", &hex[..8], &hex[8..12], &hex[12..16], &hex[16..20], &hex[20..])
}

/// pi's models.json [config] with Ollama's [model] in it, keeping the other entries.
///
/// It's marked a reasoning model, so pi passes on its thinking level, "off" as Ollama's "none": told
/// nothing, models like Gemma 4 think at length anyway.
pub fn with_ollama_model(mut config: Value, url: &str, model: &str, context: u64) -> Value {
    if !config.is_object() {
        config = json!({});
    }
    let provider = config["providers"]["ollama"].take();
    let mut models: Vec<Value> = provider["models"].as_array().cloned().unwrap_or_default();
    models.retain(|m| m["id"] != model);
    models.push(json!({"id": model, "contextWindow": context, "reasoning": true, "thinkingLevelMap": {"off": "none"}}));
    let mut provider = if provider.is_object() { provider } else { json!({}) };
    provider["baseUrl"] = json!(format!("{}/v1", url.trim_end_matches('/')));
    provider["api"] = json!("openai-completions");
    provider["apiKey"] = json!("ollama");
    provider["models"] = json!(models);
    config["providers"]["ollama"] = provider;
    config
}

/// Tells pi about Ollama's [model], in its models.json at [path], if it doesn't know it as it should.
pub fn ensure_ollama_model(path: &Path, url: &str, model: &str, context: u64) -> std::io::Result<()> {
    let config: Value = std::fs::read_to_string(path).ok().and_then(|text| serde_json::from_str(&text).ok()).unwrap_or(json!({}));
    let wanted = with_ollama_model(config.clone(), url, model, context);
    if wanted != config {
        std::fs::create_dir_all(path.parent().expect("pi's config folder"))?;
        std::fs::write(path, serde_json::to_string_pretty(&wanted).expect("pi's models as JSON") + "\n")?;
        log::info!("Told pi about {model} in {}", path.display());
    }
    Ok(())
}

/// The skill whose SKILL.md is at [path], if it's one.
fn skill_in(path: &str) -> Option<&str> {
    let mut parts = path.rsplit('/');
    (parts.next() == Some("SKILL.md")).then(|| parts.next()).flatten().filter(|name| !name.is_empty())
}

impl Pi {
    async fn ask_locked(&self, text: &str, on_event: EventHandler<'_>) -> String {
        let session = self.base.sessions.load().unwrap_or_else(|| (self.new_session_id)());
        let reply = self.run(text, &session, on_event).await;
        self.base.sessions.save(&session);
        reply
    }

    async fn run(&self, text: &str, session: &str, on_event: EventHandler<'_>) -> String {
        let mut cmd: Vec<String> = vec![
            self.base.binary.clone(), "--mode".into(), "json".into(), "--print".into(),
            "--model".into(), self.base.model.clone(),
            "--thinking".into(), self.base.effort.clone(),
            "--session-id".into(), session.into(),
            "--session-dir".into(), self.session_dir.to_string_lossy().into(),
            "--append-system-prompt".into(), self.base.system_prompt(),
            "--skill".into(), self.skills_dir.to_string_lossy().into(),
            "--no-context-files".into(), // the repo's CLAUDE.md is for developing saambha, not for the assistant
        ];
        for extension in &self.extensions {
            cmd.extend(["--extension".into(), extension.to_string_lossy().into()]);
        }
        cmd.extend(["--".into(), text.into()]); // so a message starting with "-" isn't read as an option

        let (mut reply, mut error, mut acted) = (String::new(), None::<String>, false);
        let run = self.base.run(&cmd, &mut |event: &Value| match event["type"].as_str() {
            Some("tool_execution_start") => {
                acted = true;
                let tool = event["toolName"].as_str().unwrap_or("tool");
                let path = event["args"]["path"].as_str().unwrap_or_default();
                on_event(Event::ToolUse(match skill_in(path) {
                    Some(skill) if tool == "read" => format!("skill {skill}"),
                    _ => tool.to_string(),
                }));
            }
            Some("message_end") if event["message"]["role"] == "assistant" => {
                let message = &event["message"];
                if message["stopReason"] == "error" {
                    // pi retries on its own; this sticks only if nothing better follows.
                    error = Some(message["errorMessage"].as_str().unwrap_or("unknown error").to_string());
                    return;
                }
                let said: String = message["content"].as_array().into_iter().flatten()
                    .filter(|block| block["type"] == "text")
                    .filter_map(|block| block["text"].as_str())
                    .collect();
                let said = said.trim();
                if !said.is_empty() {
                    on_event(Event::Text(said.to_string()));
                }
                if message["stopReason"] != "toolUse" {
                    // the last word, not what it says on the way
                    reply = said.to_string();
                    error = None;
                }
            }
            _ => {}
        }).await;
        if run.timed_out {
            return TIMEOUT_REPLY.into();
        }
        if run.code != Some(0) && reply.is_empty() && error.is_none() {
            // e.g. not logged in: pi says why on stderr ("No API key found for anthropic.")
            error = Some(why_it_failed("pi", &run));
        }
        match error {
            Some(why) if reply.is_empty() => {
                log::error!("pi failed: {why}");
                format!("Sorry, something went wrong: {why}")
            }
            _ if !reply.is_empty() => reply,
            _ if acted => DONE_REPLY.into(),
            _ => {
                log::warn!("pi's model ended without a word or a command");
                SILENT_REPLY.into()
            }
        }
    }
}

impl Assistant for Pi {
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

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use super::*;
    use crate::server::agent::tests::{Fakes, harness, real_workspace};

    fn pi(fakes: &Fakes) -> Pi {
        let n = Arc::new(AtomicUsize::new(0));
        Pi {
            base: harness(fakes, "pi", "anthropic/test-model", "medium"),
            skills_dir: fakes.workspace.join("skills"),
            extensions: vec![real_workspace().join(".pi/guard.ts")],
            session_dir: fakes.dir.path().join("pi-sessions"),
            new_session_id: Box::new(move || format!("session-{}", n.fetch_add(1, Ordering::SeqCst) + 1)),
        }
    }

    async fn ask(pi: &Pi, text: &str) -> (String, Vec<Event>) {
        let events = Mutex::new(Vec::new());
        let reply = pi.ask(text, &|e| events.lock().unwrap().push(e)).await;
        (reply, events.into_inner().unwrap())
    }

    #[tokio::test]
    async fn returns_the_reply_and_streams_events() {
        let fakes = Fakes::default();

        let (reply, events) = ask(&pi(&fakes), "play music").await;

        assert_eq!(reply, "You said: play music");
        assert_eq!(events, [Event::ToolUse("skill spotify".into()), Event::Text("You said: play music".into())]);
    }

    #[tokio::test]
    async fn the_reply_is_the_last_word_not_what_it_said_on_the_way() {
        let fakes = Fakes::default();
        fakes.set_mode("pi", "narrate");

        let (reply, events) = ask(&pi(&fakes), "move the sound").await;

        assert_eq!(reply, "Switched it");
        assert!(events.contains(&Event::Text("I will set the default sink to 91.".into()))); // shown as it works
        assert!(events.contains(&Event::ToolUse("bash".into())));
    }

    #[tokio::test]
    async fn ending_without_a_word_says_its_done() {
        let fakes = Fakes::default();
        fakes.set_mode("pi", "narrate-mute");

        assert_eq!(ask(&pi(&fakes), "move the sound").await.0, DONE_REPLY);
    }

    #[tokio::test]
    async fn doing_nothing_at_all_isnt_called_done() {
        let fakes = Fakes::default();
        fakes.set_mode("pi", "mute");

        assert_eq!(ask(&pi(&fakes), "Play Tension").await.0, SILENT_REPLY);
    }

    #[tokio::test]
    async fn runs_in_the_workspace_with_prompt_skills_and_guard() {
        let fakes = Fakes::default();

        ask(&pi(&fakes), "hi").await;

        let [call] = fakes.calls("pi").try_into().unwrap();
        assert_eq!(call.cwd, fakes.workspace);
        assert!(call.after("--mode") == "json" && call.args.iter().any(|a| a == "--print"));
        assert_eq!(call.after("--model"), "anthropic/test-model");
        assert_eq!(call.after("--thinking"), "medium");
        assert_eq!(call.after("--append-system-prompt"), "Be Saambha in /workspace.");
        assert_eq!(call.after("--skill"), fakes.workspace.join("skills").to_str().unwrap());
        assert_eq!(call.after("--extension"), real_workspace().join(".pi/guard.ts").to_str().unwrap());
        assert!(call.args.iter().any(|a| a == "--no-context-files"));
        assert_eq!(call.after("--session-dir"), fakes.dir.path().join("pi-sessions").to_str().unwrap());
        assert_eq!(call.args[call.args.len() - 2..], ["--", "hi"]);
    }

    #[tokio::test]
    async fn a_message_starting_with_a_dash_is_not_an_option() {
        let fakes = Fakes::default();

        assert_eq!(ask(&pi(&fakes), "--help me").await.0, "You said: --help me");
    }

    #[tokio::test]
    async fn a_follow_up_continues_the_same_session_and_reset_starts_a_new_one() {
        let fakes = Fakes::default();
        let pi = pi(&fakes);

        ask(&pi, "first").await;
        ask(&pi, "second").await;
        assert!(pi.has_session());
        pi.reset();
        ask(&pi, "third").await;

        let sessions: Vec<String> = fakes.calls("pi").iter().map(|c| c.after("--session-id").to_string()).collect();
        assert_eq!(sessions, ["session-1", "session-1", "session-2"]);
    }

    #[tokio::test]
    async fn a_model_error_is_reported_unless_a_retry_succeeds() {
        let fakes = Fakes::default();
        let pi = pi(&fakes);

        fakes.set_mode("pi", "error");
        assert_eq!(ask(&pi, "hello").await.0, "Sorry, something went wrong: Connection error.");
        fakes.set_mode("pi", "retry");
        assert_eq!(ask(&pi, "hello").await.0, "You said: hello");
    }

    #[tokio::test]
    async fn not_logged_in() {
        let fakes = Fakes::default();
        fakes.set_mode("pi", "no-auth");

        assert_eq!(ask(&pi(&fakes), "hello").await.0, "Sorry, something went wrong: No API key found for anthropic.");
    }

    #[tokio::test]
    async fn a_hung_pi_is_killed_after_the_timeout() {
        let fakes = Fakes::default();
        let mut pi = pi(&fakes);
        pi.base.timeout = Duration::from_secs(1);
        fakes.set_mode("pi", "hang");

        let reply = tokio::time::timeout(Duration::from_secs(5), ask(&pi, "hello")).await.unwrap().0;

        assert_eq!(reply, TIMEOUT_REPLY);
    }

    #[test]
    fn a_skill_is_named_by_its_folder() {
        assert_eq!(skill_in("/home/me/skills/spotify/SKILL.md"), Some("spotify"));
        assert_eq!(skill_in("spotify/SKILL.md"), Some("spotify"));
        assert_eq!(skill_in("/home/me/notes.md"), None);
    }

    #[test]
    fn session_ids_are_uuids() {
        let id = uuid();

        assert_eq!((id.len(), &id[14..15]), (36, "4"));
        assert_ne!(id, uuid());
    }

    #[test]
    fn an_ollama_model_is_added_to_pis_models_keeping_the_rest() {
        let config = json!({"providers": {"anthropic": {"apiKey": "k"}, "ollama": {"models": [
            {"id": "other:1b", "contextWindow": 4096}, {"id": "gemma:4b", "contextWindow": 4096}]}}});

        let updated = with_ollama_model(config, "http://127.0.0.1:11434/", "gemma:4b", 16384);

        assert_eq!(updated["providers"]["anthropic"], json!({"apiKey": "k"}));
        let ollama = &updated["providers"]["ollama"];
        assert_eq!((&ollama["baseUrl"], &ollama["api"]), (&json!("http://127.0.0.1:11434/v1"), &json!("openai-completions")));
        assert_eq!(ollama["models"], json!([
            {"id": "other:1b", "contextWindow": 4096},
            {"id": "gemma:4b", "contextWindow": 16384, "reasoning": true, "thinkingLevelMap": {"off": "none"}},
        ]));
    }

    #[test]
    fn pis_models_are_written_only_when_they_need_to_change() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("agent/models.json");

        ensure_ollama_model(&path, "http://127.0.0.1:11434", "gemma:4b", 16384).unwrap();
        let written = std::fs::metadata(&path).unwrap().modified().unwrap();
        std::thread::sleep(Duration::from_millis(20));
        ensure_ollama_model(&path, "http://127.0.0.1:11434", "gemma:4b", 16384).unwrap();

        assert_eq!(std::fs::metadata(&path).unwrap().modified().unwrap(), written);
        let config: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(config["providers"]["ollama"]["models"][0]["id"], "gemma:4b");
    }
}
