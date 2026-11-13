//! Keeps the local model loaded, except while something heavy runs (to free the GPU and RAM for it).

use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::server::model::ollama::Ollama;

/// Every interval: while [heavy_task] reports something, unload [model] (unless a command is using it,
/// see [busy]); once nothing heavy has run for [resume_after] seconds, load it again and keep it loaded.
/// Says through [on_cold] why the model isn't loaded, or None once it is.
pub struct ModelWarden {
    ollama: Arc<dyn Ollama>,
    model: String,
    /// Blocking: it may run a program (nvidia-smi).
    heavy_task: Arc<dyn Fn() -> Option<String> + Send + Sync>,
    busy: Box<dyn Fn() -> bool + Send + Sync>,
    on_cold: Box<dyn Fn(Option<String>) + Send + Sync>,
    resume_after: f64,
    /// Seconds, from any start.
    clock: Box<dyn Fn() -> f64 + Send + Sync>,
    /// Why the model isn't loaded, and when something heavy last ran.
    state: Mutex<(Option<String>, f64)>,
}

impl ModelWarden {
    #[allow(clippy::too_many_arguments)]
    pub fn new(ollama: Arc<dyn Ollama>, model: &str, heavy_task: Arc<dyn Fn() -> Option<String> + Send + Sync>,
               busy: impl Fn() -> bool + Send + Sync + 'static, on_cold: impl Fn(Option<String>) + Send + Sync + 'static,
               resume_after: f64, clock: impl Fn() -> f64 + Send + Sync + 'static) -> ModelWarden {
        ModelWarden {
            ollama, model: model.into(), heavy_task, busy: Box::new(busy), on_cold: Box::new(on_cold), resume_after,
            clock: Box::new(clock), state: Mutex::new((None, f64::NEG_INFINITY)),
        }
    }

    /// One look at what the user is doing, and the model loaded or unloaded to match.
    pub async fn check(&self) {
        if let Err(e) = self.try_check().await {
            self.set_cold(Some("Ollama isn't responding".into()), &e);
        }
    }

    async fn try_check(&self) -> Result<(), String> {
        let heavy_task = self.heavy_task.clone();
        let heavy = tokio::task::spawn_blocking(move || heavy_task()).await.map_err(|e| e.to_string())?;
        let now = (self.clock)();
        if let Some(heavy) = heavy {
            self.state.lock().unwrap().1 = now;
            if (self.busy)() {
                return Ok(()); // a command is running: leave the model alone until it's done
            }
            if self.ollama.loaded_models().await?.contains(&self.model) {
                self.ollama.unload(&self.model).await?;
            }
            self.set_cold(Some(format!("Paused: {heavy}")), "");
        } else if now - self.state.lock().unwrap().1 >= self.resume_after {
            if !self.ollama.loaded_models().await?.contains(&self.model) {
                self.set_cold(Some("Loading the model".into()), "");
            }
            // Every time, not only when it wasn't loaded: commands load the model with Ollama's default
            // keep-alive, which would unload it again after a few idle minutes.
            self.ollama.load(&self.model).await?;
            self.set_cold(None, "");
        }
        Ok(())
    }

    fn set_cold(&self, reason: Option<String>, detail: &str) {
        let mut state = self.state.lock().unwrap();
        if reason == state.0 {
            return;
        }
        match (&reason, detail) {
            (Some(reason), detail) if !detail.is_empty() => log::warn!("{}: {reason} ({detail})", self.model),
            (reason, _) => log::info!("{}: {}", self.model, reason.as_deref().unwrap_or("loaded")),
        }
        state.0 = reason.clone();
        (self.on_cold)(reason);
    }

    /// Checks every [interval] from now on.
    pub fn start(self: Arc<Self>, interval: Duration) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            loop {
                self.check().await;
                tokio::time::sleep(interval).await;
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::sync::atomic::{AtomicBool, Ordering};

    use super::*;
    use crate::server::BoxFuture;

    #[derive(Default)]
    struct FakeOllama {
        loaded: Mutex<BTreeSet<String>>,
        calls: Mutex<Vec<String>>,
        down: AtomicBool,
    }

    impl FakeOllama {
        fn up(&self) -> Result<(), String> {
            if self.down.load(Ordering::SeqCst) { Err("ollama is down".into()) } else { Ok(()) }
        }

        fn loaded(&self) -> Vec<String> {
            self.loaded.lock().unwrap().iter().cloned().collect()
        }
    }

    impl Ollama for FakeOllama {
        fn loaded_models(&self) -> BoxFuture<'_, Result<Vec<String>, String>> {
            Box::pin(async move { self.up().map(|_| self.loaded()) })
        }

        fn load<'a>(&'a self, model: &'a str) -> BoxFuture<'a, Result<(), String>> {
            Box::pin(async move {
                self.up()?;
                self.calls.lock().unwrap().push(format!("load {model}"));
                self.loaded.lock().unwrap().insert(model.into());
                Ok(())
            })
        }

        fn unload<'a>(&'a self, model: &'a str) -> BoxFuture<'a, Result<(), String>> {
            Box::pin(async move {
                self.up()?;
                self.calls.lock().unwrap().push(format!("unload {model}"));
                self.loaded.lock().unwrap().remove(model);
                Ok(())
            })
        }
    }

