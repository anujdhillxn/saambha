//! The server: what the phone, watch and hotkey ask Saambha over HTTPS, the agent that does it, and
//! what the dot shows of it. It runs on a tokio runtime beside the UI.

pub mod agent;
pub mod api;
pub mod auth;
pub mod compose;
pub mod history;
pub mod logs;
pub mod model;
pub mod mouse;
pub mod reply;
pub mod services;
pub mod settings;
pub mod status;
pub mod voice;

use std::future::Future;
use std::pin::Pin;

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;
