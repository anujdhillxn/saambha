//! Saambha: the server the phone, watch and hotkey talk to, running the agent that does what they
//! ask, and the dot floating above the other windows that shows what it's doing, with the chat panel
//! that grows out of it.
//!     saambha            runs it
//!     saambha settings   prints the settings as shell variables, for scripts/install.sh

use std::cell::RefCell;
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::rc::Rc;

use gtk::prelude::*;
use gtk::{gdk, gio, glib};

use saambha::server::logs::{self, Logger};
use saambha::server::settings::{self, Settings};
use saambha::server::compose;
use saambha::ui::desktop::Desktop;
use saambha::ui::saved::SavedPlace;
use saambha::ui::server::Server;
use saambha::ui::{overlay, theme};

fn main() -> glib::ExitCode {
    // Drawn on the CPU: its few small changes cost about the same as on the GPU. GDK would still
    // set up GL, loading drivers that take two thirds of its memory, unless told not to (by
    // GDK_DEBUG before GTK 4.16, GDK_DISABLE after).
    let mut environment: Vec<(String, String)> =
        [("GSK_RENDERER", "cairo"), ("GDK_DEBUG", "gl-disable"), ("GDK_DISABLE", "gl")].map(|(k, v)| (k.into(), v.into())).into();
    // Settings and secrets, in the environment for the agent's tools (e.g. the Spotify skill) too.
    let root = settings::root();
    environment.extend(settings::env_file(&root.join(".env"), |_| false));
    for (name, value) in environment {
        if std::env::var_os(&name).is_none() {
            // SAFETY: no other threads yet.
            unsafe { std::env::set_var(name, value) };
        }
    }
    let settings = Settings::new(&root, &glib::home_dir(), |name| std::env::var(name).ok()).unwrap_or_else(|e| {
        eprintln!("{e}");
        std::process::exit(2);
    });
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        None => {}
        Some("settings") => {
            print!("{}", settings.shell());
            return glib::ExitCode::SUCCESS;
        }
        Some(_) => {
            eprintln!("Usage: saambha [settings]");
            std::process::exit(2);
        }
    }
    if overlay::needs_x11() {
        let error = std::process::Command::new(std::env::current_exe().expect("this program's path"))
            .env("GDK_BACKEND", "x11")
            .exec();
        eprintln!("Can't start over under X11: {error}");
        std::process::exit(1);
    }

    logs::configure(Logger::new(logs::level(&settings.log_level),
                                Some((settings.log_file.clone(), settings.log_max_bytes, settings.log_backups)), true));
    // The server runs here; one thread is plenty for the few things it does at a time.
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .expect("a runtime for the server");
    let services = runtime.block_on(compose::start(&settings)).unwrap_or_else(|e| {
        log::error!("{e}");
        std::process::exit(1);
    });
    let server = Server::new(services, runtime.handle().clone());
    let state_dir = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| glib::home_dir().join(".local/state"));
    let saved = state_dir.join("saambha").join("dot.json");

    let app = gtk::Application::builder()
        .application_id("dev.saambha.desktop")
        .flags(gio::ApplicationFlags::NON_UNIQUE)
        .build();
    app.connect_startup(|_| style());
    let desktop = Rc::new(RefCell::new(None)); // held while the app runs
    app.connect_activate(move |app| {
        let started = Desktop::start(app, server.clone(), SavedPlace::new(saved.clone()));
        desktop.replace(Some(started));
    });
    app.run_with_args(&[&args[0]])
}

fn style() {
    let display = gdk::Display::default().expect("a display");
    let settings = gtk::Settings::for_display(&display);
    settings.set_gtk_theme_name(Some("Adwaita")); // the same look on every desktop
    settings.set_gtk_application_prefer_dark_theme(true);
    let provider = gtk::CssProvider::new();
    provider.load_from_string(&theme::stylesheet());
    gtk::style_context_add_provider_for_display(&display, &provider, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION);
}
