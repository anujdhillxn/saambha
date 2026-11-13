//! How well each agent model does Saambha's job: the same commands through every model, several times
//! each (a model that gets it right once in three isn't reliable), each reply checked against the
//! machine's own answer, timed, and compared in a table at the end.
//!
//! The commands only look things up, and whatever an agent saves in Saambha's data folder is put back.
//! Besides single questions, they cover what goes wrong in use: a misheard request, a request after a
//! long conversation, using a skill twice, saying which speaker sound is on (which small models wrongly
//! claim they can't), and replies that run long.
//!
//! It runs the real models, so it costs usage and takes a while, and only when asked:
//!     cargo test --test models -- --run
//! compares the configured claude_model and pi_model at the configured effort (SAAMBHA_EFFORT=low to
//! change it); `--models claude:claude-haiku-5-5,pi:ollama/saambha-gemma4:e4b` picks others (an Ollama
//! model has to be pulled already), `--runs 1` for a quick look, `--only <task>` for one task.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::{Duration, Instant};

use gtk::glib;
use saambha::server::agent::Assistant;
use saambha::server::agent::pi::with_ollama_model;
use saambha::server::compose;
use saambha::server::reply::parse;
use saambha::server::settings::{self, Settings};
use serde_json::{Value, json};

/// Words: about 15 seconds spoken.
const LONG_REPLY: usize = 40;

fn app() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

struct Reply {
    said: String,
    /// The summary: what's spoken.
    text: String,
    details: String,
    seconds: f64,
}

/// A harness that remembers each reply it's asked for and how long it took.
struct Agent {
    harness: Arc<dyn Assistant>,
    replies: Vec<Reply>,
}

impl Agent {
    /// The reply's summary: what's spoken.
    async fn ask(&mut self, text: &str) -> String {
        let start = Instant::now();
        let reply = parse(&self.harness.ask(text, &|_| {}).await);
        self.replies.push(Reply { said: text.into(), text: reply.summary.clone(), details: reply.details, seconds: start.elapsed().as_secs_f64() });
        reply.summary
    }

    /// Says [texts] first, in the same conversation, without counting the replies.
    async fn chat(&self, texts: &[&str]) {
        for text in texts {
            self.harness.ask(text, &|_| {}).await;
        }
    }
}

/// Whether the task went right, and if not, what the reply was.
type Outcome = Result<(), String>;

fn check(ok: bool, reply: &str) -> Outcome {
    if ok { Ok(()) } else { Err(reply.to_string()) }
}

/// Every number in [text], commas and all.
fn numbers(text: &str) -> Vec<f64> {
    let mut found = Vec::new();
    let mut current = String::new();
    for c in text.chars().chain([' ']) {
        if c.is_ascii_digit() || (!current.is_empty() && (c == ',' || c == '.')) {
            current.push(c);
        } else if !current.is_empty() {
            let number = current.trim_end_matches([',', '.']).replace(',', "");
            found.extend(number.parse::<f64>().ok());
            current.clear();
        }
    }
    found
}

/// Every h:mm (or h.mm) in [text], as minutes past 12 or 0 o'clock (so 8:40 pm and 20:40 match).
fn minutes_said(text: &str) -> Vec<i64> {
    let chars: Vec<char> = text.chars().collect();
    (0..chars.len()).filter(|&i| (chars[i] == ':' || chars[i] == '.') && i > 0).filter_map(|i| {
        let hour: String = chars[..i].iter().rev().take_while(|c| c.is_ascii_digit()).collect::<Vec<_>>().into_iter().rev().collect();
        let minute: String = chars[i + 1..].iter().take(2).collect();
        let (h, m) = (hour.parse::<i64>().ok()?, minute.parse::<i64>().ok()?);
        (minute.len() == 2 && hour.len() <= 2 && chars.get(i + 3).is_none_or(|c| !c.is_ascii_digit())).then_some(h % 12 * 60 + m)
    }).collect()
}

fn lucky(name: &str) -> f64 {
    let out = Command::new(app().join("tests/models/skills/lucky-number/lucky.sh")).arg(name).output().unwrap();
    String::from_utf8_lossy(&out.stdout).trim().parse().unwrap()
}

