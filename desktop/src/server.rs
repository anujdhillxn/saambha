//! The connection to the Saambha server, over HTTPS on this machine, trusting only its local CA.
//! (The web view couldn't: it trusts the system's CAs, so the windows ask through here.)

use std::time::Duration;

use reqwest::{Certificate, Client, Method, RequestBuilder};
use serde_json::Value;

const TIMEOUT: Duration = Duration::from_secs(5);
/// For requests the server takes its time over: commands the agent runs, and status requests it
/// holds until something changes (status_wait).
const SLOW_TIMEOUT: Duration = Duration::from_secs(300);

pub struct Server {
    url: String,
    http: Client,
}

impl Server {
    pub fn new(url: &str, ca_file: &str) -> Result<Self, String> {
        let pem = std::fs::read(ca_file).map_err(|e| format!("Can't read the CA {ca_file}: {e}"))?;
        let ca = Certificate::from_pem(&pem).map_err(|e| format!("Bad CA {ca_file}: {e}"))?;
        let http = Client::builder()
            .tls_certs_only([ca])
            .timeout(TIMEOUT)
            .build()
            .map_err(|e| e.to_string())?;
        Ok(Self { url: url.trim_end_matches('/').to_string(), http })
    }

    /// Calls [path] on the server and returns its JSON reply; an error says why it couldn't.
    pub async fn call(&self, method: Method, path: &str, body: Option<Value>) -> Result<Value, String> {
        send(self.request(method, path, body)).await
    }

    /// [call], for a request the server takes its time over.
    pub async fn call_slow(&self, method: Method, path: &str, body: Option<Value>) -> Result<Value, String> {
        send(self.request(method, path, body).timeout(SLOW_TIMEOUT)).await
    }

    fn request(&self, method: Method, path: &str, body: Option<Value>) -> RequestBuilder {
        let request = self.http.request(method, format!("{}{path}", self.url));
        match body {
            Some(body) => request.json(&body),
            None => request,
        }
    }
}

async fn send(request: RequestBuilder) -> Result<Value, String> {
    let response = request.send().await.map_err(|e| e.to_string())?;
    let status = response.status();
    let reply: Value = response.json().await.map_err(|e| e.to_string())?;
    if status.is_success() { Ok(reply) } else { Err(format!("{status}: {reply}")) }
}
