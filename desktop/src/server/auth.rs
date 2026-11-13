//! Device pairing: an app calls POST /pair, which asks on the desktop whether to allow it. Approved
//! devices get a long-lived token they send as `Authorization: Bearer <token>`. Requests from this
//! machine are always allowed.
//!
//! Approved devices live in devices.json; delete an entry to revoke it.

use std::collections::{BTreeMap, HashMap};
use std::io::Read;
use std::net::IpAddr;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::Mutex;

use gtk::glib;
use serde::{Deserialize, Serialize};

use crate::server::BoxFuture;

/// This machine: 127.0.0.1 or ::1 (the rest of 127.0.0.0/8 stands in for other devices in the tests).
pub fn is_local(ip: IpAddr) -> bool {
    ip.to_canonical() == IpAddr::from([127, 0, 0, 1]) || ip.is_loopback() && ip.is_ipv6()
}

/// The token in an `Authorization: Bearer <token>` header.
pub fn bearer_token(authorization: Option<&str>) -> Option<&str> {
    let auth = authorization?;
    auth.get(..7).filter(|scheme| scheme.eq_ignore_ascii_case("bearer ")).map(|_| auth[7..].trim())
}

fn hash(token: &str) -> String {
    glib::compute_checksum_for_string(glib::ChecksumType::Sha256, token).expect("a SHA-256").into()
}

