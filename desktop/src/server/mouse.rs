//! A virtual mouse made with the kernel's uinput, which the phone and watch apps move through /mouse.
//!
//! The desktop sees it as a real mouse, so it works the same on X11 and on any Wayland desktop, and the
//! desktop's own pointer acceleration applies to it.

use std::collections::VecDeque;
use std::fs::OpenOptions;
use std::io::Write;
use std::os::fd::AsRawFd;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

use serde_json::Value;

const EV_SYN: u16 = 0;
const EV_KEY: u16 = 1;
const EV_REL: u16 = 2;
const SYN_REPORT: u16 = 0;
const REL_X: u16 = 0;
const REL_Y: u16 = 1;
const REL_WHEEL: u16 = 8;
const BTN_LEFT: u16 = 0x110;
const BTN_RIGHT: u16 = 0x111;
const BUS_VIRTUAL: u16 = 0x06;

/// An ioctl request number, as the kernel's _IOC macro makes them for uinput ('U').
const fn ioc(direction: u64, number: u64, size: u64) -> u64 {
    direction << 30 | size << 16 | (b'U' as u64) << 8 | number
}

/// struct uinput_setup: an input_id (bus, vendor, product, version), the name, and ff_effects_max.
const SETUP_SIZE: usize = 4 * 2 + 80 + 4;
const UI_DEV_CREATE: u64 = ioc(0, 1, 0);
const UI_DEV_DESTROY: u64 = ioc(0, 2, 0);
const UI_DEV_SETUP: u64 = ioc(1, 3, SETUP_SIZE as u64);
const UI_SET_EVBIT: u64 = ioc(1, 100, 4);
const UI_SET_KEYBIT: u64 = ioc(1, 101, 4);
const UI_SET_RELBIT: u64 = ioc(1, 102, 4);

pub trait Mouse: Send {
    fn move_by(&mut self, dx: i32, dy: i32) -> std::io::Result<()>;
    fn click(&mut self, right: bool) -> std::io::Result<()>;
    /// Positive scrolls up, like turning a wheel away from you.
    fn scroll(&mut self, notches: i32) -> std::io::Result<()>;
}

/// Exists while it's open: two buttons and a wheel.
pub struct VirtualMouse {
    device: std::fs::File,
}

