//! Settings: every tunable value and external endpoint, in one place.
//!
//! Any of them can be overridden per user as SAAMBHA_<NAME> in desktop/.env (e.g. SAAMBHA_HARNESS=pi),
//! which also holds secrets (e.g. the Spotify keys) for the agent's skills. See .env.sample.

use std::path::{Path, PathBuf};

/// The app's folder (desktop/): its certificates, approved devices, log and .env live here.
pub fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// KEY=value lines from [path] that [is_set] doesn't already have, to set in the environment, so
/// that the agent's own tools (e.g. the Spotify skill) see them too.
pub fn env_file(path: &Path, is_set: impl Fn(&str) -> bool) -> Vec<(String, String)> {
    let Ok(text) = std::fs::read_to_string(path) else { return Vec::new() };
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .filter_map(|line| line.split_once('='))
        .map(|(key, value)| (key.trim().to_string(), value.trim().to_string()))
        .filter(|(key, _)| !is_set(key))
        .collect()
}

/// A setting's value, read from its SAAMBHA_<NAME> override.
trait Value: Sized {
    fn parse(raw: &str, home: &Path) -> Option<Self>;
    fn show(&self) -> String;
}

impl Value for String {
    fn parse(raw: &str, _: &Path) -> Option<Self> {
        Some(raw.to_string())
    }

    fn show(&self) -> String {
        self.clone()
    }
}

impl Value for PathBuf {
    fn show(&self) -> String {
        self.to_string_lossy().into_owned()
    }

    fn parse(raw: &str, home: &Path) -> Option<Self> {
        Some(match raw.strip_prefix("~/") {
            Some(rest) => home.join(rest),
            None if raw == "~" => home.to_path_buf(),
            None => PathBuf::from(raw),
        })
    }
}

macro_rules! numbers {
    ($($kind:ty),*) => {$(
        impl Value for $kind {
            fn parse(raw: &str, _: &Path) -> Option<Self> {
                raw.parse().ok()
            }

            fn show(&self) -> String {
                self.to_string()
            }
        }
    )*};
}
numbers!(u16, u64, f64);

