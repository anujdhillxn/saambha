//! The HTTP API the phone and watch apps, the hotkey and the agent use, served over HTTPS.

use std::net::{IpAddr, SocketAddr};
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::Json;
use axum::Router;
use axum::extract::rejection::JsonRejection;
use axum::extract::ws::{CloseFrame, Message, WebSocket, WebSocketUpgrade};
use axum::extract::{ConnectInfo, Multipart, Request, State};
use axum::http::StatusCode;
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use hyper_util::rt::TokioIo;
use hyper_util::service::TowerToHyperService;
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpListener;
use tokio_rustls::TlsAcceptor;
use tokio_rustls::rustls::ServerConfig;
use tokio_rustls::rustls::pki_types::pem::PemObject;
use tokio_rustls::rustls::pki_types::{CertificateDer, PrivateKeyDer};
use tower::ServiceExt;

use crate::server::auth::{Refused, bearer_token, is_local};
use crate::server::mouse::play;
use crate::server::services::{Services, VoiceError};

type Shared = State<Arc<Services>>;

/// Reachable without a token: the apps' LAN scan and the pairing request.
const PUBLIC_PATHS: [&str; 2] = ["/discover", "/pair"];

pub fn router(services: Arc<Services>) -> Router {
    Router::new()
        .route("/discover", get(discover))
        .route("/pair", post(pair))
        .route("/listen", post(listen).delete(stop_listening))
        .route("/voice", post(voice))
        .route("/command", post(command))
        .route("/reset", post(reset))
        .route("/status", get(status))
        .route("/mouse", get(touchpad))
        .fallback(|| async { error(StatusCode::NOT_FOUND, "Not Found") })
        .layer(middleware::from_fn_with_state(services.clone(), require_paired_device))
        .layer(middleware::from_fn(log_request))
        .with_state(services)
}

/// A request turned away: its status, and why.
struct Refusal(StatusCode, String);

impl IntoResponse for Refusal {
    fn into_response(self) -> Response {
        (self.0, Json(json!({ "detail": self.1 }))).into_response()
    }
}

fn error(status: StatusCode, detail: &str) -> Refusal {
    Refusal(status, detail.into())
}

/// Where a request comes from, as serve_connection says; IPv4 as IPv4, not mapped into IPv6.
fn peer(request: &Request) -> IpAddr {
    request.extensions().get::<ConnectInfo<SocketAddr>>().map_or(IpAddr::from([0, 0, 0, 0]), |c| c.0.ip().to_canonical())
}

async fn require_paired_device(State(s): Shared, request: Request, next: Next) -> Response {
    let token = bearer_token(request.headers().get("authorization").and_then(|v| v.to_str().ok()));
    if is_local(peer(&request)) || PUBLIC_PATHS.contains(&request.uri().path()) || s.devices.is_valid(token) {
        return next.run(request).await;
    }
    error(StatusCode::UNAUTHORIZED, "Not paired").into_response()
}

async fn log_request(request: Request, next: Next) -> Response {
    let line = format!("{} - \"{} {}\"", peer(&request), request.method(), request.uri());
    let response = next.run(request).await;
    log::info!("{line} {}", response.status().as_u16());
    response
}

/// A JSON body, or 422 saying what's wrong with it.
fn body<T>(body: Result<Json<T>, JsonRejection>) -> Result<T, Refusal> {
    body.map(|Json(body)| body).map_err(|e| error(StatusCode::UNPROCESSABLE_ENTITY, &e.body_text()))
}

/// Identifies this server to the apps' LAN scan.
async fn discover(State(s): Shared) -> Json<Value> {
    Json(json!({ "service": "saambha", "name": s.hostname }))
}

#[derive(Deserialize)]
struct PairRequest {
    #[serde(default = "unknown_device")]
    name: String,
}

fn unknown_device() -> String {
    "Unknown device".into()
}

/// Asks on the desktop whether to allow this device (waiting up to a minute for the answer); its token
/// if allowed.
async fn pair(State(s): Shared, ConnectInfo(from): ConnectInfo<SocketAddr>, request: Result<Json<PairRequest>, JsonRejection>)
              -> Result<Json<Value>, Refusal> {
    let request = body(request)?;
    match s.pairing.request(&from.ip().to_canonical().to_string(), request.name.trim()).await {
        Ok(token) => Ok(Json(json!({ "token": token, "name": s.hostname }))),
        Err(Refused::Busy) => Err(error(StatusCode::TOO_MANY_REQUESTS, "Another device is awaiting approval")),
        Err(Refused::Denied) => Err(error(StatusCode::FORBIDDEN, "Denied on the desktop")),
    }
}