/// Markdown or emoji, which shouldn't be spoken.
fn formatted(text: &str) -> bool {
    text.contains("**") || text.contains('`') || text.lines().any(|line| {
        let line = line.trim_start();
        line.starts_with('#') || line.starts_with("- ") || line.starts_with("* ")
            || line.split_once(". ").is_some_and(|(n, _)| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()))
    }) || text.chars().any(|c| ('\u{1F300}'..='\u{1FAFF}').contains(&c) || ('\u{2600}'..='\u{27BF}').contains(&c))
}

/// Skills Claude Code has from a user's own setup, which aren't Saambha's.
const NOT_SAAMBHAS: [&str; 12] = ["pdf", "spreadsheet", "slides", "powerpoint", "excel", "word document", "google docs",
                                  "google drive", "google workspace", "code review", "deep research", "linux setup"];

/// The words a reply naming the current sound output would use: from its name, and for HDMI what it is.
fn sound_output_words() -> Option<Vec<String>> {
    let default = Command::new("pactl").arg("get-default-sink").output().ok()?;
    let default = String::from_utf8_lossy(&default.stdout).trim().to_string();
    let sinks: Value = serde_json::from_slice(&Command::new("pactl").args(["-f", "json", "list", "sinks"]).output().ok()?.stdout).ok()?;
    let description = sinks.as_array()?.iter().find(|s| s["name"] == default.as_str())?["description"].as_str()?.to_lowercase();
    let generic = ["output", "stereo", "digital", "audio", "sof", "hda", "dsp", "analog"];
    let mut words: Vec<String> = description.split(|c: char| !c.is_ascii_alphanumeric()).filter(|w| w.len() > 2 && !generic.contains(w)).map(String::from).collect();
    if words.iter().any(|w| w == "hdmi" || w == "displayport") {
        words.extend(["monitor", "display", "screen", "tv"].map(String::from));
    }
    Some(words)
}

type Task = for<'a> fn(&'a mut Agent) -> saambha::server::BoxFuture<'a, Outcome>;

