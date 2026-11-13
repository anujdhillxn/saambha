//! The log window: the server's log, following it as it grows while the window's open.

use std::time::Duration;

use gtk::glib;
use gtk::prelude::*;

use crate::server::Server;

const POLL: Duration = Duration::from_millis(700);
const KEEP: i32 = 2_000_000; // characters shown, the oldest dropped beyond that
const NAME: &str = "logs";

/// Opens the log window, or brings it forward if it's open.
pub fn open(app: &gtk::Application, server: &Server) {
    if let Some(window) = app.windows().into_iter().find(|w| w.widget_name() == NAME) {
        window.present();
        return;
    }
    let view = gtk::TextView::builder()
        .editable(false)
        .cursor_visible(false)
        .monospace(true)
        .wrap_mode(gtk::WrapMode::WordChar)
        .left_margin(14)
        .right_margin(14)
        .top_margin(10)
        .bottom_margin(10)
        .css_classes(["log"])
        .build();
    let scroller = gtk::ScrolledWindow::builder().child(&view).build();
    let window = gtk::ApplicationWindow::builder()
        .application(app)
        .name(NAME)
        .title("Saambha logs")
        .default_width(1100)
        .default_height(650)
        .child(&scroller)
        .build();
    window.present();

    let view = view.downgrade();
    let server = server.clone();
    glib::spawn_future_local(async move {
        let mut offset = None;
        loop {
            let piece = server.log(offset).await;
            let Some(view) = view.upgrade() else { break }; // the window's closed
            if let Ok(piece) = piece {
                offset = Some(piece.offset);
                append(&view, &piece.text);
            } // else the server's down: try again next time
            drop(view);
            glib::timeout_future(POLL).await;
        }
    });
}

fn append(view: &gtk::TextView, text: &str) {
    if text.is_empty() {
        return;
    }
    let buffer = view.buffer();
    let adjustment = view.vadjustment().expect("a scrolled view");
    let at_end = adjustment.value() + adjustment.page_size() >= adjustment.upper() - 4.0;
    buffer.insert(&mut buffer.end_iter(), text);
    let extra = buffer.char_count() - KEEP;
    if extra > 0 {
        buffer.delete(&mut buffer.start_iter(), &mut buffer.iter_at_offset(extra));
    }
    if at_end {
        // don't yank the view away from what's being read
        let end = buffer.mark("end").unwrap_or_else(|| buffer.create_mark(Some("end"), &buffer.end_iter(), false));
        view.scroll_to_mark(&end, 0.0, false, 0.0, 1.0);
    }
}