/// The hotkey started recording: cut off the narration and show that it's listening.
async fn listen(State(s): Shared) -> Json<Value> {
    s.listen(true);
    Json(json!({ "ok": true }))
}

/// The hotkey stopped recording without anything to send.
async fn stop_listening(State(s): Shared) -> Json<Value> {
    s.listen(false);
    Json(json!({ "ok": true }))
}

// Commands run in a task of their own, which carries on if the client gives up waiting: the agent's
// work and what the dot shows of it shouldn't be cut off halfway.

/// The hotkey's recording, as the "audio" file of a form.
async fn voice(State(s): Shared, mut form: Multipart) -> Result<Json<Value>, Refusal> {
    let mut audio = None;
    while let Ok(Some(field)) = form.next_field().await {
        if field.name() == Some("audio") {
            let suffix = field.file_name().and_then(|name| Path::new(name).extension()).map_or(".wav".into(), |e| format!(".{}", e.to_string_lossy()));
            audio = field.bytes().await.ok().map(|bytes| (bytes, suffix));
        }
    }
    let (audio, suffix) = audio.ok_or_else(|| error(StatusCode::UNPROCESSABLE_ENTITY, "No audio file"))?;
    let heard = tokio::spawn(async move { s.voice(&audio, &suffix).await }).await.map_err(|_| error(StatusCode::INTERNAL_SERVER_ERROR, "The command failed"))?;
    match heard {
        Ok((text, reply)) => Ok(Json(json!({ "input": text, "text": reply.summary }))),
        Err(VoiceError::Unreadable) => Err(error(StatusCode::BAD_REQUEST, "Could not read the recording")),
        Err(VoiceError::Silent) => Err(error(StatusCode::BAD_REQUEST, "Could not transcribe audio")),
    }
}

#[derive(Deserialize)]
struct CommandRequest {
    text: String,
    /// "desktop": speak the reply here; "none": the app speaks it.
    #[serde(default)]
    speak: Option<String>,
}

/// A text command from a client that does its own speech recognition (the apps).
async fn command(State(s): Shared, request: Result<Json<CommandRequest>, JsonRejection>) -> Result<Json<Value>, Refusal> {
    let request = body(request)?;
    let text = request.text.trim().to_string();
    if text.is_empty() {
        return Err(error(StatusCode::BAD_REQUEST, "Empty command"));
    }
    s.speaker.stop();
    let speak = request.speak.as_deref().unwrap_or("desktop") == "desktop";
    let input = text.clone();
    let reply = tokio::spawn(async move { s.run_command(&text, speak).await }).await.map_err(|_| error(StatusCode::INTERNAL_SERVER_ERROR, "The command failed"))?;
    Ok(Json(json!({ "input": input, "text": reply.summary })))
}

async fn reset(State(s): Shared) -> Json<Value> {
    s.new_conversation();
    Json(json!({ "ok": true, "message": "Session reset" }))
}

async fn status(State(s): Shared) -> Json<Value> {
    Json(json!({ "ok": true, "has_session": s.assistant.has_session() }))
}

/// The apps' touchpad: moves, clicks and scrolls a mouse on the desktop while it's connected. Each
/// message says when it happened on the device ("t", in ms), so it's played at that pace.
async fn touchpad(State(s): Shared, ConnectInfo(from): ConnectInfo<SocketAddr>, upgrade: WebSocketUpgrade) -> Response {
    upgrade.on_upgrade(move |socket| move_the_mouse(s, from.ip().to_canonical(), socket))
}

/// [error]'s description, without the "(os error 13)".
fn describe(error: &std::io::Error) -> String {
    let text = error.to_string();
    text.split(" (os error").next().unwrap_or(&text).to_string()
}

