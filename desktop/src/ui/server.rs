//! Saambha's server, in this process: what the dot and its panel show of it and ask of it. Commands
//! run on the server's runtime; the UI's main loop awaits them.

use std::sync::Arc;

use tokio::runtime::Handle;
use tokio::sync::watch;

use crate::server::history::Entry;
use crate::server::services::Services;
use crate::server::status::Activity;

#[derive(Clone)]
pub struct Server {
    services: Arc<Services>,
    runtime: Handle,
}

impl Server {
    pub fn new(services: Arc<Services>, runtime: Handle) -> Server {
        Server { services, runtime }
    }

    /// What Saambha is doing, told of each change.
    pub fn status(&self) -> watch::Receiver<Activity> {
        self.services.status.subscribe()
    }

    /// The last [limit] commands and replies, and new conversations, oldest first.
    pub fn history(&self, limit: usize) -> Vec<Entry> {
        self.services.history.recent(limit)
    }

    /// The log from [offset] on (without one, its end), and the offset to carry on from.
    pub fn log(&self, offset: Option<u64>) -> (String, u64) {
        self.services.log.read(offset)
    }

    pub fn answer_pairing(&self, allow: bool) {
        self.services.status.answer(allow);
    }

    pub fn stop_speaking(&self) {
        self.services.speaker.stop();
    }

    /// Runs [text] as a command; the reply shows in the history (it isn't spoken).
    pub async fn send_command(&self, text: &str) {
        let (services, text) = (self.services.clone(), text.to_string());
        let _ = self.runtime.spawn(async move { services.run_command(&text, false).await }).await;
    }

    pub fn new_conversation(&self) {
        self.services.new_conversation();
    }
}