fn ioctl(device: &std::fs::File, request: u64, arg: libc::c_ulong) -> std::io::Result<()> {
    // SAFETY: uinput's requests, each with the argument it takes (an int, or a pointer to a uinput_setup).
    if unsafe { libc::ioctl(device.as_raw_fd(), request as _, arg) } < 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

impl VirtualMouse {
    pub fn open(path: &Path, name: &str) -> std::io::Result<VirtualMouse> {
        let device = OpenOptions::new().write(true).custom_flags(libc::O_NONBLOCK).open(path)?;
        ioctl(&device, UI_SET_EVBIT, EV_KEY.into())?;
        for button in [BTN_LEFT, BTN_RIGHT] {
            ioctl(&device, UI_SET_KEYBIT, button.into())?;
        }
        ioctl(&device, UI_SET_EVBIT, EV_REL.into())?;
        for axis in [REL_X, REL_Y, REL_WHEEL] {
            ioctl(&device, UI_SET_RELBIT, axis.into())?;
        }
        let mut setup = [0u8; SETUP_SIZE];
        setup[..2].copy_from_slice(&BUS_VIRTUAL.to_ne_bytes());
        let name = &name.as_bytes()[..name.len().min(79)];
        setup[8..8 + name.len()].copy_from_slice(name);
        ioctl(&device, UI_DEV_SETUP, setup.as_ptr() as libc::c_ulong)?;
        ioctl(&device, UI_DEV_CREATE, 0)?;
        Ok(VirtualMouse { device })
    }

    /// [events] (type, code, value) as one report: the desktop takes them in together.
    fn send(&mut self, events: &[(u16, u16, i32)]) -> std::io::Result<()> {
        let mut report = Vec::with_capacity((events.len() + 1) * 24);
        for &(kind, code, value) in events.iter().chain([&(EV_SYN, SYN_REPORT, 0)]) {
            report.extend_from_slice(&[0u8; 16]); // struct input_event's timeval: the kernel fills it in
            report.extend_from_slice(&kind.to_ne_bytes());
            report.extend_from_slice(&code.to_ne_bytes());
            report.extend_from_slice(&value.to_ne_bytes());
        }
        self.device.write_all(&report)
    }
}

impl Mouse for VirtualMouse {
    fn move_by(&mut self, dx: i32, dy: i32) -> std::io::Result<()> {
        self.send(&[(EV_REL, REL_X, dx), (EV_REL, REL_Y, dy)])
    }

    fn click(&mut self, right: bool) -> std::io::Result<()> {
        let button = if right { BTN_RIGHT } else { BTN_LEFT };
        self.send(&[(EV_KEY, button, 1)])?;
        self.send(&[(EV_KEY, button, 0)])
    }

    fn scroll(&mut self, notches: i32) -> std::io::Result<()> {
        self.send(&[(EV_REL, REL_WHEEL, notches)])
    }
}

impl Drop for VirtualMouse {
    fn drop(&mut self) {
        let _ = ioctl(&self.device, UI_DEV_DESTROY, 0);
    }
}

/// Does what a touchpad [message] says with [mouse].
pub fn play(mouse: &mut dyn Mouse, message: &Value) -> std::io::Result<()> {
    let number = |key: &str| message[key].as_i64().and_then(|n| i32::try_from(n).ok());
    match (message["type"].as_str(), message["button"].as_str()) {
        (Some("move"), _) if number("dx").is_some() && number("dy").is_some() => mouse.move_by(number("dx").unwrap(), number("dy").unwrap()),
        (Some("click"), Some(button @ ("left" | "right"))) => mouse.click(button == "right"),
        (Some("scroll"), _) if number("notches").is_some() => mouse.scroll(number("notches").unwrap()),
        _ => {
            log::warn!("MOUSE: ignored {message}");
            Ok(())
        }
    }
}

/// Plays a touchpad's events at the pace they happened, not in the bunches the network brings them in
/// (a watch on weak Wi-Fi delivers ~10 at a time). The desktop speeds the pointer up by how fast moves
/// come, so bunches make it lurch.
///
/// Each event carries when it happened on the device, in that device's clock. The fastest an event has
/// come recently is the network at its best; the slowest, how late the bunching makes some. Holding
/// every event back by that spread spaces them out again, so it costs as much lag as the network
/// jitters: next to nothing on a good connection, and at most [max_delay].
pub struct Pacer {
    /// Seconds of recent events it judges the network by.
    window: f64,
    max_delay: f64,
    /// (arrived, how long after it happened)
    recent: VecDeque<(f64, f64)>,
}

impl Pacer {
    pub fn new(window: f64, max_delay: f64) -> Pacer {
        Pacer { window, max_delay, recent: VecDeque::new() }
    }

    /// When to play an event that [happened] (device seconds) and [arrived] (seconds, our clock).
    pub fn play_at(&mut self, happened: f64, arrived: f64) -> f64 {
        self.recent.push_back((arrived, arrived - happened));
        while self.recent.front().is_some_and(|(at, _)| *at < arrived - self.window) {
            self.recent.pop_front();
        }
        let delays = self.recent.iter().map(|(_, delay)| *delay);
        let (fastest, slowest) = delays.fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), d| (lo.min(d), hi.max(d)));
        happened + slowest.min(fastest + self.max_delay)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn the_mouse_is_on_the_desktop_only_while_open() {
        let uinput = Path::new("/dev/uinput");
        if OpenOptions::new().write(true).open(uinput).is_err() {
            return eprintln!("skipped: needs access to /dev/uinput (scripts/install.sh)");
        }
        let devices = || std::fs::read_to_string("/proc/bus/input/devices").unwrap();
        let name = format!("Saambha test mouse {}", std::process::id());

        let mut mouse = VirtualMouse::open(uinput, &name).unwrap();
        assert!(devices().contains(&format!("N: Name=\"{name}\"")));
        mouse.move_by(0, 0).unwrap();
        mouse.scroll(0).unwrap();
        drop(mouse);

        assert!(!devices().contains(&name));
    }

    #[derive(Default)]
    struct Recorded(Vec<String>);

    impl Mouse for Recorded {
        fn move_by(&mut self, dx: i32, dy: i32) -> std::io::Result<()> {
            self.0.push(format!("move {dx} {dy}"));
            Ok(())
        }

        fn click(&mut self, right: bool) -> std::io::Result<()> {
            self.0.push(format!("click {}", if right { "right" } else { "left" }));
            Ok(())
        }

        fn scroll(&mut self, notches: i32) -> std::io::Result<()> {
            self.0.push(format!("scroll {notches}"));
            Ok(())
        }
    }

    #[test]
    fn the_apps_messages_move_click_and_scroll_and_others_are_ignored() {
        let mut mouse = Recorded::default();

        for message in [json!({"type": "move", "dx": 3, "dy": -2, "t": 5}), json!({"type": "click", "button": "right"}),
                        json!({"type": "scroll", "notches": -1}), json!({"type": "click", "button": "middle"}),
                        json!({"type": "move", "dx": "far"}), json!("nonsense")] {
            play(&mut mouse, &message).unwrap();
        }

        assert_eq!(mouse.0, ["move 3 -2", "click right", "scroll -1"]);
    }

    /// When a Pacer plays [events], each (happened on the device, arrived here).
    fn paced(events: &[(f64, f64)], max_delay: f64) -> Vec<f64> {
        let mut pacer = Pacer::new(1.0, max_delay);
        events.iter().map(|&(happened, arrived)| (pacer.play_at(happened, arrived) * 1000.0).round() / 1000.0).collect()
    }

    #[test]
    fn a_steady_stream_plays_as_it_comes() {
        // The device's clock is 50s behind ours, and the network takes 5ms.
        let events: Vec<(f64, f64)> = (0..5).map(|t| (t as f64 / 100.0, 50.0 + t as f64 / 100.0 + 0.005)).collect();

        assert_eq!(paced(&events, 0.15), events.iter().map(|(_, arrived)| (arrived * 1000.0).round() / 1000.0).collect::<Vec<_>>());
    }

    #[test]
    fn bunches_are_spaced_out_at_the_pace_they_happened() {
        // Moves every 10ms, delivered 4 at a time every 40ms.
        let events: Vec<(f64, f64)> = (0..12).map(|t| (t as f64 * 0.01, 50.0 + 0.005 + (t / 4 + 1) as f64 * 0.04)).collect();

        let played = paced(&events, 0.15);

        let gaps: Vec<f64> = played[4..].windows(2).map(|w| ((w[1] - w[0]) * 1000.0).round() / 1000.0).collect();
        assert_eq!(gaps, [0.01; 7]);
        assert!(played.iter().zip(&events).all(|(play, (_, arrived))| *play >= (arrived * 1000.0).round() / 1000.0));
    }

    #[test]
    fn it_holds_back_no_more_than_max_delay() {
        let events = [(0.0, 50.0), (0.01, 50.5)]; // the second came half a second late

        assert_eq!(paced(&events, 0.15)[1], 50.16); // it's late: played at once
    }

    #[test]
    fn it_forgets_how_the_network_was_after_the_window() {
        let events = [(0.0, 50.0), (0.01, 50.2), (5.0, 55.0), (5.01, 55.01)];

        assert_eq!(paced(&events, 0.15)[2..], [55.0, 55.01]);
    }
}
