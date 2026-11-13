//! Is the user running something heavy (a game, a render, a training job...)? Each signal returns a
//! short reason when it sees one; HeavyTaskDetector asks them all.

use std::collections::HashSet;
use std::process::Command;
use std::sync::Mutex;

/// Why the machine is busy, if it is; an error if it can't tell (e.g. no nvidia-smi).
pub type Signal = Box<dyn Fn() -> Result<Option<String>, String> + Send + Sync>;

pub struct HeavyTaskDetector {
    signals: Vec<Signal>,
    broken: Mutex<HashSet<usize>>,
}

impl HeavyTaskDetector {
    pub fn new(signals: Vec<Signal>) -> HeavyTaskDetector {
        HeavyTaskDetector { signals, broken: Mutex::default() }
    }

    /// Why the machine is busy (e.g. "the CPU is busy (load 20 on 24 cores)"), or None. A signal that
    /// fails counts as "not busy", logged once.
    pub fn heavy_task(&self) -> Option<String> {
        for (i, signal) in self.signals.iter().enumerate() {
            match signal() {
                Err(why) => {
                    if self.broken.lock().unwrap().insert(i) {
                        log::warn!("A heavy task check failed: {why}");
                    }
                }
                Ok(reason) => {
                    self.broken.lock().unwrap().remove(&i);
                    if reason.is_some() {
                        return reason;
                    }
                }
            }
        }
        None
    }
}

/// (process name, MB of GPU memory) for every process on the NVIDIA GPU, graphics or compute.
pub fn nvidia_gpu_memory() -> Result<Vec<(String, u64)>, String> {
    let output = Command::new("nvidia-smi").args(["pmon", "-c", "1", "-s", "m"]).output().map_err(|e| format!("nvidia-smi: {e}"))?;
    if !output.status.success() {
        return Err(format!("nvidia-smi: {}", String::from_utf8_lossy(&output.stderr).trim()));
    }
    Ok(String::from_utf8_lossy(&output.stdout).lines()
        .filter(|line| !line.starts_with('#'))
        // gpu pid type fb ccpm command...
        .filter_map(|line| {
            let fields: Vec<&str> = line.split_whitespace().collect();
            let mb = fields.get(3)?.parse().ok()?;
            Some((fields.get(5..)?.join(" "), mb))
        })
        .collect())
}

/// The model server, and the display servers: on X11, Xorg holds every window's textures, so it grows
/// with ordinary desktop use (games render directly and show up under their own name).
pub const NOT_HEAVY: [&str; 7] = ["ollama", "llama-server", "Xorg", "Xwayland", "gnome-shell", "cosmic-comp", "kwin"];

/// A process (other than those NOT_HEAVY) using at least [min_mb] of GPU memory. Desktop apps use a few
/// hundred MB at most; games, renders and ML jobs use gigabytes.
pub fn gpu_memory_hog(min_mb: u64, read_usage: impl Fn() -> Result<Vec<(String, u64)>, String> + Send + Sync + 'static) -> Signal {
    Box::new(move || {
        Ok(read_usage()?.into_iter()
            .find(|(name, mb)| *mb >= min_mb && !NOT_HEAVY.iter().any(|ignored| name.starts_with(ignored)))
            .map(|(name, mb)| format!("{name} is using {:.1} GB of GPU memory", mb as f64 / 1024.0)))
    })
}

pub fn available_memory_mb() -> Result<u64, String> {
    let meminfo = std::fs::read_to_string("/proc/meminfo").map_err(|e| e.to_string())?;
    meminfo.lines()
        .find_map(|line| line.strip_prefix("MemAvailable:"))
        .and_then(|rest| rest.split_whitespace().next()?.parse::<u64>().ok())
        .map(|kb| kb / 1024)
        .ok_or_else(|| "no MemAvailable in /proc/meminfo".into())
}

pub fn low_memory(min_available_mb: u64, read_available: impl Fn() -> Result<u64, String> + Send + Sync + 'static) -> Signal {
    Box::new(move || {
        let available = read_available()?;
        Ok((available < min_available_mb).then(|| format!("only {:.1} GB of RAM is free", available as f64 / 1024.0)))
    })
}

