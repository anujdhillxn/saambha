//! Logging for the whole app: one format, to stderr (the journal, under systemd) and a rotating log
//! file. Modules log with the `log` macros; only the composition root calls [configure].

use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use gtk::glib;
use log::{LevelFilter, Log, Metadata, Record};

/// A log file that's rotated before it grows past [max_bytes]: to file.1, file.1 to file.2, and so on,
/// keeping [backups] of them.
struct RotatingFile {
    path: PathBuf,
    max_bytes: u64,
    backups: u64,
    file: Option<File>,
}

impl RotatingFile {
    fn write(&mut self, line: &str) -> std::io::Result<()> {
        let size = std::fs::metadata(&self.path).map_or(0, |m| m.len());
        if size > 0 && size + line.len() as u64 > self.max_bytes {
            self.file = None;
            for n in (1..self.backups).rev() {
                let _ = std::fs::rename(backup(&self.path, n), backup(&self.path, n + 1));
            }
            if self.backups > 0 {
                std::fs::rename(&self.path, backup(&self.path, 1))?;
            } else {
                std::fs::remove_file(&self.path)?;
            }
        }
        if self.file.is_none() {
            self.file = Some(OpenOptions::new().create(true).append(true).open(&self.path)?);
        }
        self.file.as_mut().expect("the open log").write_all(line.as_bytes())
    }
}

fn backup(path: &Path, n: u64) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(format!(".{n}"));
    PathBuf::from(name)
}

pub struct Logger {
    level: LevelFilter,
    file: Option<Mutex<RotatingFile>>,
    to_stderr: bool,
}

impl Logger {
    /// Logs [level] and up, to [file] (rotated at [max_bytes], with [backups]) if given.
    pub fn new(level: LevelFilter, file: Option<(PathBuf, u64, u64)>, to_stderr: bool) -> Logger {
        let file = file.map(|(path, max_bytes, backups)| Mutex::new(RotatingFile { path, max_bytes, backups, file: None }));
        Logger { level, file, to_stderr }
    }
}

/// e.g. "2026-11-13 16:23:17,581"
fn now() -> String {
    glib::DateTime::now_local()
        .map(|t| format!("{},{:03}", t.format("%Y-%m-%d %H:%M:%S").map(String::from).unwrap_or_default(), t.microsecond() / 1000))
        .unwrap_or_default()
}

impl Log for Logger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= self.level
    }

    fn log(&self, record: &Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let line = format!("{} {} {}: {}\n", now(), record.level(), record.target(), record.args());
        if self.to_stderr {
            let _ = std::io::stderr().write_all(line.as_bytes());
        }
        if let Some(file) = &self.file {
            let _ = file.lock().unwrap().write(&line);
        }
    }

    fn flush(&self) {}
}

/// Logs through [logger] from now on.
pub fn configure(logger: Logger) {
    log::set_max_level(logger.level);
    log::set_boxed_logger(Box::new(logger)).expect("logging set up once");
}

/// The level named [name] ("error", "warn", "info", "debug"), else info.
pub fn level(name: &str) -> LevelFilter {
    name.parse().unwrap_or(LevelFilter::Info)
}

/// The log file, read a piece at a time by the log window as it grows.
pub struct LogFile {
    path: PathBuf,
    backlog: u64,
}

impl LogFile {
    pub fn new(path: PathBuf, backlog: u64) -> LogFile {
        LogFile { path, backlog }
    }

    /// What's been written from [offset] on, and the offset to carry on from. With no offset: the last
    /// [backlog] bytes, from a line start. An offset past the end means the log was rotated since, so it
    /// starts over from the new one's start.
    pub fn read(&self, offset: Option<u64>) -> (String, u64) {
        let Ok(mut file) = File::open(&self.path) else { return (String::new(), 0) };
        let size = file.metadata().map_or(0, |m| m.len());
        let start = match offset {
            None => size.saturating_sub(self.backlog),
            Some(offset) if offset > size => 0,
            Some(offset) => offset,
        };
        let mut data = Vec::new();
        if file.seek(SeekFrom::Start(start)).and_then(|_| file.take(size - start).read_to_end(&mut data)).is_err() {
            return (String::new(), size);
        }
        if offset.is_none() && start > 0 {
            // the first line is cut off
            data = data.iter().position(|b| *b == b'\n').map_or(Vec::new(), |at| data[at + 1..].to_vec());
        }
        (String::from_utf8_lossy(&data).into_owned(), size)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(logger: &Logger, level: log::Level, target: &str, message: &str) {
        logger.log(&Record::builder().level(level).target(target).args(format_args!("{message}")).build());
    }

    fn lines(path: &Path) -> Vec<String> {
        std::fs::read_to_string(path).unwrap_or_default().lines().map(String::from).collect()
    }

    #[test]
    fn a_line_has_the_time_level_and_module() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("saambha.log");
        let logger = Logger::new(LevelFilter::Info, Some((path.clone(), 2000, 2)), false);

        record(&logger, log::Level::Info, "saambha::server::app", "INPUT: hello");

        let [line] = lines(&path).try_into().unwrap();
        assert!(line.ends_with(" INFO saambha::server::app: INPUT: hello"), "{line}");
        assert_eq!(line.len() - " INFO saambha::server::app: INPUT: hello".len(), "2026-11-13 16:23:17,581".len());
    }