fn tasks() -> Vec<(&'static str, Task)> {
    vec![
        ("time", |a| Box::pin(async move {
            let now = glib::DateTime::now_local().unwrap();
            let reply = a.ask("What time is it?").await;
            let expected = i64::from(now.hour() % 12 * 60 + now.minute());
            check(minutes_said(&reply).iter().any(|m| (m - expected).abs().min(720 - (m - expected).abs()) <= 2), &reply)
        })),
        ("weekday", |a| Box::pin(async move {
            let reply = a.ask("What day of the week is it today?").await.to_lowercase();
            let today = glib::DateTime::now_local().unwrap().format("%A").unwrap().to_lowercase();
            let days = ["monday", "tuesday", "wednesday", "thursday", "friday", "saturday", "sunday"];
            check(days.iter().filter(|d| reply.contains(*d)).collect::<Vec<_>>() == [&today.as_str()], &reply)
        })),
        ("free_disk_space", |a| Box::pin(async move {
            let reply = a.ask("How much free space is on my home disk, in gigabytes?").await;
            // SAFETY: statvfs fills in the struct it's given.
            let free = unsafe {
                let mut stat: libc::statvfs = std::mem::zeroed();
                let home = std::ffi::CString::new(glib::home_dir().to_string_lossy().as_bytes()).unwrap();
                libc::statvfs(home.as_ptr(), &mut stat);
                stat.f_bavail as f64 * stat.f_frsize as f64
            };
            check(numbers(&reply).iter().any(|said| [free / 1e9, free / 1073741824.0].iter().any(|size| (said - size).abs() <= size * 0.1)), &reply)
        })),
        ("lines_in_a_file", |a| Box::pin(async move {
            let dir = tempfile::tempdir().unwrap();
            let notes = dir.path().join("notes.txt");
            std::fs::write(&notes, (0..137).map(|n| format!("note {n}\n")).collect::<String>()).unwrap();
            let reply = a.ask(&format!("How many lines are in {}?", notes.display())).await;
            check(numbers(&reply).contains(&137.0), &reply)
        })),
        ("lists_only_its_own_skills", |a| Box::pin(async move {
            let reply = a.ask("Which skills do you have?").await;
            let lower = reply.to_lowercase();
            check(lower.contains("alarm") && (lower.contains("spotify") || lower.contains("music")) && lower.contains("lucky")
                  && !NOT_SAAMBHAS.iter().any(|s| lower.contains(s)), &reply)
        })),
        ("uses_a_skill", |a| Box::pin(async move {
            let reply = a.ask("What is Priya's lucky number?").await;
            check(numbers(&reply).contains(&lucky("Priya")), &reply)
        })),
        ("uses_a_skill_twice", |a| Box::pin(async move {
            let reply = a.ask("What do Priya's and Arjun's lucky numbers add up to?").await;
            check(numbers(&reply).contains(&(lucky("Priya") + lucky("Arjun"))), &reply)
        })),
        ("a_misheard_request", |a| Box::pin(async move {
            let reply = a.ask("What's Priya's lucky dumber?").await;
            check(numbers(&reply).contains(&lucky("Priya")), &reply)
        })),
        ("says_which_speaker_sound_is_on", |a| Box::pin(async move {
            let Some(expected) = sound_output_words() else { return Err("no pactl".into()) };
            let reply = a.ask("Which speaker is sound playing on right now?").await;
            let words: Vec<String> = reply.to_lowercase().split(|c: char| !c.is_ascii_alphanumeric()).map(String::from).collect();
            check(expected.iter().any(|w| words.contains(w)), &reply)
        })),
        ("arithmetic", |a| Box::pin(async move {
            let reply = a.ask("What is 17 times 23?").await;
            check(numbers(&reply).contains(&391.0), &reply)
        })),
        ("a_big_question_gets_the_gist_and_the_rest_in_the_details", |a| Box::pin(async move {
            let reply = a.ask("What's the difference between git merge and git rebase?").await;
            let details = a.replies.last().unwrap().details.to_lowercase();
            check(reply.split_whitespace().count() <= LONG_REPLY && details.contains("rebase"), &reply)
        })),
        ("a_failure_in_a_sentence", |a| Box::pin(async move {
            let reply = a.ask("Open the zorblaxify app.").await;
            check(reply.split_whitespace().count() <= LONG_REPLY, &reply)
        })),
        ("small_talk_gets_a_few_words_back", |a| Box::pin(async move {
            // the second, a stray word speech recognition picked up
            for said in ["What's up?", "You"] {
                let reply = a.ask(said).await;
                let lower = reply.to_lowercase();
                check(reply.split_whitespace().count() <= 10 && !["help", "assist", "saambha"].iter().any(|w| lower.contains(w)), &reply)?;
            }
            Ok(())
        })),
        ("follow_up", |a| Box::pin(async move {
            a.ask("My locker code is 4821.").await;
            let reply = a.ask("What was my locker code?").await;
            check(numbers(&reply).contains(&4821.0), &reply)
        })),
        ("a_skill_after_a_long_conversation", |a| Box::pin(async move {
            a.chat(&["What time is it?", "How much free space is on my home disk?", "Which speaker is sound playing on?",
                     "What is 12 times 12?", "Thanks, that's all for now.", "Actually, one more: how many CPU cores do I have?"]).await;
            let reply = a.ask("What is Arjun's lucky number?").await;
            check(numbers(&reply).contains(&lucky("Arjun")), &reply)
        })),
    ]
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &to.join(entry.file_name()));
        } else {
            std::fs::copy(entry.path(), to.join(entry.file_name())).unwrap();
        }
    }
}

/// The real prompt and guards, with the sample skills and the benchmark's own.
fn bench_workspace(dir: &Path) -> PathBuf {
    let workspace = dir.join("workspace");
    for file in ["system-prompt.md", ".claude/settings.json", ".pi/guard.ts"] {
        std::fs::create_dir_all(workspace.join(file).parent().unwrap()).unwrap();
        std::fs::copy(app().join("../agent").join(file), workspace.join(file)).unwrap();
    }
    copy_dir(&app().join("tests/fixtures/skills"), &workspace.join("skills"));
    copy_dir(&app().join("tests/models/skills"), &workspace.join("skills"));
    workspace
}