async fn move_the_mouse(s: Arc<Services>, from: IpAddr, mut socket: WebSocket) {
    let mut mouse = match (s.mouse)() {
        Ok(mouse) => mouse,
        Err(e) => {
            log::warn!("MOUSE: can't make a mouse: {e}");
            let close = CloseFrame { code: 1011, reason: format!("Can't control the mouse: {}", describe(&e)).into() };
            let _ = socket.send(Message::Close(Some(close))).await;
            return;
        }
    };
    log::info!("MOUSE: {from} connected");
    let start = Instant::now();
    let mut pacer = (s.pacer)();
    let (moves, mut to_play) = tokio::sync::mpsc::unbounded_channel::<(f64, Value)>();
    let receiving = tokio::spawn(async move {
        while let Some(Ok(message)) = socket.recv().await {
            let Message::Text(text) = message else { continue };
            let Ok(message) = serde_json::from_str::<Value>(&text) else { continue };
            let arrived = start.elapsed().as_secs_f64();
            let play_at = message["t"].as_f64().map_or(arrived, |happened| pacer.play_at(happened / 1000.0, arrived));
            let _ = moves.send((play_at, message));
        }
    });
    // What's left to play when it disconnects is played first.
    while let Some((play_at, message)) = to_play.recv().await {
        tokio::time::sleep_until((start + Duration::from_secs_f64(play_at.max(0.0))).into()).await;
        if let Err(e) = play(mouse.as_mut(), &message) {
            log::warn!("MOUSE: {e}");
        }
    }
    let _ = receiving.await;
    log::info!("MOUSE: {from} disconnected");
}

/// HTTPS with the server's certificate and key (PEM files).
pub fn tls(cert_file: &Path, key_file: &Path) -> Result<TlsAcceptor, String> {
    let certs = CertificateDer::pem_file_iter(cert_file).and_then(Iterator::collect::<Result<Vec<_>, _>>)
        .map_err(|e| format!("Can't read the certificate {}: {e}", cert_file.display()))?;
    let key = PrivateKeyDer::from_pem_file(key_file).map_err(|e| format!("Can't read the key {}: {e}", key_file.display()))?;
    let provider = Arc::new(tokio_rustls::rustls::crypto::ring::default_provider());
    let config = ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .and_then(|config| config.with_no_client_auth().with_single_cert(certs, key))
        .map_err(|e| format!("Can't serve HTTPS: {e}"))?;
    Ok(TlsAcceptor::from(Arc::new(config)))
}

/// Serves [app] over HTTPS on [listener], each connection in a task of its own.
pub async fn serve(listener: TcpListener, tls: TlsAcceptor, app: Router) {
    loop {
        let (tcp, from) = match listener.accept().await {
            Ok(connection) => connection,
            Err(e) => {
                log::warn!("Can't take a connection: {e}");
                tokio::time::sleep(Duration::from_millis(100)).await;
                continue;
            }
        };
        let (tls, app) = (tls.clone(), app.clone());
        tokio::spawn(async move {
            // A client that never finishes the handshake (or doesn't trust our CA) is let go.
            if let Ok(Ok(stream)) = tokio::time::timeout(Duration::from_secs(10), tls.accept(tcp)).await {
                serve_connection(stream, from, app).await;
            }
        });
    }
}

/// Serves [app] on one connection from [from], WebSockets included.
pub async fn serve_connection(io: impl AsyncRead + AsyncWrite + Unpin + Send + 'static, from: SocketAddr, app: Router) {
    let app = app.map_request(move |mut request: hyper::Request<hyper::body::Incoming>| {
        request.extensions_mut().insert(ConnectInfo(from));
        request
    });
    let served = hyper::server::conn::http1::Builder::new()
        .serve_connection(TokioIo::new(io), TowerToHyperService::new(app))
        .with_upgrades()
        .await;
    if let Err(e) = served {
        log::debug!("Connection from {from} ended: {e}");
    }
}

