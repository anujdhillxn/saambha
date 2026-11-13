//! Loads and unloads a model in Ollama, through its HTTP API.

use std::time::Duration;

use serde_json::{Value, json};

use crate::server::BoxFuture;

pub trait Ollama: Send + Sync {
    fn loaded_models(&self) -> BoxFuture<'_, Result<Vec<String>, String>>;
    /// Loads [model] (if it isn't already) and keeps it loaded until unloaded.
    fn load<'a>(&'a self, model: &'a str) -> BoxFuture<'a, Result<(), String>>;
    fn unload<'a>(&'a self, model: &'a str) -> BoxFuture<'a, Result<(), String>>;
}

pub struct OllamaClient {
    base_url: String,
    http: reqwest::Client,
}

impl OllamaClient {
    pub fn new(base_url: &str, timeout: Duration) -> OllamaClient {
        let http = reqwest::Client::builder().timeout(timeout).build().expect("an HTTP client");
        OllamaClient { base_url: base_url.trim_end_matches('/').to_string(), http }
    }

    async fn generate(&self, model: &str, keep_alive: i64) -> Result<(), String> {
        self.http.post(format!("{}/api/generate", self.base_url))
            .json(&json!({"model": model, "keep_alive": keep_alive}))
            .send().await
            .and_then(reqwest::Response::error_for_status)
            .map(drop)
            .map_err(|e| e.to_string())
    }
}

impl Ollama for OllamaClient {
    fn loaded_models(&self) -> BoxFuture<'_, Result<Vec<String>, String>> {
        Box::pin(async move {
            let models: Value = self.http.get(format!("{}/api/ps", self.base_url))
                .send().await
                .and_then(reqwest::Response::error_for_status)
                .map_err(|e| e.to_string())?
                .json().await
                .map_err(|e| e.to_string())?;
            Ok(models["models"].as_array().into_iter().flatten().filter_map(|m| m["name"].as_str().map(String::from)).collect())
        })
    }

    fn load<'a>(&'a self, model: &'a str) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(self.generate(model, -1))
    }

    fn unload<'a>(&'a self, model: &'a str) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(self.generate(model, 0))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use axum::Json;
    use axum::extract::State;
    use axum::http::Method;

    use super::*;

    type Requests = Arc<Mutex<Vec<(Method, String, Value)>>>;

    /// A fake Ollama API that records requests; /api/ps lists one loaded model.
    async fn fake_ollama() -> (String, Requests) {
        let requests = Requests::default();
        let record = |requests: Requests, method: Method, path: &str, body: Value| requests.lock().unwrap().push((method, path.into(), body));
        let app = axum::Router::new()
            .route("/api/ps", axum::routing::get(move |State(r): State<Requests>| async move {
                record(r, Method::GET, "/api/ps", Value::Null);
                Json(json!({"models": [{"name": "gemma4:e4b", "size": 1}]}))
            }))
            .route("/api/generate", axum::routing::post(move |State(r): State<Requests>, Json(body): Json<Value>| async move {
                record(r, Method::POST, "/api/generate", body);
                Json(json!({"done": true}))
            }))
            .with_state(requests.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/", listener.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(listener, app).await });
        (url, requests)
    }

    fn client(url: &str) -> OllamaClient {
        OllamaClient::new(url, Duration::from_secs(5))
    }

    #[tokio::test]
    async fn loaded_models() {
        let (url, requests) = fake_ollama().await;

        assert_eq!(client(&url).loaded_models().await, Ok(vec!["gemma4:e4b".into()]));
        assert_eq!(*requests.lock().unwrap(), [(Method::GET, "/api/ps".into(), Value::Null)]);
    }

    #[tokio::test]
    async fn load_keeps_the_model_loaded_and_unload_lets_it_go() {
        let (url, requests) = fake_ollama().await;

        client(&url).load("gemma4:e4b").await.unwrap();
        client(&url).unload("gemma4:e4b").await.unwrap();

        assert_eq!(*requests.lock().unwrap(), [
            (Method::POST, "/api/generate".into(), json!({"model": "gemma4:e4b", "keep_alive": -1})),
            (Method::POST, "/api/generate".into(), json!({"model": "gemma4:e4b", "keep_alive": 0})),
        ]);
    }

    #[tokio::test]
    async fn unreachable() {
        assert!(OllamaClient::new("http://127.0.0.1:1", Duration::from_secs(2)).loaded_models().await.is_err());
    }
}
