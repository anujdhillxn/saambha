//! The real pi CLI against a fake model server: checks the pi harness against the events pi actually
//! prints, and that the workspace (prompt, skills, guard) works in pi. Skipped where pi isn't installed.
//!
//! The fake model is OpenAI-compatible and streams its replies:
//!   - the user mentions "music": a `read` call for the spotify skill's SKILL.md, then "Playing music"
//!   - the user says "run <command>": a `bash` call for <command>, then "Ran it"
//!   - anything else: "You said: <what they said>"

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::Json;
use axum::extract::State;
use saambha::server::agent::pi::{Pi, with_ollama_model};
use saambha::server::agent::sessions::SessionStore;
use saambha::server::agent::{Assistant, Event, Harness};
use serde_json::{Value, json};

type Requests = Arc<Mutex<Vec<Value>>>;

fn app() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn pi_binary() -> Option<PathBuf> {
    let home = PathBuf::from(std::env::var("HOME").ok()?);
    std::env::split_paths(&std::env::var_os("PATH")?).map(|dir| dir.join("pi")).chain([home.join(".local/bin/pi")]).find(|p| p.is_file())
}

fn text_of(message: &Value) -> String {
    match &message["content"] {
        Value::Array(parts) => parts.iter().filter_map(|p| p["text"].as_str()).collect(),
        content => content.as_str().unwrap_or_default().to_string(),
    }
}

/// {"text": ...} or {"tool": name, "args": {...}} for the conversation so far.
fn reply_for(messages: &[Value]) -> Value {
    let user = messages.iter().rposition(|m| m["role"] == "user").unwrap();
    let asked = text_of(&messages[user]).trim().to_string();
    if messages[user + 1..].iter().any(|m| m["role"] == "tool") {
        return json!({"text": if asked.contains("music") { "Playing music" } else { "Ran it" }});
    }
    if asked.contains("music") {
        return json!({"tool": "read", "args": {"path": "skills/spotify/SKILL.md"}});
    }
    if let Some(command) = asked.strip_prefix("run ") {
        return json!({"tool": "bash", "args": {"command": command}});
    }
    json!({"text": format!("You said: {asked}")})
}