    /// What the warden sees: the heavy task (if any), whether a command runs, and the time; and what
    /// on_cold was told.
    #[derive(Default)]
    struct World {
        heavy: Mutex<Option<String>>,
        busy: AtomicBool,
        now: Mutex<f64>,
        cold: Mutex<Vec<Option<String>>>,
    }

    impl World {
        fn heavy(&self, task: Option<&str>) {
            *self.heavy.lock().unwrap() = task.map(String::from);
        }

        fn later(&self, seconds: f64) {
            *self.now.lock().unwrap() += seconds;
        }

        fn cold(&self) -> Vec<Option<String>> {
            self.cold.lock().unwrap().clone()
        }
    }

    const GAME: &str = "Cyberpunk2077.ex is using 6.0 GB of GPU memory";

    fn told(reasons: &[Option<&str>]) -> Vec<Option<String>> {
        reasons.iter().map(|r| r.map(String::from)).collect()
    }

    fn warden() -> (ModelWarden, Arc<FakeOllama>, Arc<World>) {
        let (ollama, world) = (Arc::new(FakeOllama::default()), Arc::new(World::default()));
        let (heavy, busy, cold, now) = (world.clone(), world.clone(), world.clone(), world.clone());
        let warden = ModelWarden::new(
            ollama.clone(), "gemma4:e4b",
            Arc::new(move || heavy.heavy.lock().unwrap().clone()),
            move || busy.busy.load(Ordering::SeqCst),
            move |reason| cold.cold.lock().unwrap().push(reason),
            60.0,
            move || *now.now.lock().unwrap(),
        );
        (warden, ollama, world)
    }

    #[tokio::test]
    async fn keeps_the_model_loaded() {
        let (warden, ollama, world) = warden();

        warden.check().await;
        world.later(10.0);
        warden.check().await;

        assert_eq!(ollama.loaded(), ["gemma4:e4b"]);
        assert_eq!(*ollama.calls.lock().unwrap(), ["load gemma4:e4b", "load gemma4:e4b"]); // asked for each time
        assert_eq!(world.cold(), told(&[Some("Loading the model"), None]));
    }

    #[tokio::test]
    async fn loads_it_again_if_something_else_unloaded_it() {
        let (warden, ollama, world) = warden();
        warden.check().await;
        ollama.loaded.lock().unwrap().clear();

        warden.check().await;

        assert_eq!(ollama.loaded(), ["gemma4:e4b"]);
        assert_eq!(world.cold(), told(&[Some("Loading the model"), None, Some("Loading the model"), None]));
    }

    #[tokio::test]
    async fn unloads_while_something_heavy_runs_and_stays_quiet_about_it() {
        let (warden, ollama, world) = warden();
        warden.check().await;
        world.heavy(Some(GAME));

        for _ in 0..5 {
            warden.check().await;
            world.later(10.0);
        }

        assert!(ollama.loaded().is_empty());
        assert_eq!(*ollama.calls.lock().unwrap(), ["load gemma4:e4b", "unload gemma4:e4b"]);
        assert_eq!(world.cold(), told(&[Some("Loading the model"), None, Some(&*format!("Paused: {GAME}"))]));
    }

    #[tokio::test]
    async fn a_command_during_a_game_unloads_the_model_after_it_finishes() {
        let (warden, ollama, world) = warden();
        world.heavy(Some(GAME));
        warden.check().await;
        ollama.loaded.lock().unwrap().insert("gemma4:e4b".into()); // a command loaded it
        world.busy.store(true, Ordering::SeqCst);

        warden.check().await;
        assert_eq!(ollama.loaded(), ["gemma4:e4b"]); // not while the command runs

        world.busy.store(false, Ordering::SeqCst);
        warden.check().await;
        assert!(ollama.loaded().is_empty());
    }

    #[tokio::test]
    async fn reloads_once_things_have_been_calm_for_a_while() {
        let (warden, ollama, world) = warden();
        world.heavy(Some("the CPU is busy (load 20 on 24 cores)"));
        warden.check().await;
        world.heavy(None);

        world.later(50.0);
        warden.check().await;
        assert!(ollama.loaded().is_empty()); // not yet: it might come straight back

        world.heavy(Some("the CPU is busy (load 19 on 24 cores)"));
        warden.check().await;
        world.heavy(None);
        world.later(30.0);
        warden.check().await;
        assert!(ollama.loaded().is_empty()); // the spike started the calm over

        world.later(30.0);
        warden.check().await;
        assert_eq!(ollama.loaded(), ["gemma4:e4b"]);
    }

    #[tokio::test]
    async fn ollama_down_is_said_once_and_recovers() {
        let (warden, ollama, world) = warden();
        ollama.down.store(true, Ordering::SeqCst);
        warden.check().await;
        warden.check().await;
        assert_eq!(world.cold(), told(&[Some("Ollama isn't responding")]));

        ollama.down.store(false, Ordering::SeqCst);
        warden.check().await;

        assert_eq!(ollama.loaded(), ["gemma4:e4b"]);
        assert_eq!(world.cold(), told(&[Some("Ollama isn't responding"), Some("Loading the model"), None]));
    }

    #[tokio::test]
    async fn it_checks_in_the_background() {
        let (warden, ollama, _) = warden();

        let running = Arc::new(warden).start(Duration::from_millis(10));
        tokio::time::sleep(Duration::from_millis(200)).await;
        running.abort();

        assert!(ollama.calls.lock().unwrap().len() > 1);
    }
}
