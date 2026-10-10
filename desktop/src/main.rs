//! The Saambha desktop app: the dot, floating above the other windows and showing what Saambha is
//! doing, and the chat panel that grows out of it. It runs alongside the Saambha server:
//!     saambha-desktop <server url> <the server's CA file>

mod layer_shell;
mod overlay;
mod server;

use std::os::unix::process::CommandExt;

use reqwest::Method;
use serde_json::{Value, json};
use server::Server;
use tauri::{AppHandle, Manager, State, WebviewUrl, WebviewWindow};

// What the windows ask the server.

/// What Saambha is doing; with [after] (a version of it), once that's changed.
#[tauri::command]
async fn status(server: State<'_, Server>, after: Option<u64>) -> Result<Value, String> {
    match after {
        None => server.call(Method::GET, "/desktop/status", None).await,
        Some(version) => server.call_slow(Method::GET, &format!("/desktop/status?after={version}"), None).await,
    }
}

#[tauri::command]
async fn history(server: State<'_, Server>, limit: u32) -> Result<Value, String> {
    server.call(Method::GET, &format!("/desktop/history?limit={limit}"), None).await
}

#[tauri::command]
async fn log(server: State<'_, Server>, offset: Option<u64>) -> Result<Value, String> {
    let query = offset.map(|o| format!("?offset={o}")).unwrap_or_default();
    server.call(Method::GET, &format!("/desktop/log{query}"), None).await
}

#[tauri::command]
async fn answer_pairing(server: State<'_, Server>, allow: bool) -> Result<Value, String> {
    server.call(Method::POST, "/desktop/pairing", Some(json!({ "allow": allow }))).await
}

#[tauri::command]
async fn stop_speaking(server: State<'_, Server>) -> Result<Value, String> {
    server.call(Method::POST, "/desktop/speech/stop", None).await
}

/// Runs [text] as a command; the reply shows in the history (it isn't spoken).
#[tauri::command]
async fn send_command(server: State<'_, Server>, text: String) -> Result<Value, String> {
    server.call_slow(Method::POST, "/command", Some(json!({ "text": text, "speak": "none" }))).await
}

#[tauri::command]
async fn new_conversation(server: State<'_, Server>) -> Result<Value, String> {
    server.call(Method::POST, "/reset", None).await
}

/// Puts the dot [margins] (left, top, right, bottom) from the stage's edges, and shows it.
#[tauri::command]
fn place_dot(app: AppHandle, margins: overlay::Margins) -> Result<(), String> {
    let dot = app.get_webview_window("dot").ok_or("no dot window")?;
    overlay::place_dot(&dot.gtk_window().map_err(|e| e.to_string())?, margins);
    dot.show().map_err(|e| e.to_string())
}

#[tauri::command]
fn set_input(window: WebviewWindow, parts: Vec<overlay::Rect>, keyboard: bool) -> Result<(), String> {
    overlay::set_input(&window.gtk_window().map_err(|e| e.to_string())?, &parts, keyboard);
    Ok(())
}

#[tauri::command]
fn open_logs(app: AppHandle) -> Result<(), String> {
    if let Some(logs) = app.get_webview_window("logs") {
        return logs.set_focus().map_err(|e| e.to_string());
    }
    tauri::WebviewWindowBuilder::new(&app, "logs", WebviewUrl::App("logs/index.html".into()))
        .title("Saambha logs")
        .inner_size(1100.0, 650.0)
        .build()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

fn main() {
    if overlay::needs_x11() {
        let error = std::process::Command::new(std::env::current_exe().expect("this program's path"))
            .args(std::env::args_os().skip(1))
            .env("GDK_BACKEND", "x11")
            .exec();
        eprintln!("Can't start over under X11: {error}");
        std::process::exit(1);
    }

    let args: Vec<String> = std::env::args().collect();
    let [_, url, ca_file] = args.as_slice() else {
        eprintln!("Usage: saambha-desktop <server url> <the server's CA file>");
        std::process::exit(2);
    };
    let server = Server::new(url, ca_file).unwrap_or_else(|e| {
        eprintln!("{e}");
        std::process::exit(1);
    });

    tauri::Builder::default()
        .manage(server)
        .invoke_handler(tauri::generate_handler![
            status,
            history,
            log,
            answer_pairing,
            stop_speaking,
            send_command,
            new_conversation,
            place_dot,
            set_input,
            open_logs,
        ])
        .setup(|app| {
            let stage = app.get_webview_window("stage").expect("the stage window");
            let dot = app.get_webview_window("dot").expect("the dot window");
            overlay::make_stage(&stage.gtk_window()?);
            overlay::make_dot(&dot.gtk_window()?);
            stage.show()?; // the stage then puts the dot where it goes, and shows it
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("the desktop app failed");
}