    #[test]
    fn quieter_lines_than_the_level_are_left_out() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("saambha.log");
        let logger = Logger::new(level("warn"), Some((path.clone(), 2000, 2)), false);

        record(&logger, log::Level::Info, "saambha", "quiet");
        record(&logger, log::Level::Warn, "saambha", "loud");

        assert_eq!(lines(&path).iter().map(|l| l.split(": ").nth(1).unwrap()).collect::<Vec<_>>(), ["loud"]);
    }

    #[test]
    fn the_log_file_is_rotated() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("saambha.log");
        let logger = Logger::new(LevelFilter::Info, Some((path.clone(), 2000, 2)), false);

        for i in 0..200 {
            record(&logger, log::Level::Info, "saambha", &format!("line {i} {}", "x".repeat(40)));
        }

        let mut files: Vec<String> = std::fs::read_dir(dir.path()).unwrap().map(|e| e.unwrap().file_name().into_string().unwrap()).collect();
        files.sort();
        assert_eq!(files, ["saambha.log", "saambha.log.1", "saambha.log.2"]);
        assert!(std::fs::metadata(&path).unwrap().len() <= 2000);
        assert!(lines(&path).last().unwrap().contains("line 199 "));
    }

    #[test]
    fn an_unknown_level_is_info() {
        assert_eq!((level("debug"), level("WARN"), level("loud")), (LevelFilter::Debug, LevelFilter::Warn, LevelFilter::Info));
    }

    fn write(path: &Path, text: &str) {
        OpenOptions::new().create(true).append(true).open(path).unwrap().write_all(text.as_bytes()).unwrap();
    }

    #[test]
    fn the_log_window_starts_with_the_end_of_the_log_from_a_line_start() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("saambha.log");
        write(&path, &(0..100).map(|n| format!("line {n}\n")).collect::<String>());

        assert_eq!(LogFile::new(path.clone(), 20).read(None), ("line 98\nline 99\n".into(), std::fs::metadata(&path).unwrap().len()));
    }

    #[test]
    fn a_short_log_is_read_whole() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("saambha.log");
        write(&path, "line 1\nline 2\n");

        assert_eq!(LogFile::new(path, 1000).read(None), ("line 1\nline 2\n".into(), 14));
    }

    #[test]
    fn it_carries_on_from_the_offset() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("saambha.log");
        write(&path, "line 1\n");
        let logs = LogFile::new(path.clone(), 1000);
        let (_, offset) = logs.read(None);

        assert_eq!(logs.read(Some(offset)), (String::new(), offset));
        write(&path, "line 2\nhalf a ");
        let (text, offset) = logs.read(Some(offset));
        assert_eq!(text, "line 2\nhalf a ");
        write(&path, "line\n");
        assert_eq!(logs.read(Some(offset)).0, "line\n");
    }

    #[test]
    fn it_starts_over_after_rotation() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("saambha.log");
        write(&path, "old 1\nold 2\n");
        let logs = LogFile::new(path.clone(), 1000);
        let (_, offset) = logs.read(None);

        std::fs::rename(&path, dir.path().join("saambha.log.1")).unwrap();
        write(&path, "new\n");

        assert_eq!(logs.read(Some(offset)), ("new\n".into(), 4));
    }

    #[test]
    fn no_log_yet() {
        let dir = tempfile::tempdir().unwrap();

        assert_eq!(LogFile::new(dir.path().join("saambha.log"), 1000).read(None), (String::new(), 0));
    }
}