macro_rules! settings {
    ($($(#[doc = $doc:literal])* $name:ident: $kind:ty = $default:expr,)*) => {
        #[derive(Clone, Debug, PartialEq)]
        pub struct Settings {
            $($(#[doc = $doc])* pub $name: $kind,)*
        }

        impl Settings {
            /// The defaults, for files under [root] and [home], overridden by any SAAMBHA_<NAME> that
            /// [var] has. A value that doesn't parse is an error naming it.
            pub fn new(root: &Path, home: &Path, var: impl Fn(&str) -> Option<String>) -> Result<Settings, String> {
                $(
                    let $name: $kind = {
                        let key = concat!("SAAMBHA_", stringify!($name)).to_uppercase();
                        match var(&key) {
                            Some(raw) => Value::parse(&raw, home).ok_or_else(|| format!("{key}={raw:?} isn't valid"))?,
                            None => $default(root, home),
                        }
                    };
                )*
                Ok(Settings { $($name,)* })
            }

            /// Every setting as a shell variable (NAME='value'), for scripts/install.sh.
            pub fn shell(&self) -> String {
                let mut lines = String::new();
                $(
                    let value = Value::show(&self.$name).replace('\'', r"'\''");
                    lines.push_str(&format!("{}='{value}'\n", stringify!($name).to_uppercase()));
                )*
                lines
            }
        }
    };
}

fn which(name: &str, home: &Path) -> String {
    std::env::var_os("PATH")
        .into_iter()
        .flat_map(|path| std::env::split_paths(&path).collect::<Vec<_>>())
        .map(|dir| dir.join(name))
        .find(|candidate| candidate.is_file())
        .unwrap_or_else(|| home.join(".local/bin").join(name))
        .to_string_lossy()
        .into_owned()
}

settings! {
    port: u16 = |_, _| 8765,
    cert_file: PathBuf = |root: &Path, _| root.join("cert.pem"),
    key_file: PathBuf = |root: &Path, _| root.join("key.pem"),
    devices_file: PathBuf = |root: &Path, _| root.join("devices.json"),
    session_file: PathBuf = |_, home: &Path| home.join(".config/saambha/session_id"),
    /// Seconds without use before the conversation starts over.
    session_ttl: u64 = |_, _| 3600,
    /// Every command and reply, shown in the chat panel.
    history_file: PathBuf = |_, home: &Path| home.join(".local/share/saambha/history.jsonl"),
    /// The agent harness that runs commands: "pi" (https://pi.dev) or "claude" (Claude Code).
    harness: String = |_, _| "claude".to_string(),
    /// The agent's working folder: system-prompt.md, .claude/ and .pi/ live here.
    workspace: PathBuf = |root: &Path, _| root.parent().expect("the repo").join("agent"),
    /// The agent's skills (a separate git repo), shared by both harnesses.
    skills_dir: PathBuf = |_, home: &Path| home.join("saambha-skills"),
    /// Seconds a command may take.
    agent_timeout: u64 = |_, _| 120,
    pi_bin: String = |_, home: &Path| which("pi", home),
    /// provider/model; "ollama/..." models run locally and are unloaded during heavy tasks.
    pi_model: String = |_, _| "anthropic/claude-haiku-5-5".to_string(),
    pi_session_dir: PathBuf = |_, home: &Path| home.join(".local/share/saambha/pi-sessions"),
    /// How hard the model thinks before it acts: "off", "low", "medium" or "high" (Claude Code's least is "low").
    effort: String = |_, _| "off".to_string(),
    claude_bin: String = |_, home: &Path| which("claude", home),
    claude_model: String = |_, _| "claude-haiku-5-5".to_string(),
    /// Speech, and the voice it speaks in (scripts/speech.sh installs both).
    piper_bin: PathBuf = |_, home: &Path| home.join(".local/share/saambha/piper/piper"),
    piper_voice: PathBuf = |_, home: &Path| home.join(".local/share/piper-voices/en_US-lessac-medium.onnx"),
    aplay_bin: String = |_, _| "aplay".to_string(),
    /// Speech recognition, and its model (scripts/speech.sh installs both).
    whisper_bin: PathBuf = |_, home: &Path| home.join(".local/share/saambha/whisper/whisper-cli"),
    whisper_model: PathBuf = |_, home: &Path| home.join(".local/share/saambha/whisper/ggml-small-q8_0.bin"),
    /// Seconds a new device waits to be allowed on the desktop before it's denied.
    approval_timeout: u64 = |_, _| 60,
    /// Seconds before a denied device can ask again.
    deny_cooldown: u64 = |_, _| 300,
    /// Where the kernel makes virtual input devices: the apps' mouse is one.
    uinput_path: PathBuf = |_, _| PathBuf::from("/dev/uinput"),
    /// The touchpad's moves are held back by as much as the network bunches them (see mouse::Pacer):
    /// seconds of recent moves that tell how much that is, and the most they're held back.
    mouse_jitter_window: f64 = |_, _| 1.0,
    mouse_max_delay: f64 = |_, _| 0.15,
    /// Keeping a local (Ollama) model loaded, except while something heavy runs.
    ollama_url: String = |_, _| "http://127.0.0.1:11434".to_string(),
    /// scripts/install.sh sets this context window on "ollama/saambha-<model>" (made from "<model>").
    ollama_context: u64 = |_, _| 16384,
    /// Seconds between looks at what the user is running.
    heavy_check_interval: u64 = |_, _| 10,
    /// Seconds of nothing heavy before the model is loaded again.
    heavy_resume_after: u64 = |_, _| 60,
    /// A process using this much GPU memory is heavy.
    heavy_gpu_mb: u64 = |_, _| 1500,
    /// Less free RAM than this is heavy.
    heavy_min_free_ram_mb: u64 = |_, _| 1500,
    /// A load average above this per core is heavy.
    heavy_cpu_load: f64 = |_, _| 0.75,
    /// "error", "warn", "info" or "debug".
    log_level: String = |_, _| "info".to_string(),
    log_file: PathBuf = |root: &Path, _| root.join("saambha.log"),
    log_max_bytes: u64 = |_, _| 5_000_000,
    log_backups: u64 = |_, _| 3,
    /// Of the log's end, what the log window starts with.
    log_view_bytes: u64 = |_, _| 200_000,
}

impl Settings {
    pub fn system_prompt_file(&self) -> PathBuf {
        self.workspace.join("system-prompt.md")
    }

    pub fn pi_extensions(&self) -> Vec<PathBuf> {
        vec![self.workspace.join(".pi/guard.ts")]
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    fn settings(vars: &[(&str, &str)]) -> Result<Settings, String> {
        let vars: HashMap<String, String> = vars.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        Settings::new(Path::new("/repo/desktop"), Path::new("/home/me"), |key| vars.get(key).cloned())
    }

    #[test]
    fn defaults_live_with_the_app_and_in_the_home_folder() {
        let s = settings(&[]).unwrap();

        assert_eq!(s.port, 8765);
        assert_eq!(s.devices_file, PathBuf::from("/repo/desktop/devices.json"));
        assert_eq!(s.workspace, PathBuf::from("/repo/agent"));
        assert_eq!(s.skills_dir, PathBuf::from("/home/me/saambha-skills"));
        assert_eq!(s.system_prompt_file(), PathBuf::from("/repo/agent/system-prompt.md"));
    }

    #[test]
    fn an_override_is_read_as_the_settings_kind() {
        let s = settings(&[("SAAMBHA_HARNESS", "pi"), ("SAAMBHA_PORT", "9000"), ("SAAMBHA_HEAVY_CPU_LOAD", "1.5"),
                           ("SAAMBHA_SKILLS_DIR", "~/projects/skills")]).unwrap();

        assert_eq!((s.harness.as_str(), s.port, s.heavy_cpu_load), ("pi", 9000, 1.5));
        assert_eq!(s.skills_dir, PathBuf::from("/home/me/projects/skills"));
    }

    #[test]
    fn an_override_that_doesnt_parse_is_named() {
        assert_eq!(settings(&[("SAAMBHA_PORT", "lots")]), Err("SAAMBHA_PORT=\"lots\" isn't valid".to_string()));
    }

    #[test]
    fn every_setting_is_shown_for_the_shell() {
        let shell = settings(&[("SAAMBHA_SKILLS_DIR", "/it's here")]).unwrap().shell();

        assert!(shell.contains("PORT='8765'\n") && shell.contains("HARNESS='claude'\n"), "{shell}");
        assert!(shell.contains(r"SKILLS_DIR='/it'\''s here'"), "{shell}");
    }

    #[test]
    fn the_env_file_fills_in_what_isnt_set() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".env");
        std::fs::write(&path, "# secrets\nSPOTIFY_ID = abc\n\nSAAMBHA_HARNESS=pi\nnot a setting\n").unwrap();

        let vars = env_file(&path, |key| key == "SAAMBHA_HARNESS");

        assert_eq!(vars, [("SPOTIFY_ID".to_string(), "abc".to_string())]);
    }

    #[test]
    fn no_env_file_sets_nothing() {
        assert_eq!(env_file(Path::new("/nowhere/.env"), |_| false), []);
    }
}