#[cfg(test)]
pub mod tests {
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};

    use axum::body::Body;
    use axum::http::Method;

    use super::*;
    use crate::server::BoxFuture;
    use crate::server::agent::{Assistant, Event, EventHandler};
    use crate::server::auth::{Ask, DeviceRegistry, Pairing};
    use crate::server::history::ChatHistory;
    use crate::server::logs::LogFile;
    use crate::server::mouse::{Mouse, Pacer};
    use crate::server::status::Status;
    use crate::server::voice::narrator::Speaker;
    use crate::server::voice::whisper::Transcriber;

    const LAN: [u8; 4] = [192, 168, 0, 12];

    #[derive(Default)]
    pub struct FakeAssistant {
        pub asked: Mutex<Vec<String>>,
        session: AtomicBool,
        /// Else "You said: ...".
        pub reply: Mutex<Option<String>>,
        /// What the dot showed while it worked.
        pub tasks: Mutex<Vec<Option<String>>>,
        pub status: Mutex<Option<Arc<Status>>>,
    }

    impl Assistant for FakeAssistant {
        fn ask<'a>(&'a self, text: &'a str, on_event: EventHandler<'a>) -> BoxFuture<'a, String> {
            Box::pin(async move {
                let shown = || self.status.lock().unwrap().as_ref().map(|s| s.activity().task);
                self.tasks.lock().unwrap().extend(shown());
                self.asked.lock().unwrap().push(text.into());
                self.session.store(true, Ordering::SeqCst);
                on_event(Event::ToolUse("skill spotify".into()));
                self.tasks.lock().unwrap().extend(shown());
                on_event(Event::Text(format!("You said: {text}")));
                self.reply.lock().unwrap().clone().unwrap_or(format!("You said: {text}"))
            })
        }

        fn reset(&self) {
            self.session.store(false, Ordering::SeqCst);
        }

        fn has_session(&self) -> bool {
            self.session.load(Ordering::SeqCst)
        }

        fn busy(&self) -> bool {
            false
        }
    }

    #[derive(Default)]
    pub struct FakeSpeaker {
        pub spoken: Mutex<Vec<String>>,
        pub stops: AtomicUsize,
    }

    impl Speaker for FakeSpeaker {
        fn speak(&self, text: &str) {
            self.spoken.lock().unwrap().push(text.into());
        }

        fn stop(&self) {
            self.stops.fetch_add(1, Ordering::SeqCst);
        }
    }

    pub struct FakeTranscriber {
        pub result: Arc<Mutex<Result<String, String>>>,
        pub uploads: Arc<Mutex<Vec<Vec<u8>>>>,
    }

    impl Transcriber for FakeTranscriber {
        fn transcribe<'a>(&'a self, audio: &'a Path) -> BoxFuture<'a, Result<String, String>> {
            Box::pin(async move {
                self.uploads.lock().unwrap().push(std::fs::read(audio).unwrap());
                self.result.lock().unwrap().clone()
            })
        }
    }

    /// What the touchpad's mouse was told to do, whether it's on the desktop, and the error it fails with.
    #[derive(Default)]
    pub struct Desk {
        pub done: Mutex<Vec<(String, Instant)>>,
        pub there: AtomicBool,
        pub error: Mutex<Option<i32>>,
    }

    struct FakeMouse(Arc<Desk>);

    impl FakeMouse {
        fn did(&self, what: String) -> std::io::Result<()> {
            self.0.done.lock().unwrap().push((what, Instant::now()));
            Ok(())
        }
    }

    impl Mouse for FakeMouse {
        fn move_by(&mut self, dx: i32, dy: i32) -> std::io::Result<()> {
            self.did(format!("move {dx} {dy}"))
        }

        fn click(&mut self, right: bool) -> std::io::Result<()> {
            self.did(format!("click {}", if right { "right" } else { "left" }))
        }

        fn scroll(&mut self, notches: i32) -> std::io::Result<()> {
            self.did(format!("scroll {notches}"))
        }
    }

    impl Drop for FakeMouse {
        fn drop(&mut self) {
            self.0.there.store(false, Ordering::SeqCst);
        }
    }

    impl Desk {
        pub fn done(&self) -> Vec<String> {
            self.done.lock().unwrap().iter().map(|(what, _)| what.clone()).collect()
        }
    }

    pub struct World {
        pub services: Arc<Services>,
        pub assistant: Arc<FakeAssistant>,
        pub speaker: Arc<FakeSpeaker>,
        pub transcript: Arc<Mutex<Result<String, String>>>,
        pub uploads: Arc<Mutex<Vec<Vec<u8>>>>,
        pub desk: Arc<Desk>,
        /// Whether the desktop allows a device, and the devices it was asked about.
        pub allow: Arc<AtomicBool>,
        pub asked: Arc<Mutex<Vec<(String, String)>>>,
        pub now: Arc<AtomicU64>,
        /// What the dot showed each time the history was written.
        pub shown_when_kept: Arc<Mutex<Vec<Option<String>>>>,
        pub dir: tempfile::TempDir,
    }

    pub fn world() -> World {
        let dir = tempfile::tempdir().unwrap();
        let now = Arc::new(AtomicU64::new(1_000_000));
        let clock = { let now = now.clone(); move || now.load(Ordering::SeqCst) as f64 };
        let devices = Arc::new(DeviceRegistry::new(dir.path().join("devices.json")));
        let (allow, asked) = (Arc::new(AtomicBool::new(true)), Arc::new(Mutex::new(Vec::new())));
        let ask: Ask = { let (allow, asked) = (allow.clone(), asked.clone()); Box::new(move |ip, name| {
            asked.lock().unwrap().push((ip, name));
            let allowed = allow.load(Ordering::SeqCst);
            Box::pin(async move { allowed })
        }) };
        let (assistant, speaker, desk) = (Arc::new(FakeAssistant::default()), Arc::new(FakeSpeaker::default()), Arc::new(Desk::default()));
        let (transcript, uploads) = (Arc::new(Mutex::new(Ok("what time is it".to_string()))), Arc::default());
        let status = Arc::new(Status::new(Duration::from_secs(5)));
        *assistant.status.lock().unwrap() = Some(status.clone());
        let mouse_desk = desk.clone();
        let shown_when_kept = Arc::new(Mutex::new(Vec::new()));
        let history_clock = { let (status, shown, clock) = (status.clone(), shown_when_kept.clone(), clock.clone()); move || {
            shown.lock().unwrap().push(status.activity().task);
            clock()
        } };
        let services = Arc::new(Services {
            assistant: assistant.clone(),
            devices: devices.clone(),
            pairing: Pairing::new(devices, ask, 300.0, clock.clone()),
            speaker: speaker.clone(),
            status,
            history: ChatHistory::new(dir.path().join("history.jsonl"), history_clock),
            log: LogFile::new(dir.path().join("saambha.log"), 1000),
            transcriber: Box::new(FakeTranscriber { result: transcript.clone(), uploads: Arc::clone(&uploads) }),
            mouse: Box::new(move || {
                if let Some(code) = *mouse_desk.error.lock().unwrap() {
                    return Err(std::io::Error::from_raw_os_error(code));
                }
                mouse_desk.there.store(true, Ordering::SeqCst);
                Ok(Box::new(FakeMouse(mouse_desk.clone())))
            }),
            pacer: Box::new(|| Pacer::new(1.0, 0.15)),
            hostname: "desktop".into(),
        });
        World { services, assistant, speaker, transcript, uploads, desk, allow, asked, now, shown_when_kept, dir }
    }

    /// A request as from [from], with [token] if given: its status and JSON reply.
    pub async fn call(w: &World, method: Method, path: &str, body: Option<Value>, from: IpAddr, token: Option<&str>) -> (StatusCode, Value) {
        let mut request = Request::builder().method(method).uri(path);
        if let Some(token) = token {
            request = request.header("authorization", format!("Bearer {token}"));
        }
        let body = match body {
            Some(body) => {
                request = request.header("content-type", "application/json");
                Body::from(body.to_string())
            }
            None => Body::empty(),
        };
        let mut request = request.body(body).unwrap();
        request.extensions_mut().insert(ConnectInfo(SocketAddr::new(from, 50000)));
        respond(router(w.services.clone()).oneshot(request).await.unwrap()).await
    }

    async fn respond(response: Response) -> (StatusCode, Value) {
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
        (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
    }

    fn lan() -> IpAddr {
        IpAddr::from(LAN)
    }

    fn local() -> IpAddr {
        IpAddr::from([127, 0, 0, 1])
    }

    /// A device on the LAN, paired: its token.
    fn paired(w: &World) -> String {
        w.services.devices.add("Phone", "192.168.0.12").unwrap()
    }

    #[tokio::test]
    async fn discover_identifies_the_server_without_a_token() {
        let w = world();

        assert_eq!(call(&w, Method::GET, "/discover", None, lan(), None).await, (StatusCode::OK, json!({"service": "saambha", "name": "desktop"})));
    }

    #[tokio::test]
    async fn an_unpaired_device_is_turned_away() {
        let w = world();
        for (method, path) in [(Method::GET, "/status"), (Method::POST, "/command"), (Method::POST, "/voice"), (Method::POST, "/reset"),
                               (Method::POST, "/listen"), (Method::DELETE, "/listen"), (Method::GET, "/mouse"), (Method::GET, "/docs")] {
            assert_eq!(call(&w, method.clone(), path, None, lan(), None).await.0, StatusCode::UNAUTHORIZED, "{method} {path}");
        }
        assert_eq!(call(&w, Method::GET, "/status", None, lan(), Some("nope")).await.0, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn this_machine_needs_no_token() {
        let w = world();

        assert_eq!(call(&w, Method::GET, "/status", None, local(), None).await.0, StatusCode::OK);
        assert_eq!(call(&w, Method::GET, "/status", None, "::ffff:127.0.0.1".parse().unwrap(), None).await.0, StatusCode::OK);
    }

    #[tokio::test]
    async fn pairing_returns_a_working_token_when_allowed() {
        let w = world();

        let (status, reply) = call(&w, Method::POST, "/pair", Some(json!({"name": " Pixel Watch 4 "})), lan(), None).await;

        assert_eq!((status, &reply["name"]), (StatusCode::OK, &json!("desktop")));
        let token = reply["token"].as_str().unwrap();
        assert_eq!(call(&w, Method::GET, "/status", None, lan(), Some(token)).await.0, StatusCode::OK);
        assert_eq!(*w.asked.lock().unwrap(), [("192.168.0.12".to_string(), "Pixel Watch 4".to_string())]);
    }

    #[tokio::test]
    async fn pairing_without_a_name_says_unknown_device() {
        let w = world();

        call(&w, Method::POST, "/pair", Some(json!({})), "::ffff:192.168.0.12".parse().unwrap(), None).await;

        assert_eq!(*w.asked.lock().unwrap(), [("192.168.0.12".to_string(), "Unknown device".to_string())]);
    }

    #[tokio::test]
    async fn pairing_denied() {
        let w = world();
        w.allow.store(false, Ordering::SeqCst);

        let (status, reply) = call(&w, Method::POST, "/pair", Some(json!({"name": "Phone"})), lan(), None).await;

        assert_eq!((status, reply), (StatusCode::FORBIDDEN, json!({"detail": "Denied on the desktop"})));
    }

    #[tokio::test]
    async fn a_command_replies_and_speaks_on_the_desktop() {
        let w = world();
        let token = paired(&w);

        let reply = call(&w, Method::POST, "/command", Some(json!({"text": "  play music  "})), lan(), Some(&token)).await;

        assert_eq!(reply, (StatusCode::OK, json!({"input": "play music", "text": "You said: play music"})));
        assert_eq!(*w.assistant.asked.lock().unwrap(), ["play music"]);
        assert_eq!(*w.speaker.spoken.lock().unwrap(), ["You said: play music"]);
        assert_eq!(w.speaker.stops.load(Ordering::SeqCst), 1); // cuts off the previous reply first
        let history = w.services.history.recent(10);
        assert_eq!((history[0].said.as_str(), history[0].reply.as_str()), ("play music", "You said: play music"));
    }

    #[tokio::test]
    async fn only_the_summary_is_spoken_and_the_details_stay_in_the_chat() {
        let w = world();
        *w.assistant.reply.lock().unwrap() = Some("Merge keeps both. More in the details.<details>The long version.</details>".into());

        let (_, reply) = call(&w, Method::POST, "/command", Some(json!({"text": "merge or rebase"})), local(), None).await;

        assert_eq!(reply["text"], "Merge keeps both. More in the details.");
        assert_eq!(*w.speaker.spoken.lock().unwrap(), ["Merge keeps both. More in the details."]);
        assert_eq!(w.services.history.recent(10)[0].details, "The long version.");
    }

    #[tokio::test]
    async fn a_command_for_the_phone_isnt_spoken_on_the_desktop() {
        let w = world();

        let (_, reply) = call(&w, Method::POST, "/command", Some(json!({"text": "play music", "speak": "none"})), local(), None).await;

        assert_eq!(reply["text"], "You said: play music");
        assert!(w.speaker.spoken.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn the_dot_shows_what_the_agent_is_doing_until_the_history_has_the_reply() {
        let w = world();

        call(&w, Method::POST, "/command", Some(json!({"text": "play music"})), local(), None).await;

        assert_eq!(*w.assistant.tasks.lock().unwrap(), [Some("Thinking".into()), Some("Running skill spotify".into())]);
        // so the panel, told the dot is done, shows the reply
        assert_eq!(*w.shown_when_kept.lock().unwrap(), [Some("Running skill spotify".into())]);
        assert_eq!(w.services.status.activity().task, None);
    }

    #[tokio::test]
    async fn an_empty_or_malformed_command_is_turned_away() {
        let w = world();

        assert_eq!(call(&w, Method::POST, "/command", Some(json!({"text": "   "})), local(), None).await.0, StatusCode::BAD_REQUEST);
        assert_eq!(call(&w, Method::POST, "/command", Some(json!({})), local(), None).await.0, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(call(&w, Method::POST, "/command", None, local(), None).await.0, StatusCode::UNPROCESSABLE_ENTITY);
        assert!(w.assistant.asked.lock().unwrap().is_empty());
    }

    /// A /voice request with [audio] as its "audio" file: its status and JSON reply.
    async fn voice(w: &World, audio: &[u8]) -> (StatusCode, Value) {
        let boundary = "saambha-test";
        let mut body = format!("--{boundary}\r\nContent-Disposition: form-data; name=\"audio\"; filename=\"rec.wav\"\r\nContent-Type: audio/wav\r\n\r\n").into_bytes();
        body.extend_from_slice(audio);
        body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
        let mut request = Request::builder().method(Method::POST).uri("/voice")
            .header("content-type", format!("multipart/form-data; boundary={boundary}"))
            .body(Body::from(body)).unwrap();
        request.extensions_mut().insert(ConnectInfo(SocketAddr::new(local(), 50000)));
        respond(router(w.services.clone()).oneshot(request).await.unwrap()).await
    }

    #[tokio::test]
    async fn the_hotkeys_recording_is_transcribed_and_run() {
        let w = world();
        w.services.status.set_listening(true);

        let (_, reply) = voice(&w, b"RIFF-audio").await;

        assert_eq!(reply, json!({"input": "what time is it", "text": "You said: what time is it"}));
        assert_eq!(*w.uploads.lock().unwrap(), [b"RIFF-audio".to_vec()]);
        assert_eq!(*w.speaker.spoken.lock().unwrap(), ["You said: what time is it"]);
        assert_eq!(w.speaker.stops.load(Ordering::SeqCst), 1);
        assert!(!w.services.status.activity().listening);
    }

    #[tokio::test]
    async fn a_recording_that_cant_be_read_or_says_nothing() {
        let w = world();

        *w.transcript.lock().unwrap() = Err("bad audio".into());
        assert_eq!(voice(&w, b"").await, (StatusCode::BAD_REQUEST, json!({"detail": "Could not read the recording"})));
        *w.transcript.lock().unwrap() = Ok(String::new());
        assert_eq!(voice(&w, b"RIFF").await, (StatusCode::BAD_REQUEST, json!({"detail": "Could not transcribe audio"})));
        assert!(w.assistant.asked.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn listening_cuts_off_the_narration_until_its_done() {
        let w = world();

        assert_eq!(call(&w, Method::POST, "/listen", None, local(), None).await.0, StatusCode::OK);
        assert!(w.services.status.activity().listening);
        assert_eq!(w.speaker.stops.load(Ordering::SeqCst), 1);

        assert_eq!(call(&w, Method::DELETE, "/listen", None, local(), None).await.0, StatusCode::OK);
        assert!(!w.services.status.activity().listening);
    }

    #[tokio::test]
    async fn status_and_reset() {
        let w = world();
        w.now.store(1_000_000, Ordering::SeqCst);
        call(&w, Method::POST, "/command", Some(json!({"text": "hi", "speak": "none"})), local(), None).await;
        assert_eq!(call(&w, Method::GET, "/status", None, local(), None).await.1["has_session"], true);
        w.now.store(1_000_005, Ordering::SeqCst);

        call(&w, Method::POST, "/reset", None, local(), None).await;

        assert_eq!(call(&w, Method::GET, "/status", None, local(), None).await.1["has_session"], false);
        let history = w.services.history.recent(10);
        assert!(history.last().unwrap().new_conversation && history.last().unwrap().time == 1_000_005.0);
    }

    /// Serves the API over plain TCP, every connection as from [from]: its address.
    pub async fn serve_as(w: &World, from: IpAddr) -> SocketAddr {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let app = router(w.services.clone());
        tokio::spawn(async move {
            while let Ok((tcp, _)) = listener.accept().await {
                tokio::spawn(serve_connection(tcp, SocketAddr::new(from, 50000), app.clone()));
            }
        });
        address
    }

    type Socket = tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

    async fn touchpad(address: SocketAddr, token: Option<&str>) -> Result<Socket, tokio_tungstenite::tungstenite::Error> {
        use tokio_tungstenite::tungstenite::client::IntoClientRequest;
        let mut request = format!("ws://{address}/mouse").into_client_request().unwrap();
        if let Some(token) = token {
            request.headers_mut().insert("authorization", format!("Bearer {token}").parse().unwrap());
        }
        // Without Nagle's algorithm holding small writes back, as the apps open it.
        tokio_tungstenite::connect_async_with_config(request, None, true).await.map(|(socket, _)| socket)
    }

    async fn send(socket: &mut Socket, message: Value) {
        use futures_util::SinkExt;
        socket.send(tokio_tungstenite::tungstenite::Message::text(message.to_string())).await.unwrap();
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

    #[tokio::test]
    async fn the_touchpad_moves_clicks_and_scrolls_the_mouse_while_its_there() {
        let w = world();
        let token = paired(&w);
        let mut socket = touchpad(serve_as(&w, lan()).await, Some(&token)).await.unwrap();

        for message in [json!({"type": "move", "dx": 12, "dy": -3, "t": 1000}), json!({"type": "click", "button": "left", "t": 1010}),
                        json!({"type": "click", "button": "right", "t": 1020}), json!({"type": "scroll", "notches": -2, "t": 1030}),
                        json!({"type": "teleport"}), json!({"type": "move", "dx": "far"})] {
            send(&mut socket, message).await;
        }
        assert!(wait_until(|| w.desk.done().len() == 4).await);
        assert!(w.desk.there.load(Ordering::SeqCst));
        drop(socket);

        assert!(wait_until(|| !w.desk.there.load(Ordering::SeqCst)).await);
        assert_eq!(w.desk.done(), ["move 12 -3", "click left", "click right", "scroll -2"]);
    }

    #[tokio::test]
    async fn bunched_moves_are_played_at_the_pace_they_happened() {
        let w = world();
        let mut socket = touchpad(serve_as(&w, local()).await, None).await.unwrap();

        for t in (0..100).step_by(20) {
            // sent together, though they happened 20ms apart
            send(&mut socket, json!({"type": "move", "dx": 1, "dy": 0, "t": t})).await;
        }
        assert!(wait_until(|| w.desk.done().len() == 5).await);

        let played: Vec<Instant> = w.desk.done.lock().unwrap().iter().map(|(_, at)| *at).collect();
        let gaps: Vec<u128> = played.windows(2).map(|pair| (pair[1] - pair[0]).as_millis()).collect();
        assert!(gaps.iter().all(|gap| (18..=30).contains(gap)), "{gaps:?}");
    }

    #[tokio::test]
    async fn the_touchpad_needs_a_paired_device() {
        let w = world();

        assert!(touchpad(serve_as(&w, lan()).await, Some("nope")).await.is_err());
        assert!(w.desk.done().is_empty());
    }

    #[tokio::test]
    async fn the_touchpad_says_why_there_is_no_mouse() {
        use futures_util::StreamExt;
        let w = world();
        *w.desk.error.lock().unwrap() = Some(libc::EACCES);
        let mut socket = touchpad(serve_as(&w, local()).await, None).await.unwrap();

        let closed = socket.next().await.unwrap().unwrap();

        let tokio_tungstenite::tungstenite::Message::Close(Some(frame)) = closed else { panic!("{closed:?}") };
        assert_eq!((u16::from(frame.code), frame.reason.as_str()), (1011, "Can't control the mouse: Permission denied"));
    }
}