/// The user's settings, for [harness] with [model], in [workspace], with a fresh session in [dir].
fn settings_for(harness: &str, model: &str, workspace: &Path, dir: &Path) -> Settings {
    let env: HashMap<String, String> = settings::env_file(&app().join(".env"), |_| false).into_iter().collect();
    let mut vars: HashMap<String, String> = std::env::vars().chain(env).collect();
    for (name, value) in [("HARNESS", harness.to_string()), (&format!("{harness}_MODEL").to_uppercase(), model.to_string()),
                          ("WORKSPACE", workspace.to_string_lossy().into()), ("SKILLS_DIR", workspace.join("skills").to_string_lossy().into()),
                          ("SESSION_FILE", dir.join("session_id").to_string_lossy().into()),
                          ("PI_SESSION_DIR", dir.join("pi-sessions").to_string_lossy().into())] {
        vars.insert(format!("SAAMBHA_{name}"), value);
    }
    let mut settings = Settings::new(&app(), &glib::home_dir(), |name| vars.get(name).cloned()).unwrap();
    if let Some(local) = model.strip_prefix("ollama/").filter(|_| harness == "pi") {
        // pi only knows the Ollama models listed in its config, so give it one listing this model.
        let agent_dir = dir.join("pi-agent");
        std::fs::create_dir_all(&agent_dir).unwrap();
        std::fs::write(agent_dir.join("models.json"), with_ollama_model(json!({}), &settings.ollama_url, local, settings.ollama_context).to_string()).unwrap();
        let wrapper = dir.join("pi");
        std::fs::write(&wrapper, format!("#!/bin/sh\nPI_CODING_AGENT_DIR='{}' exec '{}' \"$@\"\n", agent_dir.display(), settings.pi_bin)).unwrap();
        std::fs::set_permissions(&wrapper, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
        settings.pi_bin = wrapper.to_string_lossy().into();
    }
    settings
}

/// Every file under [folder], but the chat history and pi's sessions (the live server's).
fn snapshot(folder: &Path, settings: &Settings) -> BTreeMap<PathBuf, Vec<u8>> {
    let mut files = BTreeMap::new();
    let mut folders = vec![folder.to_path_buf()];
    while let Some(dir) = folders.pop() {
        for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            let path = entry.path();
            if path == settings.history_file || path.starts_with(&settings.pi_session_dir) {
                continue;
            }
            if path.is_dir() {
                folders.push(path);
            } else if let Ok(bytes) = std::fs::read(&path) {
                files.insert(path, bytes);
            }
        }
    }
    files
}

/// Puts back what an agent added or changed in Saambha's data folder (notes, learnings): what it changed.
fn put_back(folder: &Path, before: &BTreeMap<PathBuf, Vec<u8>>, settings: &Settings) -> Vec<PathBuf> {
    let after = snapshot(folder, settings);
    let changed: Vec<PathBuf> = after.keys().chain(before.keys()).filter(|p| after.get(*p) != before.get(*p)).cloned()
        .collect::<std::collections::BTreeSet<_>>().into_iter().collect();
    for path in &changed {
        match before.get(path) {
            Some(bytes) => std::fs::write(path, bytes).unwrap(),
            None => std::fs::remove_file(path).unwrap(),
        }
    }
    changed
}

fn median(seconds: impl Iterator<Item = f64>) -> String {
    let mut all: Vec<f64> = seconds.collect();
    if all.is_empty() {
        return "-".into();
    }
    all.sort_by(f64::total_cmp);
    format!("{:.1}s", all[all.len() / 2])
}

struct Run {
    task: &'static str,
    model: String,
    passed: bool,
    replies: Vec<Reply>,
    changed: Vec<PathBuf>,
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let option = |name: &str| args.iter().position(|a| a == name).and_then(|at| args.get(at + 1)).cloned();
    if !args.iter().any(|a| a == "--run") {
        println!("models: skipped (it runs the real models; see tests/models.rs to run it)");
        return;
    }
    let runs: usize = option("--runs").map_or(3, |n| n.parse().unwrap());
    let user = Settings::new(&app(), &glib::home_dir(), |name| {
        std::env::var(name).ok().or_else(|| settings::env_file(&app().join(".env"), |_| false).into_iter().find(|(k, _)| k == name).map(|(_, v)| v))
    }).unwrap();
    let models: Vec<(String, String)> = match option("--models") {
        Some(chosen) => chosen.split(',').map(|pair| pair.split_once(':').map(|(h, m)| (h.into(), m.into())).unwrap()).collect(),
        None => vec![("claude".into(), user.claude_model.clone()), ("pi".into(), user.pi_model.clone())],
    };
    let only = option("--only");
    let data = user.history_file.parent().unwrap().to_path_buf();
    let dir = tempfile::tempdir().unwrap();
    let workspace = bench_workspace(dir.path());
    let runtime = tokio::runtime::Runtime::new().unwrap();

