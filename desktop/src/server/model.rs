//! A local (Ollama) model, kept loaded except while something heavy runs, to free the GPU and RAM for it.

pub mod activity;
pub mod ollama;
pub mod warden;