fn chunk(delta: Value, finish: Option<&str>) -> String {
    let mut body = json!({"id": "fake", "object": "chat.completion.chunk", "created": 0, "model": "fake-model",
                          "choices": [{"index": 0, "delta": delta, "finish_reason": finish}]});
    if finish.is_some() {
        body["usage"] = json!({"prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2});
    }
    format!("data: {body}\n\n")
}

async fn complete(State(requests): State<Requests>, Json(body): Json<Value>) -> ([(&'static str, &'static str); 1], String) {
    requests.lock().unwrap().push(body.clone());
    let reply = reply_for(body["messages"].as_array().unwrap());
    let mut stream = match reply["text"].as_str() {
        Some(text) => chunk(json!({"role": "assistant", "content": text}), None) + &chunk(json!({}), Some("stop")),
        None => {
            let call = json!({"index": 0, "id": "call_1", "type": "function",
                              "function": {"name": reply["tool"], "arguments": reply["args"].to_string()}});
            chunk(json!({"role": "assistant", "tool_calls": [call]}), None) + &chunk(json!({}), Some("tool_calls"))
        }
    };
    stream.push_str("data: [DONE]\n\n");
    ([("content-type", "text/event-stream")], stream)
}

struct World {
    pi: Pi,
    requests: Requests,
    dir: tempfile::TempDir,
}

/// pi, pointed (through its own config folder) at the fake model, set up as Saambha sets up an Ollama
/// model; None where pi isn't installed.
async fn world() -> Option<World> {
    let Some(binary) = pi_binary() else {
        eprintln!("skipped: pi isn't installed");
        return None;
    };
    let requests = Requests::default();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let model = axum::Router::new().route("/v1/chat/completions", axum::routing::post(complete)).with_state(requests.clone());
    tokio::spawn(async move { axum::serve(listener, model).await });

    let dir = tempfile::tempdir().unwrap();
    let agent_dir = dir.path().join("pi-agent");
    std::fs::create_dir_all(&agent_dir).unwrap();
    std::fs::write(agent_dir.join("models.json"), with_ollama_model(json!({}), &url, "fake-model", 16384).to_string()).unwrap();
    let wrapper = dir.path().join("pi");
    std::fs::write(&wrapper, format!("#!/bin/sh\nPI_CODING_AGENT_DIR='{}' PI_OFFLINE=1 exec '{}' \"$@\"\n", agent_dir.display(), binary.display())).unwrap();
    std::fs::set_permissions(&wrapper, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    let workspace = dir.path().join("workspace");
    copy_dir(&app().join("tests/fixtures/skills"), &workspace.join("skills"));

    let now = || std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs_f64();
    let pi = Pi {
        base: Harness {
            name: "pi",
            binary: wrapper.to_string_lossy().into(),
            workspace: workspace.clone(),
            system_prompt_file: app().join("../agent/system-prompt.md"),
            prompt_vars: Vec::new(),
            model: "ollama/fake-model".into(),
            effort: "off".into(),
            sessions: SessionStore::new(dir.path().join("session_id"), 3600.0, now),
            timeout: Duration::from_secs(60),
            lock: tokio::sync::Mutex::new(()),
        },
        skills_dir: workspace.join("skills"),
        extensions: vec![app().join("../agent/.pi/guard.ts")],
        session_dir: dir.path().join("pi-sessions"),
        new_session_id: Box::new(saambha::server::agent::pi::uuid),
    };
    Some(World { pi, requests, dir })
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

impl World {
    async fn ask(&self, text: &str) -> (String, Vec<Event>) {
        let events = Mutex::new(Vec::new());
        let reply = self.pi.ask(text, &|e| events.lock().unwrap().push(e)).await;
        (reply, events.into_inner().unwrap())
    }

    fn requests(&self) -> Vec<Value> {
        self.requests.lock().unwrap().clone()
    }

    /// What the tools gave back in the model's last request.
    fn tool_results(&self) -> Vec<String> {
        let last = self.requests().pop().unwrap();
        last["messages"].as_array().unwrap().iter().filter(|m| m["role"] == "tool").map(|m| m["content"].to_string()).collect()
    }
}

#[tokio::test]
async fn a_reply() {
    let Some(w) = world().await else { return };

    assert_eq!(w.ask("hello").await.0, "You said: hello");
}

#[tokio::test]
async fn the_model_is_told_how_hard_to_think() {
    for (effort, sent) in [("off", "none"), ("high", "high")] {
        let Some(mut w) = world().await else { return };
        w.pi.base.effort = effort.into();

        w.ask("hello").await;

        assert_eq!(w.requests()[0]["reasoning_effort"], sent);
    }
}

#[tokio::test]
async fn the_model_gets_saambhas_prompt_and_the_skills() {
    let Some(w) = world().await else { return };

    w.ask("hello").await;

    let request = &w.requests()[0];
    let system = request["messages"][0].to_string();
    assert!(system.contains("You are Saambha") && system.contains("spotify") && system.contains("alarms"));
    let tools: Vec<&str> = request["tools"].as_array().unwrap().iter().filter_map(|t| t["function"]["name"].as_str()).collect();
    assert_eq!(tools[..2], ["read", "bash"]);
}

#[tokio::test]
async fn using_a_skill() {
    let Some(w) = world().await else { return };

    let (reply, events) = w.ask("play some music").await;

    assert_eq!(reply, "Playing music");
    assert!(events.contains(&Event::ToolUse("skill spotify".into())), "{events:?}");
    assert!(w.tool_results()[0].contains("playerctl")); // the model read the skill's SKILL.md
}

#[tokio::test]
async fn a_follow_up_continues_the_conversation() {
    let Some(w) = world().await else { return };

    w.ask("first").await;
    w.ask("second").await;

    let last = w.requests().pop().unwrap();
    let messages = last["messages"].as_array().unwrap();
    assert!(messages.iter().any(|m| m["role"] == "user" && text_of(m).contains("first")));
    assert!(messages.iter().any(|m| m["role"] == "assistant" && m.to_string().contains("You said: first")));
}

#[tokio::test]
async fn the_guard_blocks_sudo_and_rm_rf_and_lets_the_rest_run() {
    let Some(w) = world().await else { return };
    let keep = w.dir.path().join("keep");
    std::fs::create_dir(&keep).unwrap();
    let marker = w.dir.path().join("ran");

    w.ask("run sudo ls").await;
    assert!(w.tool_results()[0].contains("sudo is not allowed"));
    w.ask(&format!("run rm -rf {}", keep.display())).await;
    assert!(w.tool_results().last().unwrap().contains("rm -rf is not allowed"));
    w.ask(&format!("run touch {}", marker.display())).await;

    assert!(keep.exists() && marker.exists());
}
