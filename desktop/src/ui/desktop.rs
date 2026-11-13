//! The app: the dot, floating above the other windows and showing what Saambha is doing, and the
//! stage below it, see-through over the whole screen. A click on the dot opens the chat panel (or,
//! while Saambha speaks, stops it) and a drag moves it: while dragged, the dot's window stays where
//! the drag started, so the pointer's positions there stay steady, and the stage draws the dot under
//! the pointer instead; once dropped, the dot's window goes there. The stage shows the chat panel,
//! growing out of the dot, and takes clicks only while that's open, so that a click away from the
//! panel puts it away; otherwise they go to the windows below.

use std::cell::RefCell;
use std::rc::{Rc, Weak};
use std::time::Duration;

use gtk::prelude::*;
use gtk::{gdk, glib, graphene, gsk};

use crate::ui::dot::{self, DotView, Look};
use crate::ui::layout::{self, Point, Rect, Size};
use crate::ui::logs;
use crate::ui::motion::{Ease, Tween};
use crate::ui::overlay;
use crate::ui::panel::Panel;
use crate::ui::saved::SavedPlace;
use crate::ui::server::Server;
use crate::server::status::Activity;

const EDGE: i32 = 8; // the dot's first distance from the screen's corner
const MARGIN: i32 = 8; // the panel's least distance from the screen's edges
const GAP: i32 = 4; // between the dot and the panel
const DRAG_FROM: f64 = 4.0; // pixels the pointer moves with the button down before it's a drag, not a click
const SETTLE: Duration = Duration::from_millis(200); // after a drop, while the dot fades back in where dropped
const SHRUNK: f64 = 0.2; // the panel's size, put away, as it grows out of the dot and back in
const FAST: Duration = Duration::from_millis(150);
const MEDIUM: Duration = Duration::from_millis(250);

pub struct Desktop {
    app: gtk::Application,
    server: Server,
    saved: SavedPlace,
    dot_window: gtk::Window,
    dot: DotView,
    stage: gtk::Window,
    fixed: gtk::Fixed,
    dragged: DotView,
    panel: Rc<Panel>,
    state: RefCell<Stage>,
}

struct Stage {
    screen: Size,
    at: Rect, // the dot's box
    press: Option<Press>,
    drag_from: Option<Rect>, // where the dot was when the drag started
    settling: Option<glib::SourceId>,
    open: bool,
    status: Activity,
    asked: Option<String>, // the device last asking to connect
    card: layout::Panel,
    grow: Tween,
    fade: Tween,
    card_ticking: bool,
}

/// The pointer's button, down on the dot, and how far it's been dragged.
struct Press {
    moved: Option<(f64, f64)>,
}

