//! The agent session to resume, forgotten after a while without use.

use std::path::PathBuf;
use std::time::UNIX_EPOCH;

pub struct SessionStore {
    path: PathBuf,
    ttl: f64,
    clock: Box<dyn Fn() -> f64 + Send + Sync>,
}

impl SessionStore {
    /// Forgets the session [ttl] seconds after it was last saved, by [clock] (seconds since the epoch).
    pub fn new(path: PathBuf, ttl: f64, clock: impl Fn() -> f64 + Send + Sync + 'static) -> SessionStore {
        SessionStore { path, ttl, clock: Box::new(clock) }
    }

    pub fn load(&self) -> Option<String> {
        let saved = std::fs::metadata(&self.path).ok()?.modified().ok()?;
        let saved = saved.duration_since(UNIX_EPOCH).ok()?.as_secs_f64();
        if (self.clock)() - saved > self.ttl {
            log::info!("Session expired after {}s, starting new", self.ttl);
            self.clear();
            return None;
        }
        Some(std::fs::read_to_string(&self.path).ok()?.trim().to_string()).filter(|id| !id.is_empty())
    }

    pub fn save(&self, session_id: &str) {
        let saved = self.path.parent().map_or(Ok(()), std::fs::create_dir_all).and_then(|_| std::fs::write(&self.path, session_id));
        if let Err(e) = saved {
            log::error!("Can't save the session to {}: {e}", self.path.display());
        }
    }

    pub fn clear(&self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::SystemTime;

    use super::*;

    fn now() -> u64 {
        SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs()
    }

    fn store(dir: &tempfile::TempDir, clock: Arc<AtomicU64>) -> SessionStore {
        SessionStore::new(dir.path().join("saambha/session_id"), 3600.0, move || clock.load(Ordering::SeqCst) as f64)
    }

    #[test]
    fn nothing_saved_is_no_session() {
        let dir = tempfile::tempdir().unwrap();

        assert_eq!(store(&dir, Arc::new(AtomicU64::new(now()))).load(), None);
    }

    #[test]
    fn a_saved_session_is_resumed() {
        let dir = tempfile::tempdir().unwrap();
        let sessions = store(&dir, Arc::new(AtomicU64::new(now())));

        sessions.save("sess-1");

        assert_eq!(sessions.load().as_deref(), Some("sess-1"));
    }

    #[test]
    fn an_old_session_is_forgotten() {
        let dir = tempfile::tempdir().unwrap();
        let clock = Arc::new(AtomicU64::new(now()));
        let sessions = store(&dir, clock.clone());
        sessions.save("sess-1");

        clock.store(now() + 3601, Ordering::SeqCst);

        assert_eq!(sessions.load(), None);
        assert!(!dir.path().join("saambha/session_id").exists());
    }

    #[test]
    fn a_cleared_session_is_gone() {
        let dir = tempfile::tempdir().unwrap();
        let sessions = store(&dir, Arc::new(AtomicU64::new(now())));
        sessions.save("sess-1");

        sessions.clear();

        assert_eq!(sessions.load(), None);
    }
}
