//! The Saambha desktop app: the dot, floating above the other windows and showing what Saambha is
//! doing, and the chat panel that grows out of it. It runs alongside the Saambha server:
//!     saambha-desktop <server url> <the server's CA file>

mod chat;
mod desktop;
mod dot;
mod icon;
mod layer_shell;
mod layout;
mod logs;
mod motion;
mod overlay;
mod panel;
mod saved;
mod server;
mod status;
mod theme;
mod x11;

use std::cell::RefCell;
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::rc::Rc;

use gtk::prelude::*;
use gtk::{gdk, gio, glib};

use crate::desktop::Desktop;
use crate::saved::SavedPlace;
use crate::server::Server;

fn main() -> glib::ExitCode {
    // Drawn on the CPU: its few small changes cost about the same as on the GPU. GDK would still
    // set up GL, loading drivers that take two thirds of its memory, unless told not to (by
    // GDK_DEBUG before GTK 4.16, GDK_DISABLE after).
    for (name, value) in [("GSK_RENDERER", "cairo"), ("GDK_DEBUG", "gl-disable"), ("GDK_DISABLE", "gl")] {
        if std::env::var_os(name).is_none() {
            // SAFETY: no other threads yet.
            unsafe { std::env::set_var(name, value) };
        }
    }
    if overlay::needs_x11() {
        let error = std::process::Command::new(std::env::current_exe().expect("this program's path"))
            .args(std::env::args_os().skip(1))
            .env("GDK_BACKEND", "x11")
            .exec();
        eprintln!("Can't start over under X11: {error}");
        std::process::exit(1);
    }

    let args: Vec<String> = std::env::args().collect();
    let [program, url, ca_file] = args.as_slice() else {
        eprintln!("Usage: saambha-desktop <server url> <the server's CA file>");
        std::process::exit(2);
    };
    // The server's requests run here; one thread is plenty for a few at a time.
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .expect("a runtime for the server's requests");
    let server = Server::new(url, ca_file, runtime.handle().clone()).unwrap_or_else(|e| {
        eprintln!("{e}");
        std::process::exit(1);
    });
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
    app.run_with_args(&[program])
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
