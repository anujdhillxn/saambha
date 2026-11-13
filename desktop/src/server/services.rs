//! What Saambha does, for the HTTP API and the dot alike, with the services it does it with (see main.rs
//! for the real ones).

use std::io::Write;
use std::sync::Arc;

use crate::server::agent::{Assistant, Event};
use crate::server::auth::{DeviceRegistry, Pairing};
use crate::server::history::ChatHistory;
use crate::server::logs::LogFile;
use crate::server::mouse::{Mouse, Pacer};
use crate::server::reply::{Reply, parse};
use crate::server::status::Status;
use crate::server::voice::narrator::Speaker;
use crate::server::voice::whisper::Transcriber;

pub struct Services {
    pub assistant: Arc<dyn Assistant>,
    pub devices: Arc<DeviceRegistry>,
    pub pairing: Pairing,
    pub speaker: Arc<dyn Speaker>,
    pub status: Arc<Status>,
    pub history: ChatHistory,
    pub log: LogFile,
    pub transcriber: Box<dyn Transcriber>,
    /// A new mouse on the desktop, there while it's kept.
    pub mouse: Box<dyn Fn() -> std::io::Result<Box<dyn Mouse>> + Send + Sync>,
    /// One for each touchpad connection.
    pub pacer: Box<dyn Fn() -> Pacer + Send + Sync>,
    pub hostname: String,
}

#[derive(Debug, PartialEq)]
pub enum VoiceError {
    /// The recording can't be read.
    Unreadable,
    /// Nothing was said in it.
    Silent,
}

impl Services {
    /// Runs [text] as a command: the agent's reply, kept in the history and, if [speak], spoken here.
    pub async fn run_command(&self, text: &str, speak: bool) -> Reply {
        log::info!("INPUT: {text:?}");
        self.status.set_task(Some("Thinking".into()));
        let status = self.status.clone();
        let on_event = move |event: Event| match event {
            Event::Text(text) => log::info!("STREAM text: {text:?}"),
            Event::ToolUse(tool) => {
                log::info!("STREAM tool: {tool}");
                status.set_task(Some(format!("Running {tool}")));
            }
        };
        let response = self.assistant.ask(text, &on_event).await;
        log::info!("RESPONSE: {response:?}");
        let reply = parse(&response);
        self.history.add(text, &reply.summary, &reply.details); // before the change that tells the panel to show it
        self.status.set_task(None);
        if speak && !reply.summary.is_empty() {
            self.speaker.speak(&reply.summary);
        }
        reply
    }

    /// The hotkey started recording (cutting off the narration) or stopped without anything to send.
    pub fn listen(&self, listening: bool) {
        if listening {
            self.speaker.stop();
        }
        self.status.set_listening(listening);
    }

    /// Runs what's said in the hotkey's recording ([audio], its file's [suffix] e.g. ".wav") as a command:
    /// what was said, and the reply.
    pub async fn voice(&self, audio: &[u8], suffix: &str) -> Result<(String, Reply), VoiceError> {
        self.status.set_listening(false);
        self.speaker.stop();
        log::info!("VOICE upload: {} bytes ({suffix})", audio.len());
        let file = tempfile::Builder::new().prefix("saambha-voice-").suffix(suffix).tempfile()
            .and_then(|mut file| file.write_all(audio).map(|_| file));
        let file = file.map_err(|e| {
            log::error!("Can't keep the recording: {e}");
            VoiceError::Unreadable
        })?;
        let text = self.transcriber.transcribe(file.path()).await.map_err(|e| {
            log::warn!("Could not decode audio ({} bytes): {e}", audio.len());
            VoiceError::Unreadable
        })?;
        if text.is_empty() {
            return Err(VoiceError::Silent);
        }
        let reply = self.run_command(&text, true).await;
        Ok((text, reply))
    }

    /// Forgets the conversation, and marks where the new one starts in the history.
    pub fn new_conversation(&self) {
        self.assistant.reset();
        self.history.start_conversation();
    }
}