/// A random token, URL-safe: 32 bytes in unpadded base64url.
fn new_token() -> String {
    let mut bytes = [0u8; 32];
    std::fs::File::open("/dev/urandom").and_then(|mut f| f.read_exact(&mut bytes)).expect("random bytes");
    glib::base64_encode(&bytes).replace('+', "-").replace('/', "_").trim_end_matches('=').to_string()
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Device {
    pub name: String,
    pub ip: String,
    pub approved_at: String,
}

/// Approved devices, keyed by a hash of their token (the token itself isn't stored).
pub struct DeviceRegistry {
    path: PathBuf,
}

impl DeviceRegistry {
    pub fn new(path: PathBuf) -> DeviceRegistry {
        DeviceRegistry { path }
    }

    /// Approves a device: its new token.
    pub fn add(&self, name: &str, ip: &str) -> std::io::Result<String> {
        let token = new_token();
        let mut devices = self.load();
        let approved_at = glib::DateTime::now_local().and_then(|t| t.format("%Y-%m-%d %H:%M:%S")).map(String::from).unwrap_or_default();
        devices.insert(hash(&token), Device { name: name.into(), ip: ip.into(), approved_at });
        std::fs::write(&self.path, serde_json::to_string_pretty(&devices).expect("devices as JSON"))?;
        std::fs::set_permissions(&self.path, std::fs::Permissions::from_mode(0o600))?;
        Ok(token)
    }

    pub fn is_valid(&self, token: Option<&str>) -> bool {
        token.is_some_and(|token| !token.is_empty() && self.load().contains_key(&hash(token)))
    }

    fn load(&self) -> BTreeMap<String, Device> {
        std::fs::read_to_string(&self.path).ok().and_then(|text| serde_json::from_str(&text).ok()).unwrap_or_default()
    }
}

#[derive(Debug, PartialEq)]
pub enum Refused {
    /// Another device is being asked about.
    Busy,
    Denied,
}

/// Puts the question to the user: whether the device at that IP, by that name, was allowed.
pub type Ask = Box<dyn Fn(String, String) -> BoxFuture<'static, bool> + Send + Sync>;

/// Asks on the desktop (one device at a time) and registers the devices that are allowed. A denied IP
/// isn't asked again for [deny_cooldown] seconds, so a device can't keep asking.
pub struct Pairing {
    registry: std::sync::Arc<DeviceRegistry>,
    ask: Ask,
    deny_cooldown: f64,
    clock: Box<dyn Fn() -> f64 + Send + Sync>,
    asking: Mutex<bool>,
    denied_until: Mutex<HashMap<String, f64>>,
}

impl Pairing {
    pub fn new(registry: std::sync::Arc<DeviceRegistry>, ask: Ask, deny_cooldown: f64,
               clock: impl Fn() -> f64 + Send + Sync + 'static) -> Pairing {
        Pairing { registry, ask, deny_cooldown, clock: Box::new(clock), asking: Mutex::new(false), denied_until: Mutex::default() }
    }

    /// A new token for the device, once it's allowed.
    pub async fn request(&self, ip: &str, device_name: &str) -> Result<String, Refused> {
        let name: String = device_name.chars().take(100).collect();
        let name = if name.is_empty() { "unknown".to_string() } else { name };
        if self.denied_until.lock().unwrap().get(ip).is_some_and(|until| *until > (self.clock)()) {
            return Err(Refused::Denied);
        }
        {
            let mut asking = self.asking.lock().unwrap();
            if *asking {
                return Err(Refused::Busy);
            }
            *asking = true;
        }
        let allowed = (self.ask)(ip.to_string(), name.clone()).await;
        *self.asking.lock().unwrap() = false;

        log::info!("AUTH: {} {ip} ({})", if allowed { "approved" } else { "denied" }, name.chars().take(60).collect::<String>());
        if !allowed {
            self.denied_until.lock().unwrap().insert(ip.to_string(), (self.clock)() + self.deny_cooldown);
            return Err(Refused::Denied);
        }
        self.registry.add(&name, ip).map_err(|e| {
            log::error!("Can't save the approved device: {e}");
            Refused::Denied
        })
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;

    fn registry(dir: &tempfile::TempDir) -> Arc<DeviceRegistry> {
        Arc::new(DeviceRegistry::new(dir.path().join("devices.json")))
    }

    /// The devices the desktop was asked about: (IP, name).
    type Asked = Arc<Mutex<Vec<(String, String)>>>;

    /// Pairing that answers every question with [allow], and the questions it was asked.
    fn pairing(registry: Arc<DeviceRegistry>, allow: bool, now: Arc<AtomicU64>) -> (Pairing, Asked) {
        let asked = Arc::new(Mutex::new(Vec::new()));
        let questions = asked.clone();
        let ask: Ask = Box::new(move |ip, name| {
            questions.lock().unwrap().push((ip, name));
            Box::pin(async move { allow })
        });
        (Pairing::new(registry, ask, 300.0, move || now.load(Ordering::SeqCst) as f64), asked)
    }

    #[test]
    fn this_machine_is_local() {
        for ip in ["127.0.0.1", "::1", "::ffff:127.0.0.1"] {
            assert!(is_local(ip.parse().unwrap()), "{ip}");
        }
        assert!(!is_local("192.168.0.12".parse().unwrap()) && !is_local("127.0.0.2".parse().unwrap()));
    }

    #[test]
    fn the_bearer_token_is_read_from_the_header() {
        assert_eq!(bearer_token(Some("Bearer abc ")), Some("abc"));
        assert_eq!(bearer_token(Some("bearer abc")), Some("abc"));
        assert_eq!(bearer_token(Some("Basic abc")), None);
        assert_eq!(bearer_token(None), None);
    }

    #[test]
    fn an_approved_device_is_known_by_its_token_which_isnt_stored() {
        let dir = tempfile::tempdir().unwrap();
        let registry = registry(&dir);

        let token = registry.add("Phone", "192.168.0.12").unwrap();

        assert!(registry.is_valid(Some(&token)));
        assert!(!registry.is_valid(Some("guessed")) && !registry.is_valid(None) && !registry.is_valid(Some("")));
        let saved = std::fs::read_to_string(dir.path().join("devices.json")).unwrap();
        assert!(!saved.contains(&token) && saved.contains("Phone"));
        let mode = std::fs::metadata(dir.path().join("devices.json")).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    #[test]
    fn devices_the_python_server_approved_still_work() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("devices.json"),
                       format!(r#"{{"{}": {{"name": "Watch", "ip": "192.168.0.11", "approved_at": "2026-10-10 10:00:00"}}}}"#, hash("watch-token"))).unwrap();

        assert!(registry(&dir).is_valid(Some("watch-token")));
    }

    #[test]
    fn tokens_are_url_safe_and_different_each_time() {
        let (a, b) = (new_token(), new_token());

        assert_ne!(a, b);
        assert_eq!(a.len(), 43);
        assert!(a.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'), "{a}");
    }

    #[tokio::test]
    async fn an_allowed_device_gets_a_token() {
        let dir = tempfile::tempdir().unwrap();
        let (pairing, asked) = pairing(registry(&dir), true, Arc::new(AtomicU64::new(0)));

        let token = pairing.request("192.168.0.12", "Pixel Watch").await.unwrap();

        assert!(registry(&dir).is_valid(Some(&token)));
        assert_eq!(*asked.lock().unwrap(), [("192.168.0.12".to_string(), "Pixel Watch".to_string())]);
    }

    #[tokio::test]
    async fn a_denied_device_isnt_asked_about_again_for_a_while() {
        let dir = tempfile::tempdir().unwrap();
        let now = Arc::new(AtomicU64::new(1000));
        let (pairing, asked) = pairing(registry(&dir), false, now.clone());

        assert_eq!(pairing.request("192.168.0.12", "Phone").await, Err(Refused::Denied));
        assert_eq!(pairing.request("192.168.0.12", "Phone").await, Err(Refused::Denied));
        now.store(1301, Ordering::SeqCst);
        assert_eq!(pairing.request("192.168.0.12", "Phone").await, Err(Refused::Denied));

        assert_eq!(asked.lock().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn one_device_is_asked_about_at_a_time() {
        let dir = tempfile::tempdir().unwrap();
        let (answer, answered) = tokio::sync::oneshot::channel::<bool>();
        let answered = Arc::new(Mutex::new(Some(answered)));
        let ask: Ask = Box::new(move |_, _| {
            let answered = answered.lock().unwrap().take().unwrap();
            Box::pin(async move { answered.await.unwrap() })
        });
        let pairing = Arc::new(Pairing::new(registry(&dir), ask, 300.0, || 0.0));
        let first = tokio::spawn({
            let pairing = pairing.clone();
            async move { pairing.request("192.168.0.12", "Phone").await }
        });
        tokio::task::yield_now().await;

        assert_eq!(pairing.request("192.168.0.11", "Watch").await, Err(Refused::Busy));
        answer.send(true).unwrap();
        assert!(first.await.unwrap().is_ok());
    }

    #[tokio::test]
    async fn a_long_name_is_cut_short_and_no_name_is_unknown() {
        let dir = tempfile::tempdir().unwrap();
        let (pairing, asked) = pairing(registry(&dir), true, Arc::new(AtomicU64::new(0)));

        pairing.request("1.2.3.4", &"x".repeat(150)).await.unwrap();
        pairing.request("1.2.3.5", "").await.unwrap();

        let names: Vec<usize> = asked.lock().unwrap().iter().map(|(_, n)| n.len()).collect();
        assert_eq!(names, [100, 7]);
    }
}