    let mut results: Vec<Run> = Vec::new();
    for (harness, model) in &models {
        let label = format!("{harness}:{model}");
        // Loaded once first, so the first command isn't timed loading it.
        let warm = tempfile::tempdir().unwrap();
        let warmed = compose::harness(&settings_for(harness, model, &workspace, warm.path())).unwrap();
        runtime.block_on(warmed.ask("Say OK.", &|_| {}));
        for (task, run) in tasks().into_iter().filter(|(task, _)| only.as_deref().is_none_or(|o| o == *task)) {
            for n in 1..=runs {
                let session = tempfile::tempdir().unwrap();
                let settings = settings_for(harness, model, &workspace, session.path());
                let mut agent = Agent { harness: compose::harness(&settings).unwrap(), replies: Vec::new() };
                let before = snapshot(&data, &user);
                let outcome = runtime.block_on(async { tokio::time::timeout(Duration::from_secs(600), run(&mut agent)).await });
                let changed = put_back(&data, &before, &user);
                let passed = matches!(outcome, Ok(Ok(())));
                match &outcome {
                    Ok(Err(reply)) => println!("FAILED {label} {task} run{n}: {reply:?}"),
                    Err(_) => println!("FAILED {label} {task} run{n}: took over 10 minutes"),
                    Ok(Ok(())) => println!("ok     {label} {task} run{n}"),
                }
                results.push(Run { task, model: label.clone(), passed, replies: agent.replies, changed });
            }
        }
    }
    summary(&results, &models.iter().map(|(h, m)| format!("{h}:{m}")).collect::<Vec<_>>(), args.iter().any(|a| a == "-v"));
}

fn runs_of<'a>(results: &'a [Run], model: &'a str) -> impl Iterator<Item = &'a Run> {
    results.iter().filter(move |r| r.model == model)
}

fn replies_of<'a>(results: &'a [Run], model: &'a str) -> Vec<&'a Reply> {
    runs_of(results, model).flat_map(|r| r.replies.iter()).collect()
}

fn summary(results: &[Run], models: &[String], verbose: bool) {
    let mut tasks: Vec<&str> = Vec::new();
    for run in results {
        if !tasks.contains(&run.task) {
            tasks.push(run.task);
        }
    }
    let width = tasks.iter().map(|t| t.len()).chain(["left changes behind".len()]).max().unwrap() + 2;
    let columns: Vec<usize> = models.iter().map(|m| m.len().max(14) + 2).collect();
    let row = |name: &str, values: Vec<String>| {
        println!("{name:width$}{}", values.iter().zip(&columns).map(|(v, w)| format!("{v:w$}")).collect::<String>());
    };
    println!("\n==== models ====");
    row("", models.to_vec());
    for task in &tasks {
        row(task, models.iter().map(|model| {
            let done: Vec<&Run> = results.iter().filter(|r| r.task == *task && &r.model == model).collect();
            let passes = done.iter().filter(|r| r.passed).count();
            let mark = if passes == done.len() { "" } else { "  <" };
            format!("{passes}/{} {:>6}{mark}", done.len(), median(done.iter().flat_map(|r| r.replies.iter().map(|x| x.seconds))))
        }).collect());
    }
    row("passed", models.iter().map(|m| format!("{}/{}", runs_of(results, m).filter(|r| r.passed).count(), runs_of(results, m).count())).collect());
    row("plain speech", models.iter().map(|m| format!("{}/{}", replies_of(results, m).iter().filter(|r| !formatted(&r.text)).count(), replies_of(results, m).len())).collect());
    row(&format!("over {LONG_REPLY} words"), models.iter().map(|m| format!("{}/{}", replies_of(results, m).iter().filter(|r| r.text.split_whitespace().count() > LONG_REPLY).count(), replies_of(results, m).len())).collect());
    row("left changes behind", models.iter().map(|m| format!("{}/{}", runs_of(results, m).filter(|r| !r.changed.is_empty()).count(), runs_of(results, m).count())).collect());
    row("median reply", models.iter().map(|m| median(replies_of(results, m).iter().map(|r| r.seconds))).collect());
    row("total", models.iter().map(|m| format!("{:.0}s", replies_of(results, m).iter().map(|r| r.seconds).sum::<f64>())).collect());
    if verbose {
        for run in results {
            for reply in &run.replies {
                println!("{}{} {} | {} -> {:?}", if run.passed { "" } else { "FAILED " }, run.model, run.task, reply.said, reply.text);
            }
            if !run.changed.is_empty() {
                println!("{} {} | changed (put back): {:?}", run.model, run.task, run.changed);
            }
        }
    }
}
