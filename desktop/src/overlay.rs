//! The app's two windows float above the others. The dot is small, so its animations stay cheap,
//! and goes where it's put. The stage is see-through and covers the screen, below the dot: it shows
//! the chat panel, and the dot while it's dragged, and takes clicks only while the panel is open.
//! On Wayland desktops with a layer shell they're layers; elsewhere X11 windows (through XWayland
//! on GNOME), which X11 window managers keep on top when asked.

use gtk::cairo::{RectangleInt, Region};
use gtk::gdk::{Rectangle, WindowTypeHint};
use gtk::prelude::{GtkWindowExt, MonitorExt, ObjectExt, WidgetExt};

use crate::layer_shell::{self, Edge, Keyboard};

/// A part of the stage, in its (CSS) pixels.
#[derive(serde::Deserialize)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// Whether to run as an X11 app instead: GTK chose Wayland, but this desktop has no layer shell.
/// Asked before the app starts, so it can start over under X11.
pub fn needs_x11() -> bool {
    gtk::init().is_ok()
        && gtk::gdk::Display::default().is_some_and(|d| d.type_().name() == "GdkWaylandDisplay")
        && !layer_shell::is_supported()
}

const EDGES: [Edge; 4] = [Edge::Left, Edge::Top, Edge::Right, Edge::Bottom];

/// The dot's distances from the stage's edges, in the order of [EDGES].
pub type Margins = [i32; 4];

/// Makes [window] the stage. Before it's shown.
pub fn make_stage(window: &gtk::ApplicationWindow) {
    if layer_shell::is_supported() {
        layer_shell::init(window, "saambha", &EDGES);
    } else {
        keep_on_top(window);
        let area = free_area(window);
        window.move_(area.x(), area.y());
        window.resize(area.width(), area.height());
    }
    set_input(window, &[], false);
}

/// Makes [window] the dot. Before it's shown.
pub fn make_dot(window: &gtk::ApplicationWindow) {
    if layer_shell::is_supported() {
        // Held to every edge, so the margins size it. GTK draws a layer only once the compositor
        // changes its size, so it starts at the smallest.
        layer_shell::init(window, "saambha-dot", &EDGES);
        window.resize(1, 1);
    } else {
        keep_on_top(window);
    }
}

/// Puts the dot [margins] from the stage's edges.
pub fn place_dot(window: &gtk::ApplicationWindow, margins: Margins) {
    if layer_shell::is_supported() {
        for (edge, margin) in EDGES.into_iter().zip(margins) {
            layer_shell::set_margin(window, edge, margin);
        }
    } else {
        let area = free_area(window);
        window.move_(area.x() + margins[0], area.y() + margins[1]);
    }
}

/// Where the stage takes clicks (elsewhere they go to the windows below), and whether it takes
/// the keyboard too, from the window in use.
pub fn set_input(window: &gtk::ApplicationWindow, parts: &[Rect], keyboard: bool) {
    let rects: Vec<RectangleInt> = parts
        .iter()
        .map(|r| {
            let (x, y) = (r.x.floor() as i32, r.y.floor() as i32);
            RectangleInt::new(x, y, (r.x + r.width).ceil() as i32 - x, (r.y + r.height).ceil() as i32 - y)
        })
        .collect();
    window.input_shape_combine_region(Some(&Region::create_rectangles(&rects)));
    if layer_shell::is_supported() {
        layer_shell::set_keyboard(window, if keyboard { Keyboard::Exclusive } else { Keyboard::None });
    } else {
        window.set_accept_focus(keyboard);
        if keyboard {
            window.present();
        }
    }
}

fn keep_on_top(window: &gtk::ApplicationWindow) {
    window.set_type_hint(WindowTypeHint::Dock); // kept on top, out of the taskbar and the tiling
    window.set_keep_above(true);
    window.stick();
}

/// The primary monitor's area that panels and docks leave free.
fn free_area(window: &gtk::ApplicationWindow) -> Rectangle {
    let display = window.display();
    display
        .primary_monitor()
        .or_else(|| display.monitor(0))
        .expect("a monitor")
        .workarea()
}
