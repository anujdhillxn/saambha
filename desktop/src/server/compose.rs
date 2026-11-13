//! The server's composition root: the only place that wires the real implementations together.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use gtk::glib;
use tokio::net::TcpListener;

use crate::server::agent::claude_code::ClaudeCode;
use crate::server::agent::pi::{self, Pi};
use crate::server::agent::sessions::SessionStore;
use crate::server::agent::{Assistant, Harness};
use crate::server::api;
use crate::server::auth::{Ask, DeviceRegistry, Pairing};
use crate::server::history::ChatHistory;
use crate::server::logs::LogFile;
use crate::server::model::activity::{self, HeavyTaskDetector};
use crate::server::model::ollama::OllamaClient;
use crate::server::model::warden::ModelWarden;
use crate::server::mouse::{Mouse, Pacer, VirtualMouse};
use crate::server::services::Services;
use crate::server::settings::Settings;
use crate::server::status::Status;
use crate::server::voice::narrator::Narrator;
use crate::server::voice::whisper::Whisper;

fn now() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).expect("a time after 1970").as_secs_f64()
}

/// The agent harness named by settings.harness.
pub fn harness(settings: &Settings) -> Result<Arc<dyn Assistant>, String> {
    let base = |name| Harness {
        name,
        binary: if name == "pi" { settings.pi_bin.clone() } else { settings.claude_bin.clone() },
        workspace: settings.workspace.clone(),
        system_prompt_file: settings.system_prompt_file(),
        // $names the system prompt can use
        prompt_vars: vec![
            ("workspace".into(), settings.workspace.to_string_lossy().into()),
            ("skills_dir".into(), settings.skills_dir.to_string_lossy().into()),
            ("app".into(), crate::server::settings::root().to_string_lossy().into()),
        ],
        model: if name == "pi" { settings.pi_model.clone() } else { settings.claude_model.clone() },
        effort: settings.effort.clone(),
        sessions: SessionStore::new(settings.session_file.clone(), settings.session_ttl as f64, now),
        timeout: Duration::from_secs(settings.agent_timeout),
        lock: tokio::sync::Mutex::new(()),
    };
    match settings.harness.as_str() {
        "pi" => Ok(Arc::new(Pi {
            base: base("pi"),
            skills_dir: settings.skills_dir.clone(),
            extensions: settings.pi_extensions(),
            session_dir: settings.pi_session_dir.clone(),
            new_session_id: Box::new(pi::uuid),
        })),
        "claude" => Ok(Arc::new(ClaudeCode::new(base("claude"), &settings.skills_dir)?)),
        other => Err(format!("Unknown harness {other:?}: use \"pi\" or \"claude\"")),
    }
}

/// The local Ollama model pi uses (pi_model "ollama/<model>"), if it does.
fn ollama_model(settings: &Settings) -> Option<&str> {
    settings.pi_model.strip_prefix("ollama/").filter(|_| settings.harness == "pi")
}

/// For a local model: keeps it loaded except while something heavy runs.
fn model_warden(settings: &Settings, assistant: Arc<dyn Assistant>, status: Arc<Status>) -> Option<Arc<ModelWarden>> {
    let model = ollama_model(settings)?;
    let (gpu_mb, ram_mb, load) = (settings.heavy_gpu_mb, settings.heavy_min_free_ram_mb, settings.heavy_cpu_load);
    let cores = std::thread::available_parallelism().map_or(1, usize::from);
    let detector = HeavyTaskDetector::new(vec![
        activity::gpu_memory_hog(gpu_mb, activity::nvidia_gpu_memory),
        activity::low_memory(ram_mb, activity::available_memory_mb),
        activity::high_cpu_load(load, cores, activity::load_average),
    ]);
    let started = Instant::now();
    Some(Arc::new(ModelWarden::new(
        Arc::new(OllamaClient::new(&settings.ollama_url, Duration::from_secs(120))),
        model,
        Arc::new(move || detector.heavy_task()),
        move || assistant.busy(),
        move |reason| status.set_model_cold(reason),
        settings.heavy_resume_after as f64,
        move || started.elapsed().as_secs_f64(),
    )))
}

pub fn services(settings: &Settings) -> Result<Arc<Services>, String> {
    let status = Arc::new(Status::new(Duration::from_secs(settings.approval_timeout)));
    let devices = Arc::new(DeviceRegistry::new(settings.devices_file.clone()));
    let ask: Ask = {
        let status = status.clone();
        Box::new(move |ip, name| {
            let status = status.clone();
            Box::pin(async move { status.ask(&ip, &name).await })
        })
    };
    let speaking = status.clone();
    let (uinput, jitter, max_delay) = (settings.uinput_path.clone(), settings.mouse_jitter_window, settings.mouse_max_delay);
    Ok(Arc::new(Services {
        assistant: harness(settings)?,
        pairing: Pairing::new(devices.clone(), ask, settings.deny_cooldown as f64, now),
        devices,
        speaker: Arc::new(Narrator::new(&settings.piper_bin, &settings.piper_voice, &settings.aplay_bin, move |on| speaking.set_speaking(on))),
        status,
        history: ChatHistory::new(settings.history_file.clone(), now),
        log: LogFile::new(settings.log_file.clone(), settings.log_view_bytes),
        transcriber: Box::new(Whisper { binary: settings.whisper_bin.clone(), model: settings.whisper_model.clone() }),
        mouse: Box::new(move || VirtualMouse::open(&uinput, "Saambha mouse").map(|mouse| Box::new(mouse) as Box<dyn Mouse>)),
        pacer: Box::new(move || Pacer::new(jitter, max_delay)),
        hostname: glib::host_name().into(),
    }))
}

