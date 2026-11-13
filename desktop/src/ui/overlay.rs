//! The app's two windows float above the others. The dot is small, so its animations stay cheap,
//! and goes where it's put. The stage is see-through and covers the screen, below the dot: it shows
//! the chat panel, and the dot while it's dragged, and takes clicks only while the panel is open.
//! On Wayland desktops with a layer shell they're layers; elsewhere X11 windows (through XWayland
//! on GNOME), which X11 window managers keep on top when asked.

use gtk::cairo::{RectangleInt, Region};
use gtk::prelude::*;

use crate::ui::layer_shell::{self, Edge, Keyboard};
use crate::ui::layout::Rect;
use crate::ui::x11;

/// Whether to run as an X11 app instead: GTK chose Wayland, but this desktop has no layer shell.
/// Asked before the app starts, so it can start over under X11.
pub fn needs_x11() -> bool {
    gtk::init().is_ok() && layer_shell::is_wayland() && !layer_shell::is_supported()
}

const EDGES: [Edge; 4] = [Edge::Left, Edge::Top, Edge::Right, Edge::Bottom];

/// The dot's distances from the stage's edges, in the order of [EDGES].
pub type Margins = [i32; 4];

/// Makes [window] the stage, taking no clicks yet. Before it's shown.
pub fn make_stage(window: &gtk::Window) {
    if layer_shell::is_supported() {
        layer_shell::init(window, "saambha", &EDGES);
    } else {
        let area = x11::free_area(window);
        window.set_default_size(area.width(), area.height());
        window.connect_realize(move |window| {
            x11::make_dock(window);
            x11::move_to(window, area.x(), area.y());
        });
        window.connect_map(x11::keep_above);
    }
    window.connect_realize(|window| set_input(window, &[], false));
}

/// Makes [window] the dot. Before it's shown.
pub fn make_dot(window: &gtk::Window) {
    if layer_shell::is_supported() {
        // Held to every edge, so the margins place and size it.
        layer_shell::init(window, "saambha-dot", &EDGES);
    } else {
        window.connect_realize(x11::make_dock);
        window.connect_map(x11::keep_above);
    }
}

/// Puts the dot [margins] from the stage's edges. Once it's realized.
pub fn place_dot(window: &gtk::Window, margins: Margins) {
    if layer_shell::is_supported() {
        for (edge, margin) in EDGES.into_iter().zip(margins) {
            layer_shell::set_margin(window, edge, margin);
        }
    } else {
        let area = x11::free_area(window);
        x11::move_to(window, area.x() + margins[0], area.y() + margins[1]);
    }
}

/// Where the stage takes clicks (elsewhere they go to the windows below), and whether it takes
/// the keyboard too, from the window in use. Once it's realized.
pub fn set_input(window: &gtk::Window, parts: &[Rect], keyboard: bool) {
    let rects: Vec<RectangleInt> = parts.iter().map(|r| RectangleInt::new(r.x, r.y, r.width, r.height)).collect();
    window.surface().expect("a realized window").set_input_region(Some(&Region::create_rectangles(&rects)));
    if layer_shell::is_supported() {
        layer_shell::set_keyboard(window, if keyboard { Keyboard::Exclusive } else { Keyboard::None });
    } else {
        x11::set_accept_focus(window, keyboard);
        if keyboard {
            x11::activate(window);
        } else {
            x11::give_back(window);
        }
    }
}