impl Desktop {
    /// Puts the dot and its stage on the screen.
    pub fn start(app: &gtk::Application, server: Server, saved: SavedPlace) -> Rc<Self> {
        let look = Look { state: Activity::default().look().0, speaking: false };
        let dot = DotView::new(look, false);
        let dot_window = overlay_window(app, dot.widget(), Some((dot::BOX, dot::BOX)));
        // Faded away, not hidden, while it isn't dragged: GTK doesn't draw a window with nothing in
        // it, which would leave the panel's last frame on the screen as it's put away.
        let dragged = DotView::new(look, true);
        dragged.widget().set_can_target(false);
        let fixed = gtk::Fixed::new();
        fixed.put(dragged.widget(), 0.0, 0.0);
        let stage = overlay_window(app, &fixed, None);

        let desktop = Rc::new_cyclic(|me: &Weak<Desktop>| {
            let panel = Panel::new(
                server.clone(),
                glib::clone!(#[strong] me, move || me.upgrade().iter().for_each(|d| d.close_panel())),
                glib::clone!(#[strong] me, move || me.upgrade().iter().for_each(|d| logs::open(&d.app, &d.server))),
            );
            panel.widget().set_visible(false);
            fixed.put(panel.widget(), 0.0, 0.0);
            Desktop {
                app: app.clone(),
                server,
                saved,
                dot_window,
                dot,
                stage,
                fixed,
                dragged,
                panel,
                state: RefCell::new(Stage {
                    screen: Size { width: 0, height: 0 },
                    at: Rect { x: 0, y: 0, width: dot::BOX, height: dot::BOX },
                    press: None,
                    drag_from: None,
                    settling: None,
                    open: false,
                    status: Activity::default(),
                    asked: None,
                    card: layout::Panel { rect: Rect { x: 0, y: 0, width: 0, height: 0 }, up: false, from_right: false },
                    grow: Tween::still(0.0),
                    fade: Tween::still(0.0),
                    card_ticking: false,
                }),
            }
        });
        overlay::make_dot(&desktop.dot_window);
        overlay::make_stage(&desktop.stage);
        desktop.wire_dot();
        desktop.wire_stage();
        desktop.watch_status();
        desktop.stage.set_visible(true); // it then puts the dot where it goes, and shows it
        desktop
    }

    fn wire_dot(self: &Rc<Self>) {
        let area = self.dot.widget();
        let hover = gtk::EventControllerMotion::new();
        hover.connect_motion(glib::clone!(#[weak(rename_to = d)] self, move |_, x, y| d.hover(d.dot.on_disc(x, y))));
        hover.connect_leave(glib::clone!(#[weak(rename_to = d)] self, move |_| d.hover(false)));
        area.add_controller(hover);

        let drag = gtk::GestureDrag::builder().button(gdk::BUTTON_PRIMARY).build();
        drag.connect_drag_begin(glib::clone!(#[weak(rename_to = d)] self, move |gesture, x, y| {
            if !d.dot.on_disc(x, y) {
                gesture.set_state(gtk::EventSequenceState::Denied);
                return;
            }
            d.state.borrow_mut().press = Some(Press { moved: None });
            d.dot.set_pressed(true);
            d.dot.widget().set_cursor_from_name(Some("grabbing"));
        }));
        drag.connect_drag_update(glib::clone!(#[weak(rename_to = d)] self, move |_, dx, dy| {
            let first = {
                let mut state = d.state.borrow_mut();
                let Some(press) = state.press.as_mut() else { return };
                if press.moved.is_none() && dx.hypot(dy) < DRAG_FROM {
                    return;
                }
                press.moved.replace((dx, dy)).is_none()
            };
            d.dragged_by(dx, dy, first);
        }));
        drag.connect_drag_end(glib::clone!(#[weak(rename_to = d)] self, move |_, dx, dy| {
            let Some(press) = d.state.borrow_mut().press.take() else { return };
            d.dot.set_pressed(false);
            d.hover(d.dot.hovered());
            // Where it's let go, which can be further than the last move GTK passed on.
            if press.moved.is_some() || dx.hypot(dy) >= DRAG_FROM {
                d.dropped(dx, dy);
            } else if d.dot.look().speaking {
                d.server.stop_speaking();
            } else {
                d.toggle_panel();
            }
        }));
        drag.connect_cancel(glib::clone!(#[weak(rename_to = d)] self, move |_, _| {
            let Some(press) = d.state.borrow_mut().press.take() else { return };
            d.dot.set_pressed(false);
            d.hover(d.dot.hovered());
            if let Some((dx, dy)) = press.moved {
                d.dropped(dx, dy);
            }
        }));
        area.add_controller(drag);
    }

    fn hover(&self, hovered: bool) {
        self.dot.set_hovered(hovered);
        let pressed = self.state.borrow().press.is_some();
        let cursor = if pressed { Some("grabbing") } else if hovered { Some("grab") } else { None };
        self.dot.widget().set_cursor_from_name(cursor);
    }

    fn wire_stage(self: &Rc<Self>) {
        self.stage.connect_realize(glib::clone!(#[weak(rename_to = d)] self, move |_| {
            let surface = d.stage.surface().expect("a realized window");
            let me = Rc::downgrade(&d);
            surface.connect_layout(move |_, width, height| {
                if let Some(d) = me.upgrade() {
                    d.resized(Size { width, height });
                }
            });
        }));

        let click = gtk::GestureClick::builder().button(0).propagation_phase(gtk::PropagationPhase::Capture).build();
        click.connect_pressed(glib::clone!(#[weak(rename_to = d)] self, move |_, _, x, y| {
            if !d.state.borrow().open {
                return;
            }
            let card = d.panel.widget();
            let on_card = d.stage.pick(x, y, gtk::PickFlags::DEFAULT).is_some_and(|w| &w == card || w.is_ancestor(card));
            if on_card {
                // A window a command brought up can take the keyboard, and clicking a dock window
                // doesn't take it back (X11): the panel asks for it again.
                d.take_input();
            } else {
                d.close_panel();
            }
        }));
        self.stage.add_controller(click);

        let keys = gtk::EventControllerKey::new();
        keys.connect_key_pressed(glib::clone!(#[weak(rename_to = d)] self, #[upgrade_or] glib::Propagation::Proceed, move |_, key, _, _| {
            if key != gdk::Key::Escape {
                return glib::Propagation::Proceed;
            }
            d.close_panel();
            glib::Propagation::Stop
        }));
        self.stage.add_controller(keys);
    }

    fn watch_status(self: &Rc<Self>) {
        let me = Rc::downgrade(self);
        let mut changes = self.server.status();
        glib::spawn_future_local(async move {
            loop {
                let activity = changes.borrow_and_update().clone();
                let Some(d) = me.upgrade() else { break };
                d.show_status(activity);
                drop(d);
                if changes.changed().await.is_err() {
                    break;
                }
            }
        });
    }

    fn show_status(self: &Rc<Self>, status: Activity) {
        let (state, line) = status.look();
        self.dot.set_look(Look { state, speaking: status.just_speaking() });
        self.dot.widget().set_tooltip_text(Some(&line));
        let (open, asking) = {
            let mut state = self.state.borrow_mut();
            let asking = status.pairing.is_some() && status.pairing != state.asked;
            state.asked = status.pairing.clone();
            state.status = status.clone();
            (state.open, asking)
        };
        if open {
            self.panel.show(Some(&status));
        } else if asking {
            self.open_panel(); // to allow or deny it there
        }
    }

    /// The stage takes its screen's size once shown, and again when that changes.
    fn resized(self: &Rc<Self>, screen: Size) {
        if self.state.borrow().screen == screen {
            return;
        }
        self.state.borrow_mut().screen = screen;
        let (width, height) = Panel::size(screen.height);
        self.panel.widget().set_size_request(width, height);
        self.close_panel();
        self.place_dot_window(layout::place_dot(screen, self.saved.load(), dot::BOX, EDGE, 0));
    }

    fn place_dot_window(&self, rect: Rect) {
        let screen = {
            let mut state = self.state.borrow_mut();
            state.at = rect;
            state.screen
        };
        if !self.dot_window.is_realized() {
            WidgetExt::realize(&self.dot_window);
        }
        let margins = [rect.x, rect.y, screen.width - rect.x - rect.width, screen.height - rect.y - rect.height];
        overlay::place_dot(&self.dot_window, margins);
        self.dot_window.set_visible(true);
    }

    fn take_input(&self) {
        let (open, screen) = {
            let state = self.state.borrow();
            (state.open, state.screen)
        };
        let parts: &[Rect] = if open { &[Rect { x: 0, y: 0, width: screen.width, height: screen.height }] } else { &[] };
        overlay::set_input(&self.stage, parts, open);
    }

    fn toggle_panel(self: &Rc<Self>) {
        if self.state.borrow().open { self.close_panel() } else { self.open_panel() }
    }

    fn open_panel(self: &Rc<Self>) {
        let now = glib::monotonic_time();
        let status = {
            let mut state = self.state.borrow_mut();
            let (width, height) = Panel::size(state.screen.height);
            state.card = layout::place_panel(state.screen, state.at, Size { width, height }, GAP, MARGIN);
            state.grow.head(1.0, now, MEDIUM, Ease::POP);
            state.fade.head(1.0, now, FAST, Ease::EASE);
            state.open = true;
            state.status.clone()
        };
        self.panel.widget().set_visible(true);
        self.animate_card();
        self.take_input();
        self.panel.show(Some(&status));
        self.panel.focus();
    }

    fn close_panel(self: &Rc<Self>) {
        let now = glib::monotonic_time();
        {
            let mut state = self.state.borrow_mut();
            if !state.open {
                return;
            }
            state.open = false;
            state.grow.head(0.0, now, FAST, Ease::IN);
            state.fade.head(0.0, now, FAST, Ease::IN);
        }
        self.animate_card();
        self.take_input();
    }

    /// Grows the card out of the dot's corner, or shrinks it back in, a frame at a time.
    fn animate_card(self: &Rc<Self>) {
        if !self.card_frame() || std::mem::replace(&mut self.state.borrow_mut().card_ticking, true) {
            return;
        }
        let me = Rc::downgrade(self);
        self.panel.widget().add_tick_callback(move |_, _| {
            let Some(d) = me.upgrade() else { return glib::ControlFlow::Break };
            if d.card_frame() {
                return glib::ControlFlow::Continue;
            }
            d.state.borrow_mut().card_ticking = false;
            glib::ControlFlow::Break
        });
    }

    /// Sets the card where it's got to; whether it's still on its way.
    fn card_frame(&self) -> bool {
        let now = glib::monotonic_time();
        let state = self.state.borrow();
        let place = state.card;
        let scale = (SHRUNK + (1.0 - SHRUNK) * state.grow.value(now)) as f32;
        let origin = graphene::Point::new(
            if place.from_right { place.rect.width as f32 } else { 0.0 },
            if place.up { place.rect.height as f32 } else { 0.0 },
        );
        let transform = gsk::Transform::new()
            .translate(&graphene::Point::new(place.rect.x as f32 + origin.x(), place.rect.y as f32 + origin.y()))
            .scale(scale, scale)
            .translate(&graphene::Point::new(-origin.x(), -origin.y()));
        self.fixed.set_child_transform(self.panel.widget(), Some(&transform));
        self.panel.widget().set_opacity(state.fade.value(now));
        let moving = !state.grow.done(now) || !state.fade.done(now);        if !moving && !state.open {
            self.panel.widget().set_visible(false);
        }
        moving
    }

    fn dragged_to(&self, dx: f64, dy: f64) -> Rect {
        let state = self.state.borrow();
        let from = state.drag_from.unwrap_or(state.at);
        let moved = Rect { x: from.x + dx.round() as i32, y: from.y + dy.round() as i32, ..from };
        layout::keep_on(state.screen, moved, 0)
    }

    fn dragged_by(self: &Rc<Self>, dx: f64, dy: f64, first: bool) {
        if first {
            {
                let mut state = self.state.borrow_mut();
                state.drag_from = Some(state.at);
                if let Some(settling) = state.settling.take() {
                    settling.remove();
                }
            }
            self.close_panel();
            self.dot.set_away(true);
        }
        let rect = self.dragged_to(dx, dy);
        self.dragged.set_look(self.dot.look());
        self.fixed.move_(self.dragged.widget(), f64::from(rect.x), f64::from(rect.y));
        self.dragged.set_away(false);
    }

    fn dropped(self: &Rc<Self>, dx: f64, dy: f64) {
        let rect = self.dragged_to(dx, dy);
        self.place_dot_window(rect);
        self.saved.save(Point { x: rect.x, y: rect.y });
        self.dot.set_away(false);
        let settling = glib::timeout_add_local_once(SETTLE, glib::clone!(#[weak(rename_to = d)] self, move || {
            d.state.borrow_mut().settling = None;
            d.dragged.set_away(true);
        }));
        let mut state = self.state.borrow_mut();
        state.drag_from = None;
        state.settling = Some(settling);
    }
}

/// A see-through window without decorations, showing [child]; [size] if it's fixed.
fn overlay_window(app: &gtk::Application, child: &impl IsA<gtk::Widget>, size: Option<(i32, i32)>) -> gtk::Window {
    let window = gtk::ApplicationWindow::builder()
        .application(app)
        .title("Saambha")
        .decorated(false)
        .css_classes(["overlay"])
        .child(child)
        .build();
    if let Some((width, height)) = size {
        window.set_default_size(width, height);
        window.set_resizable(false);
    }
    window.upcast()
}