/// Starts serving on the current runtime: the services, which the dot shows and asks things of too.
pub async fn start(settings: &Settings) -> Result<Arc<Services>, String> {
    if let Some(model) = ollama_model(settings) {
        let models = glib::home_dir().join(".pi/agent/models.json");
        pi::ensure_ollama_model(&models, &settings.ollama_url, model, settings.ollama_context)
            .map_err(|e| format!("Can't tell pi about {model}: {e}"))?;
    }
    let services = services(settings)?;
    let tls = api::tls(&settings.cert_file, &settings.key_file)?;
    let address = SocketAddr::from((std::net::Ipv6Addr::UNSPECIFIED, settings.port));
    let listener = TcpListener::bind(address).await.map_err(|e| format!("Can't serve on port {}: {e}", settings.port))?;
    log::info!("Starting on https://[::]:{} with the {} harness", settings.port, settings.harness);
    tokio::spawn(api::serve(listener, tls, api::router(services.clone())));
    if let Some(warden) = model_warden(settings, services.assistant.clone(), services.status.clone()) {
        warden.start(Duration::from_secs(settings.heavy_check_interval));
    }
    Ok(services)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::path::PathBuf;

    use super::*;

    /// Default settings with [overrides] (SAAMBHA_ left off), and the workspace and skills in [dir].
    fn settings(dir: &tempfile::TempDir, overrides: &[(&str, &str)]) -> Settings {
        std::fs::write(dir.path().join("system-prompt.md"), "Work in $workspace, skills in $skills_dir.").unwrap();
        let mut vars: HashMap<String, String> = overrides.iter().map(|(k, v)| (format!("SAAMBHA_{k}"), v.to_string())).collect();
        vars.insert("SAAMBHA_WORKSPACE".into(), dir.path().to_string_lossy().into());
        vars.entry("SAAMBHA_SKILLS_DIR".into()).or_insert(dir.path().join("skills").to_string_lossy().into());
        Settings::new(dir.path(), dir.path(), |name| vars.get(name).cloned()).unwrap()
    }

    #[test]
    fn both_harnesses_are_there_and_no_others() {
        let dir = tempfile::tempdir().unwrap();

        assert!(harness(&settings(&dir, &[("HARNESS", "pi")])).is_ok());
        assert!(harness(&settings(&dir, &[("HARNESS", "claude")])).is_ok());
        assert_eq!(harness(&settings(&dir, &[("HARNESS", "codex")])).err(), Some("Unknown harness \"codex\": use \"pi\" or \"claude\"".into()));
    }

    #[test]
    fn claude_code_finds_the_skills_setting() {
        let dir = tempfile::tempdir().unwrap();
        let skills = dir.path().join("my-skills");

        harness(&settings(&dir, &[("HARNESS", "claude"), ("SKILLS_DIR", skills.to_str().unwrap())])).unwrap();

        assert_eq!(std::fs::read_link(dir.path().join(".claude/skills")).unwrap(), PathBuf::from("../my-skills"));
    }

    #[test]
    fn the_model_warden_is_only_for_a_local_model() {
        let dir = tempfile::tempdir().unwrap();
        for (harness, model, warden) in [("pi", "ollama/gemma4:e4b", true), ("pi", "anthropic/claude-haiku-5-5", false),
                                          ("claude", "ollama/gemma4:e4b", false)] {
            let settings = settings(&dir, &[("HARNESS", harness), ("PI_MODEL", model)]);
            let services = services(&settings).unwrap();

            let built = model_warden(&settings, services.assistant.clone(), services.status.clone());

            assert_eq!(built.is_some(), warden, "{harness} {model}");
            assert_eq!(ollama_model(&settings), warden.then_some("gemma4:e4b"));
        }
    }

    #[tokio::test]
    async fn the_prompt_names_the_real_workspace_and_skills() {
        let dir = tempfile::tempdir().unwrap();
        let settings = settings(&dir, &[("HARNESS", "pi"), ("PI_BIN", &crate::server::agent::tests::fake("pi"))]);

        harness(&settings).unwrap().ask("hi", &|_| {}).await;

        let calls = std::fs::read_to_string(dir.path().join("pi_calls.jsonl")).unwrap();
        let expected = format!("Work in {}, skills in {}.", dir.path().display(), dir.path().join("skills").display());
        assert!(calls.contains(&serde_json::to_string(&expected).unwrap()), "{calls}");
    }
}
