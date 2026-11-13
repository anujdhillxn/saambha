//! What Saambha is doing, for the dot and its chat panel to show, and the question of whether to allow a
//! new device, which the desktop answers. The dot is told of each change rather than asking.

use std::sync::Mutex;
use std::time::Duration;

use tokio::sync::{oneshot, watch};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Ready,
    Cold,
    Listening,
    Working,
    Asking,
}

impl State {
    /// Its name in the stylesheet.
    pub fn name(self) -> &'static str {
        match self {
            State::Ready => "ready",
            State::Cold => "cold",
            State::Listening => "listening",
            State::Working => "working",
            State::Asking => "asking",
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Activity {
    /// Why the local model isn't loaded, e.g. "Loading the model".
    pub model_cold: Option<String>,
    pub listening: bool,
    /// What the agent is doing, e.g. "Running skill spotify".
    pub task: Option<String>,
    pub speaking: bool,
    /// The device waiting to be allowed, e.g. "Pixel Watch 4 (192.168.0.12)".
    pub pairing: Option<String>,
}

impl Activity {
    /// The state the dot shows (its colour) and the status line: the most pressing thing going on.
    pub fn look(&self) -> (State, String) {
        if let Some(device) = &self.pairing {
            (State::Asking, format!("{device} wants to connect"))
        } else if self.listening {
            (State::Listening, "Listening".into())
        } else if let Some(task) = &self.task {
            (State::Working, task.clone())
        } else if self.speaking {
            (State::Working, "Speaking".into())
        } else if let Some(reason) = &self.model_cold {
            (State::Cold, reason.clone())
        } else {
            (State::Ready, "Ready".into())
        }
    }

    /// Whether the dot shows Saambha speaking: only when it's doing nothing else.
    pub fn just_speaking(&self) -> bool {
        self.look().0 == State::Working && self.task.is_none()
    }
}

/// Set from the agent's, the narrator's and the model warden's tasks.
pub struct Status {
    activity: watch::Sender<Activity>,
    answer: Mutex<Option<oneshot::Sender<bool>>>,
    approval_timeout: Duration,
}

impl Status {
    pub fn new(approval_timeout: Duration) -> Status {
        Status { activity: watch::Sender::new(Activity::default()), answer: Mutex::new(None), approval_timeout }
    }

    pub fn activity(&self) -> Activity {
        self.activity.borrow().clone()
    }

    /// Told of every change from now on.
    pub fn subscribe(&self) -> watch::Receiver<Activity> {
        self.activity.subscribe()
    }

    pub fn set_model_cold(&self, reason: Option<String>) {
        self.activity.send_modify(|a| a.model_cold = reason);
    }

    pub fn set_listening(&self, listening: bool) {
        self.activity.send_modify(|a| a.listening = listening);
    }

    pub fn set_task(&self, task: Option<String>) {
        self.activity.send_modify(|a| a.task = task);
    }

    pub fn set_speaking(&self, speaking: bool) {
        self.activity.send_modify(|a| a.speaking = speaking);
    }

    /// Asks the desktop whether to allow a new device: whether it was allowed in time.
    pub async fn ask(&self, ip: &str, device_name: &str) -> bool {
        let (answer, answered) = oneshot::channel();
        *self.answer.lock().unwrap() = Some(answer);
        self.activity.send_modify(|a| a.pairing = Some(format!("{device_name} ({ip})")));
        let allowed = matches!(tokio::time::timeout(self.approval_timeout, answered).await, Ok(Ok(true)));
        self.answer.lock().unwrap().take();
        self.activity.send_modify(|a| a.pairing = None);
        allowed
    }

    /// Answers the device waiting to be allowed; false if none is.
    pub fn answer(&self, allowed: bool) -> bool {
        match self.answer.lock().unwrap().take() {
            Some(answer) => {
                let _ = answer.send(allowed); // it may have timed out just now
                true
            }
            None => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;

    #[test]
    fn idle_its_ready() {
        assert_eq!(Activity::default().look(), (State::Ready, "Ready".into()));
    }

    #[test]
    fn the_most_pressing_thing_shows() {
        let busy = Activity {
            model_cold: Some("Loading the model".into()),
            listening: false,
            task: Some("Running skill spotify".into()),
            speaking: true,
            pairing: None,
        };

        assert_eq!(busy.look(), (State::Working, "Running skill spotify".into()));
        assert_eq!(Activity { listening: true, ..busy.clone() }.look(), (State::Listening, "Listening".into()));
        assert_eq!(Activity { pairing: Some("Pixel (1.2.3.4)".into()), ..busy.clone() }.look(),
                   (State::Asking, "Pixel (1.2.3.4) wants to connect".into()));
        assert_eq!(Activity { task: None, speaking: false, ..busy }.look(), (State::Cold, "Loading the model".into()));
    }

    #[test]
    fn the_dot_shows_speaking_only_when_nothing_else_is_going_on() {
        let speaking = Activity { speaking: true, ..Activity::default() };

        assert!(speaking.just_speaking());
        assert!(!Activity { task: Some("Running skill spotify".into()), ..speaking.clone() }.just_speaking());
        assert!(!Activity { listening: true, ..speaking }.just_speaking());
    }

    #[tokio::test]
    async fn the_dot_is_told_of_each_change() {
        let status = Status::new(Duration::from_secs(60));
        let mut changes = status.subscribe();

        status.set_listening(true);

        changes.changed().await.unwrap();
        assert!(changes.borrow_and_update().listening);
    }

    #[tokio::test]
    async fn a_device_waits_for_the_answer_on_the_desktop() {
        let status = Arc::new(Status::new(Duration::from_secs(60)));
        let mut changes = status.subscribe();
        let asking = tokio::spawn({
            let status = status.clone();
            async move { status.ask("192.168.0.12", "Pixel Watch").await }
        });
        changes.wait_for(|a| a.pairing.is_some()).await.unwrap();

        assert_eq!(status.activity().look().1, "Pixel Watch (192.168.0.12) wants to connect");
        assert!(status.answer(true));
        assert!(asking.await.unwrap());
        assert_eq!(status.activity().pairing, None);
    }

    #[tokio::test(start_paused = true)]
    async fn unanswered_its_denied() {
        let status = Status::new(Duration::from_secs(60));

        assert!(!status.ask("192.168.0.12", "Pixel Watch").await);
        assert!(!status.answer(true)); // too late: nobody is waiting
    }
}
