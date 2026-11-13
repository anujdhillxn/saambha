//! The connection to the Saambha server, over HTTPS on this machine, trusting only its local CA.
//! Requests run on a tokio runtime of their own; the UI's main loop awaits them.

use std::time::Duration;

use reqwest::{Certificate, Client, Method, RequestBuilder};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use tokio::runtime::Handle;

use crate::chat::Entry;
use crate::status::Status;

const TIMEOUT: Duration = Duration::from_secs(5);
/// For requests the server takes its time over: commands the agent runs, and status requests it
/// holds until something changes.
const SLOW_TIMEOUT: Duration = Duration::from_secs(300);

/// A piece of the server's log, and the offset to ask for next.
#[derive(Deserialize)]
pub struct LogPiece {
    pub text: String,
    pub offset: u64,
}

#[derive(Clone)]
pub struct Server {
    url: String,
    http: Client,
    runtime: Handle,
}

impl Server {
    pub fn new(url: &str, ca_file: &str, runtime: Handle) -> Result<Self, String> {
        let pem = std::fs::read(ca_file).map_err(|e| format!("Can't read the CA {ca_file}: {e}"))?;
        let ca = Certificate::from_pem(&pem).map_err(|e| format!("Bad CA {ca_file}: {e}"))?;
        let http = Client::builder()
            .tls_certs_only([ca])
            .timeout(TIMEOUT)
            .build()
            .map_err(|e| e.to_string())?;
        Ok(Self { url: url.trim_end_matches('/').to_string(), http, runtime })
    }

    /// What Saambha is doing; with [after] (a version of it), once that's changed.
    pub async fn status(&self, after: Option<u64>) -> Result<Status, String> {
        match after {
            None => self.call(self.request(Method::GET, "/desktop/status", None)).await,
            Some(version) => {
                let request = self.request(Method::GET, &format!("/desktop/status?after={version}"), None);
                self.call(request.timeout(SLOW_TIMEOUT)).await
            }
        }
    }

    /// The last [limit] commands and replies, and new conversations, oldest first.
    pub async fn history(&self, limit: u32) -> Result<Vec<Entry>, String> {
        self.call(self.request(Method::GET, &format!("/desktop/history?limit={limit}"), None)).await
    }

    /// The server's log from [offset] on; without one, its end.
    pub async fn log(&self, offset: Option<u64>) -> Result<LogPiece, String> {
        let query = offset.map(|o| format!("?offset={o}")).unwrap_or_default();
        self.call(self.request(Method::GET, &format!("/desktop/log{query}"), None)).await
    }

    pub async fn answer_pairing(&self, allow: bool) -> Result<(), String> {
        self.call_ok(self.request(Method::POST, "/desktop/pairing", Some(json!({ "allow": allow })))).await
    }

    pub async fn stop_speaking(&self) -> Result<(), String> {
        self.call_ok(self.request(Method::POST, "/desktop/speech/stop", None)).await
    }

    /// Runs [text] as a command; the reply shows in the history (it isn't spoken).
    pub async fn send_command(&self, text: &str) -> Result<(), String> {
        let request = self.request(Method::POST, "/command", Some(json!({ "text": text, "speak": "none" })));
        self.call_ok(request.timeout(SLOW_TIMEOUT)).await
    }

    pub async fn new_conversation(&self) -> Result<(), String> {
        self.call_ok(self.request(Method::POST, "/reset", None)).await
    }

    fn request(&self, method: Method, path: &str, body: Option<Value>) -> RequestBuilder {
        let request = self.http.request(method, format!("{}{path}", self.url));
        match body {
            Some(body) => request.json(&body),
            None => request,
        }
    }

    async fn call_ok(&self, request: RequestBuilder) -> Result<(), String> {
        self.call::<Value>(request).await.map(|_| ())
    }

    /// Sends [request] and returns its JSON reply; an error says why it couldn't.
    async fn call<T: DeserializeOwned + Send + 'static>(&self, request: RequestBuilder) -> Result<T, String> {
        let reply = self.runtime.spawn(async move {
            let response = request.send().await.map_err(|e| e.to_string())?;
            let status = response.status();
            if !status.is_success() {
                return Err(format!("{status}: {}", response.text().await.unwrap_or_default()));
            }
            response.json().await.map_err(|e| e.to_string())
        });
        reply.await.map_err(|e| e.to_string())?
    }
}
