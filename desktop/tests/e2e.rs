//! End to end: the real server over HTTPS (its real wiring, compose::services), driven the way the phone
//! and watch apps and the hotkey use it, with a fresh CA from scripts/gen_certs.sh and the fakes in
//! tests/fakes standing in for the agent, whisper and piper. Requests from 127.0.0.2 and 127.0.0.3 stand
//! in for LAN devices; only 127.0.0.1 counts as this machine, where the dot is (which asks the
//! services directly, in the same process).

use std::collections::HashMap;
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use reqwest::StatusCode;
use saambha::server::services::Services;
use saambha::server::settings::Settings;
use saambha::server::{api, compose};
use serde_json::{Value, json};

const LAN: [u8; 4] = [127, 0, 0, 2];
const STRANGER: [u8; 4] = [127, 0, 0, 3];
const HERE: [u8; 4] = [127, 0, 0, 1];

fn app() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

struct Saambha {
    url: String,
    ca: reqwest::Certificate,
    services: Arc<Services>,
    work: tempfile::TempDir,
    harness: &'static str,
}

/// The server with [harness], its files in a throwaway folder.
async fn saambha(harness: &'static str) -> Saambha {
    let work = tempfile::tempdir().unwrap();
    let dir = work.path();
    let made = std::process::Command::new(app().join("../scripts/gen_certs.sh")).arg(dir).output().unwrap();
    assert!(made.status.success(), "{made:?}");
    std::fs::create_dir_all(dir.join("agent")).unwrap();
    std::fs::copy(app().join("../agent/system-prompt.md"), dir.join("agent/system-prompt.md")).unwrap();
    copy_dir(&app().join("tests/fixtures/skills"), &dir.join("skills"));
    let fake = |name: &str| app().join("tests/fakes").join(name).to_string_lossy().into_owned();
    let path = |name: &str| dir.join(name).to_string_lossy().into_owned();
    let vars: HashMap<String, String> = [
        ("HARNESS", harness.to_string()),
        ("WORKSPACE", path("agent")),
        ("SKILLS_DIR", path("skills")),
        ("CLAUDE_BIN", fake("claude")),
        ("PI_BIN", fake("pi")),
        ("PIPER_BIN", fake("piper")),
        ("PIPER_VOICE", path("voice/voice.onnx")),
        ("APLAY_BIN", fake("aplay")),
        ("WHISPER_BIN", fake("whisper-cli")),
        ("WHISPER_MODEL", path("whisper/model.bin")),
        ("UINPUT_PATH", path("no-uinput")), // so the tests can't move the real mouse
    ].map(|(name, value)| (format!("SAAMBHA_{name}"), value)).into();
    for folder in ["voice", "whisper"] {
        std::fs::create_dir_all(dir.join(folder)).unwrap();
    }
    let settings = Settings::new(dir, dir, |name| vars.get(name).cloned()).unwrap();

    let services = compose::services(&settings).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("https://127.0.0.1:{}", listener.local_addr().unwrap().port());
    tokio::spawn(api::serve(listener, api::tls(&settings.cert_file, &settings.key_file).unwrap(), api::router(services.clone())));
    let ca = reqwest::Certificate::from_pem(&std::fs::read(dir.join("ca.pem")).unwrap()).unwrap();
    Saambha { url, ca, services, work, harness }
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

impl Saambha {
    /// A client connecting from [ip], trusting only the server's CA, like the apps.
    fn device(&self, ip: [u8; 4], token: Option<&str>) -> Client {
        let http = reqwest::Client::builder().tls_certs_only([self.ca.clone()]).local_address(IpAddr::from(ip))
            .timeout(Duration::from_secs(30)).build().unwrap();
        Client { http, url: self.url.clone(), token: token.map(String::from) }
    }

    fn file(&self, path: &str) -> String {
        std::fs::read_to_string(self.work.path().join(path)).unwrap_or_default()
    }

    fn spoken(&self) -> String {
        self.file("voice/spoken.txt")
    }

    fn piper(&self) -> i32 {
        self.file("voice/piper.pid").trim().parse().unwrap()
    }

    /// The fake agent's runs, oldest first.
    fn agent_calls(&self) -> Vec<Vec<String>> {
        self.file(&format!("agent/{}_calls.jsonl", self.harness)).lines()
            .map(|line| serde_json::from_str::<Value>(line).unwrap()["args"].as_array().unwrap().iter().map(|a| a.as_str().unwrap().to_string()).collect())
            .collect()
    }

    /// The session an agent run used: pi is given --session-id, Claude Code --resume.
    fn session_of(&self, args: &[String]) -> Option<String> {
        let flag = if self.harness == "pi" { "--session-id" } else { "--resume" };
        args.iter().position(|a| a == flag).map(|at| args[at + 1].clone())
    }

    /// POST /pair from [ip], answered on the desktop as the panel does: the reply, and what the dot showed.
    async fn ask_to_pair(&self, ip: [u8; 4], name: &str, allow: bool) -> (StatusCode, Value, String) {
        let device = self.device(ip, None);
        let name = name.to_string();
        let asking = tokio::spawn(async move { device.post("/pair", Some(json!({ "name": name }))).await });
        let mut changes = self.services.status.subscribe();
        changes.wait_for(|a| a.pairing.is_some()).await.unwrap();
        let asked = self.services.status.activity().look().1;
        assert!(self.services.status.answer(allow));
        let (status, reply) = asking.await.unwrap();
        (status, reply, asked)
    }

    async fn pair(&self, name: &str) -> String {
        let (status, reply, _) = self.ask_to_pair(LAN, name, true).await;
        assert_eq!(status, StatusCode::OK, "{reply}");
        reply["token"].as_str().unwrap().to_string()
    }
}

struct Client {
    http: reqwest::Client,
    url: String,
    token: Option<String>,
}

impl Client {
    async fn send(&self, request: reqwest::RequestBuilder) -> (StatusCode, Value) {
        let request = match &self.token {
            Some(token) => request.bearer_auth(token),
            None => request,
        };
        let response = request.send().await.unwrap();
        let status = response.status();
        (status, response.json().await.unwrap_or(Value::Null))
    }

    async fn get(&self, path: &str) -> (StatusCode, Value) {
        self.send(self.http.get(format!("{}{path}", self.url))).await
    }

    async fn post(&self, path: &str, body: Option<Value>) -> (StatusCode, Value) {
        let request = self.http.post(format!("{}{path}", self.url));
        self.send(match body {
            Some(body) => request.json(&body),
            None => request,
        }).await
    }

    async fn delete(&self, path: &str) -> (StatusCode, Value) {
        self.send(self.http.delete(format!("{}{path}", self.url))).await
    }
}

async fn wait_until(condition: impl Fn() -> bool) -> bool {
    for _ in 0..250 {
        if condition() {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    false
}

fn running(pid: i32) -> bool {
    std::fs::read_to_string(format!("/proc/{pid}/stat")).is_ok_and(|stat| stat.split_whitespace().nth(2) != Some("Z"))
}

const HARNESSES: [&str; 2] = ["pi", "claude"];

#[tokio::test]
async fn only_clients_trusting_our_ca_can_connect() {
    let s = saambha("claude").await;

    assert!(reqwest::get(format!("{}/discover", s.url)).await.is_err()); // the system's trust store only
    assert_eq!(s.device(LAN, None).get("/discover").await, (StatusCode::OK, json!({"service": "saambha", "name": s.services.hostname})));
}

#[tokio::test]
async fn an_unpaired_device_is_turned_away() {
    for harness in HARNESSES {
        let s = saambha(harness).await;
        let device = s.device(LAN, None);

        assert_eq!(device.get("/status").await.0, StatusCode::UNAUTHORIZED);
        assert_eq!(device.post("/command", Some(json!({"text": "hi"}))).await.0, StatusCode::UNAUTHORIZED);
        assert_eq!(device.get("/mouse").await.0, StatusCode::UNAUTHORIZED);
        assert!(s.agent_calls().is_empty());
    }
}

#[tokio::test]
async fn pair_then_ask() {
    for harness in HARNESSES {
        let s = saambha(harness).await;

        let (_, reply, asked) = s.ask_to_pair(LAN, "Pixel Watch 4", true).await;
        let device = s.device(LAN, reply["token"].as_str());

        assert_eq!(asked, "Pixel Watch 4 (127.0.0.2) wants to connect");
        assert_eq!(device.get("/status").await.1["ok"], true);
        let (status, reply) = device.post("/command", Some(json!({"text": "what time is it", "speak": "none"}))).await;
        assert_eq!((status, &reply["text"]), (StatusCode::OK, &json!("You said: what time is it")), "{harness}");
        assert_eq!(s.spoken(), ""); // the phone speaks this reply itself
    }
}

#[tokio::test]
async fn the_reply_is_spoken_on_the_desktop_and_a_new_command_cuts_it_off() {
    for harness in HARNESSES {
        let s = saambha(harness).await;
        let device = s.device(LAN, Some(&s.pair("Phone").await));
        std::fs::write(s.work.path().join("voice/speech_seconds"), "30").unwrap();

        device.post("/command", Some(json!({"text": "tell me a long story"}))).await;
        assert!(wait_until(|| s.spoken() == "You said: tell me a long story").await, "{harness}");
        let speaking = s.piper();
        device.post("/command", Some(json!({"text": "stop", "speak": "none"}))).await;

        assert!(wait_until(|| !running(speaking)).await);
    }
}

#[tokio::test]
async fn a_follow_up_continues_the_conversation() {
    for harness in HARNESSES {
        let s = saambha(harness).await;
        let device = s.device(LAN, Some(&s.pair("Phone").await));

        device.post("/reset", None).await;
        device.post("/command", Some(json!({"text": "first", "speak": "none"}))).await;
        device.post("/command", Some(json!({"text": "second", "speak": "none"}))).await;

        let [first, second] = s.agent_calls().try_into().unwrap();
        // pi: saambha picks the id up front; Claude Code: the id comes back from the first run (sess-1).
        let expected = if harness == "pi" { s.session_of(&first) } else { Some("sess-1".into()) };
        assert!(expected.is_some());
        assert_eq!(s.session_of(&second), expected, "{harness}");
    }
}

#[tokio::test]
async fn a_denied_device_gets_403_and_isnt_asked_about_again() {
    let s = saambha("claude").await;

    let (status, _, _) = s.ask_to_pair(STRANGER, "Stranger", false).await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    assert_eq!(s.device(STRANGER, None).post("/pair", Some(json!({"name": "Stranger"}))).await.0, StatusCode::FORBIDDEN); // straight away
    assert_eq!(s.services.status.activity().pairing, None);
}

#[tokio::test]
async fn a_revoked_device_is_turned_away() {
    let s = saambha("claude").await;
    let token = s.pair("Phone").await;
    let devices = s.work.path().join("devices.json");
    let kept = std::fs::read_to_string(&devices).unwrap();

    let revoked = s.pair("Revoke me").await;
    std::fs::write(&devices, kept).unwrap();

    assert_eq!(s.device(LAN, Some(&revoked)).get("/status").await.0, StatusCode::UNAUTHORIZED);
    assert_eq!(s.device(LAN, Some(&token)).get("/status").await.0, StatusCode::OK);
}

#[tokio::test]
async fn the_hotkey_on_this_machine_needs_no_token() {
    for harness in HARNESSES {
        let s = saambha(harness).await;
        let hotkey = s.device(HERE, None);
        std::fs::write(s.work.path().join("voice/speech_seconds"), "30").unwrap();
        std::fs::write(s.work.path().join("whisper/transcript.txt"), " Hello there.\n").unwrap();

        assert_eq!(hotkey.post("/listen", None).await.0, StatusCode::OK); // a recording starts
        assert!(s.services.status.activity().listening);
        let form = reqwest::multipart::Form::new().part("audio", reqwest::multipart::Part::bytes(b"RIFF-audio".to_vec()).file_name("rec.wav"));
        let (status, reply) = hotkey.send(hotkey.http.post(format!("{}/voice", s.url)).multipart(form)).await;

        assert_eq!((status, reply), (StatusCode::OK, json!({"input": "Hello there.", "text": "You said: Hello there."})), "{harness}");
        assert!(wait_until(|| s.spoken() == "You said: Hello there.").await);
        let speaking = s.piper();
        assert_eq!(hotkey.post("/listen", None).await.0, StatusCode::OK); // the next recording cuts it off
        assert!(wait_until(|| !running(speaking)).await);
        assert_eq!(hotkey.delete("/listen").await.0, StatusCode::OK); // and ends without a word
        assert_eq!(s.services.status.activity().look().1, "Ready");
        assert_eq!(s.services.history.recent(1)[0].reply, "You said: Hello there.");
    }
}

#[tokio::test]
async fn without_a_mouse_device_the_touchpad_says_why() {
    let s = saambha("claude").await;

    let error = (s.services.mouse)().err().unwrap();

    assert_eq!(error.kind(), std::io::ErrorKind::NotFound);
}
