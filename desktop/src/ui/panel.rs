//! The chat panel's card: what Saambha is doing, the conversation, a box to type a command in, and
//! what else can be done about it.

use std::cell::RefCell;
use std::rc::Rc;

use gtk::prelude::*;
use gtk::{glib, pango};

use crate::server::history::Entry;
use crate::server::status::{Activity, State};
use crate::ui::chat::{self, Row};
use crate::ui::icon;
use crate::ui::server::Server;

const SHOWN: usize = 50; // entries of the history
const WIDTH: i32 = 400;
const TALLEST: i32 = 740;
const BUBBLE_CHARS: i32 = 40; // a bubble's widest, roughly

pub struct Panel {
    server: Server,
    card: gtk::Box,
    light: gtk::Box,
    line: gtk::Label,
    pairing: gtk::Box,
    asking: gtk::Label,
    stop: gtk::Button,
    chat: gtk::Box,
    scroller: gtk::ScrolledWindow,
    text: gtk::Entry,
    shown: RefCell<Option<Vec<Entry>>>,
}

impl Panel {
    /// The card; [close] puts it away, and [logs] opens the detailed logs.
    pub fn new(server: Server, close: impl Fn() + 'static, logs: impl Fn() + 'static) -> Rc<Self> {
        let light = gtk::Box::builder().css_classes(["light", "cold"]).valign(gtk::Align::Center).build();
        let line = gtk::Label::builder().css_classes(["line"]).xalign(0.0).ellipsize(pango::EllipsizeMode::End).build();
        let heading = gtk::Box::builder().orientation(gtk::Orientation::Vertical).hexpand(true).build();
        heading.append(&gtk::Label::builder().label("Saambha").css_classes(["title"]).xalign(0.0).build());
        heading.append(&line);
        let logs_button = icon_button(icon::LOGS, "Detailed logs");
        let new_button = icon_button(icon::NEW, "New conversation");
        let close_button = icon_button(icon::CLOSE, "Close");
        let header = gtk::Box::builder().css_classes(["header"]).spacing(8).build();
        for part in [light.upcast_ref::<gtk::Widget>(), heading.upcast_ref(), logs_button.upcast_ref(), new_button.upcast_ref(), close_button.upcast_ref()] {
            header.append(part);
        }

        let asking = gtk::Label::builder().hexpand(true).xalign(0.0).wrap(true).build();
        let deny = gtk::Button::with_label("Deny");
        let allow = gtk::Button::builder().label("Allow").css_classes(["primary"]).build();
        let pairing = gtk::Box::builder().css_classes(["pairing"]).spacing(8).visible(false).build();
        pairing.append(&asking);
        pairing.append(&deny);
        pairing.append(&allow);

        let chat = gtk::Box::builder().orientation(gtk::Orientation::Vertical).css_classes(["chat"]).spacing(6).build();
        let scroller = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vexpand(true)
            .child(&chat)
            .build();

        let stop = gtk::Button::builder().label("Stop speaking").halign(gtk::Align::Center).visible(false).build();
        let text = gtk::Entry::builder().placeholder_text("Type a command").hexpand(true).build();
        let send_button = icon_button(icon::SEND, "Send");
        send_button.add_css_class("primary");
        let compose = gtk::Box::builder().spacing(8).build();
        compose.append(&text);
        compose.append(&send_button);
        let footer = gtk::Box::builder().orientation(gtk::Orientation::Vertical).css_classes(["footer"]).spacing(8).build();
        footer.append(&stop);
        footer.append(&compose);

        let card = gtk::Box::builder().orientation(gtk::Orientation::Vertical).css_classes(["card"]).width_request(WIDTH).build();
        for part in [header.upcast_ref::<gtk::Widget>(), pairing.upcast_ref(), scroller.upcast_ref(), footer.upcast_ref()] {
            card.append(part);
        }

        let panel = Rc::new(Panel { server, card, light, line, pairing, asking, stop, chat, scroller, text, shown: RefCell::new(None) });
        close_button.connect_clicked(move |_| close());
        logs_button.connect_clicked(move |_| logs());
        new_button.connect_clicked(glib::clone!(#[weak] panel, move |_| {
            panel.server.new_conversation();
            panel.show(None);
        }));
        for (button, allowed) in [(allow, true), (deny, false)] {
            let server = panel.server.clone();
            button.connect_clicked(move |_| server.answer_pairing(allowed));
        }
        panel.stop.connect_clicked(glib::clone!(#[weak] panel, move |_| panel.server.stop_speaking()));
        panel.text.connect_activate(glib::clone!(#[weak] panel, move |_| panel.send()));
        send_button.connect_clicked(glib::clone!(#[weak] panel, move |_| panel.send()));
        panel
    }

    pub fn widget(&self) -> &gtk::Box {
        &self.card
    }

    /// Its size on a screen [height] pixels tall.
    pub fn size(height: i32) -> (i32, i32) {
        (WIDTH, TALLEST.min(height - 16))
    }

    /// Puts the keyboard in the box to type a command in, and the latest of the chat in view.
    pub fn focus(&self) {
        self.text.grab_focus();
        self.scroll_to_end();
    }

    /// Shows [status] (else keeps what's shown) and the latest history.
    pub fn show(&self, status: Option<&Activity>) {
        if let Some(status) = status {
            let (state, line) = status.look();
            self.light.set_css_classes(&["light", state.name()]);
            self.line.set_label(&line);
            self.line.set_visible(state != State::Ready);
            self.pairing.set_visible(status.pairing.is_some());
            self.asking.set_label(&status.pairing.as_ref().map(|p| format!("{p} wants to connect")).unwrap_or_default());
            self.stop.set_visible(status.speaking);
        }
        self.show_chat(self.server.history(SHOWN));
    }

    /// Runs what's typed as a command, showing it at once; the reply comes with the history.
    fn send(self: &Rc<Self>) {
        let text = self.text.text().trim().to_string();
        if text.is_empty() {
            return;
        }
        self.text.set_text("");
        if let Some(empty) = self.chat.first_child().filter(|c| c.has_css_class("empty")) {
            self.chat.remove(&empty);
        }
        let pending = bubble("said", &text);
        pending.add_css_class("pending");
        self.chat.append(&pending);
        self.scroll_to_end();
        let panel = self.clone();
        glib::spawn_future_local(async move {
            panel.server.send_command(&text).await;
            panel.show(None);
        });
    }

    fn show_chat(&self, entries: Vec<Entry>) {
        if self.shown.borrow().as_ref() == Some(&entries) {
            return;
        }
        let rows = chat::rows(&entries, &glib::DateTime::now_local().expect("the time now"));
        *self.shown.borrow_mut() = Some(entries);
        clear(&self.chat);
        if rows.is_empty() {
            let empty = gtk::Label::builder()
                .label("Your commands and Saambha's replies show up here")
                .css_classes(["empty"])
                .wrap(true)
                .justify(gtk::Justification::Center)
                .vexpand(true)
                .build();
            self.chat.append(&empty);
        }
        for row in rows {
            let label = match row {
                Row::When(time) => gtk::Label::builder().label(time).css_classes(["when"]).build(),
                Row::Said(text) => bubble("said", &text),
                Row::Reply(text) => bubble("reply", &text),
                Row::Details(text) => bubble("details", &text),
            };
            self.chat.append(&label);
        }
        self.scroll_to_end();
    }

    /// Scrolls to the end once the chat's laid out.
    fn scroll_to_end(&self) {
        let adjustment = self.scroller.vadjustment();
        glib::idle_add_local_once(move || adjustment.set_value(adjustment.upper() - adjustment.page_size()));
    }
}

fn icon_button(strokes: icon::Strokes, tooltip: &str) -> gtk::Button {
    gtk::Button::builder().child(&icon::new(strokes)).css_classes(["icon"]).tooltip_text(tooltip).build()
}

fn bubble(kind: &str, text: &str) -> gtk::Label {
    gtk::Label::builder()
        .label(text)
        .css_classes([kind])
        .halign(if kind == "said" { gtk::Align::End } else { gtk::Align::Start })
        .xalign(0.0)
        .wrap(true)
        .wrap_mode(pango::WrapMode::WordChar)
        .max_width_chars(BUBBLE_CHARS)
        .build()
}

fn clear(container: &gtk::Box) {
    while let Some(child) = container.first_child() {
        container.remove(&child);
    }
}
