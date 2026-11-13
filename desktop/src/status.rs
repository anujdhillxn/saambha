//! What Saambha is doing, from the server, and following it as it changes.

use std::time::Duration;

use serde::Deserialize;

const RETRY: Duration = Duration::from_secs(2);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
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

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct Status {
    pub state: State,
    pub line: String,
    #[serde(default)]
    pub task: Option<String>,
    #[serde(default)]
    pub speaking: bool,
    /// The device waiting to be allowed to connect.
    #[serde(default)]
    pub pairing: Option<String>,
    #[serde(default)]
    pub version: u64,
}

impl Status {
    pub fn offline() -> Self {
        Status {
            state: State::Cold,
            line: "Can't reach the Saambha server".into(),
            task: None,
            speaking: false,
            pairing: None,
            version: 0,
        }
    }

    /// Whether the dot shows Saambha speaking: only when it's doing nothing else.
    pub fn just_speaking(&self) -> bool {
        self.speaking && self.state == State::Working && self.task.is_none()
    }
}

/// Follows the server's status: each request names the version last shown, and the server answers
/// it once that's changed; while the server can't be reached, it's offline.
#[derive(Default)]
pub struct Watch {
    version: Option<u64>,
}

impl Watch {
    /// The version to ask for a change from: none, to be told the status now.
    pub fn after(&self) -> Option<u64> {
        self.version
    }

    /// What to show for the server's [reply], and how long to wait before asking again.
    pub fn reply(&mut self, reply: Result<Status, String>) -> (Status, Duration) {
        match reply {
            Ok(status) => {
                self.version = Some(status.version);
                (status, Duration::ZERO)
            }
            Err(_) => {
                self.version = None;
                (Status::offline(), RETRY)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status(state: State, version: u64) -> Status {
        Status { state, version, ..Status::offline() }
    }

    #[test]
    fn each_request_waits_for_a_change_from_the_version_last_shown() {
        let mut watch = Watch::default();
        let mut asked = vec![watch.after()];

        let ready = watch.reply(Ok(status(State::Ready, 4)));
        asked.push(watch.after());
        let listening = watch.reply(Ok(status(State::Listening, 5)));
        asked.push(watch.after());

        assert_eq!(asked, [None, Some(4), Some(5)]);
        assert_eq!(ready, (status(State::Ready, 4), Duration::ZERO));
        assert_eq!(listening, (status(State::Listening, 5), Duration::ZERO));
    }

    #[test]
    fn while_the_server_cant_be_reached_its_offline() {
        let mut watch = Watch::default();
        watch.reply(Ok(status(State::Ready, 4)));

        let shown = watch.reply(Err("refused".into()));

        assert_eq!(shown, (Status::offline(), RETRY));
    }

    #[test]
    fn once_the_servers_back_it_starts_over_from_its_status_now() {
        let mut watch = Watch::default();
        watch.reply(Ok(status(State::Ready, 4)));
        watch.reply(Err("refused".into()));

        assert_eq!(watch.after(), None);
    }

    #[test]
    fn the_dot_shows_speaking_only_when_nothing_else_is_going_on() {
        let speaking = Status { speaking: true, ..status(State::Working, 1) };

        assert!(speaking.just_speaking());
        assert!(!Status { task: Some("Running skill spotify".into()), ..speaking.clone() }.just_speaking());
        assert!(!Status { state: State::Listening, ..speaking }.just_speaking());
    }
}