/// The 1-minute load average.
pub fn load_average() -> Result<f64, String> {
    let loadavg = std::fs::read_to_string("/proc/loadavg").map_err(|e| e.to_string())?;
    loadavg.split_whitespace().next().and_then(|load| load.parse().ok()).ok_or_else(|| "no load in /proc/loadavg".into())
}

/// The 1-minute load average above [max_per_core] times the number of [cores].
pub fn high_cpu_load(max_per_core: f64, cores: usize, read_load: impl Fn() -> Result<f64, String> + Send + Sync + 'static) -> Signal {
    Box::new(move || {
        let load = read_load()?;
        Ok((load > max_per_core * cores as f64).then(|| format!("the CPU is busy (load {load:.0} on {cores} cores)")))
    })
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    fn desktop() -> Vec<(String, u64)> {
        [("Xorg", 225), ("gnome-shell", 42), ("chrome --type=g", 21), ("llama-server", 4856)].map(|(n, mb)| (n.to_string(), mb)).into()
    }

    #[test]
    fn desktop_apps_and_the_model_server_are_not_heavy() {
        assert_eq!(gpu_memory_hog(1500, || Ok(desktop()))(), Ok(None));
        assert_eq!(gpu_memory_hog(1500, || Ok(vec![("ollama".into(), 5000)]))(), Ok(None));
        assert_eq!(gpu_memory_hog(1500, || Ok(vec![("Xorg".into(), 1993), ("gnome-shell".into(), 1600)]))(), Ok(None));
    }

    #[test]
    fn a_process_using_lots_of_gpu_memory_is() {
        let hog = gpu_memory_hog(1500, || Ok([desktop(), vec![("Cyberpunk2077.ex".into(), 6144)]].concat()));

        assert_eq!(hog(), Ok(Some("Cyberpunk2077.ex is using 6.0 GB of GPU memory".into())));
    }

    #[test]
    fn low_memory_and_high_cpu_load() {
        assert_eq!(low_memory(1500, || Ok(1024))(), Ok(Some("only 1.0 GB of RAM is free".into())));
        assert_eq!(low_memory(1500, || Ok(6000))(), Ok(None));
        assert_eq!(high_cpu_load(0.75, 24, || Ok(20.0))(), Ok(Some("the CPU is busy (load 20 on 24 cores)".into())));
        assert_eq!(high_cpu_load(0.75, 24, || Ok(17.0))(), Ok(None));
    }

    #[test]
    fn reads_the_real_free_memory_and_load() {
        assert!(available_memory_mb().unwrap() > 0);
        assert!(load_average().unwrap() >= 0.0);
    }

    #[test]
    fn the_first_busy_signal_wins() {
        let detector = HeavyTaskDetector::new(vec![Box::new(|| Ok(None)), Box::new(|| Ok(Some("a game is running".into()))),
                                                   Box::new(|| Ok(Some("the CPU is busy".into())))]);

        assert_eq!(detector.heavy_task().as_deref(), Some("a game is running"));
        assert_eq!(HeavyTaskDetector::new(vec![Box::new(|| Ok(None))]).heavy_task(), None);
    }

    #[test]
    fn a_broken_signal_doesnt_hide_the_others() {
        let failed = Arc::new(AtomicUsize::new(0));
        let tries = failed.clone();
        let detector = HeavyTaskDetector::new(vec![
            Box::new(move || {
                tries.fetch_add(1, Ordering::SeqCst);
                Err("nvidia-smi: No such file".into())
            }),
            Box::new(|| Ok(Some("a game is running".into()))),
        ]);

        assert_eq!(detector.heavy_task().as_deref(), Some("a game is running"));
        assert_eq!(detector.heavy_task().as_deref(), Some("a game is running"));
        assert_eq!(failed.load(Ordering::SeqCst), 2);
        assert_eq!(*detector.broken.lock().unwrap(), HashSet::from([0])); // so it's logged once
    }
}
