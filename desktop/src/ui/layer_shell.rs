//! The few functions of gtk4-layer-shell this app uses (built by scripts/gtk4-layer-shell.sh, so no
//! headers are needed). A layer is a surface the compositor keeps in a band of its own and places
//! itself: on Wayland desktops that have a layer shell (COSMIC, KDE, wlroots), not on GNOME.
//! The library stands in for some of libwayland's functions, so it must load before libwayland:
//! linking it into the program, rather than leaving it to GTK, sees to that.

use std::ffi::{CString, c_char, c_int};

use gtk::ffi::GtkWindow;
use gtk::glib::ffi::gboolean;
use gtk::glib::translate::ToGlibPtr;
use gtk::prelude::ObjectExt;

const LAYER_TOP: c_int = 2; // above the windows, below full-screen ones

#[derive(Clone, Copy)]
#[repr(C)]
pub enum Edge {
    Left = 0,
    Right = 1,
    Top = 2,
    Bottom = 3,
}

#[derive(Clone, Copy)]
#[repr(C)]
pub enum Keyboard {
    None = 0,
    Exclusive = 1,
}

#[link(name = "gtk4-layer-shell")]
unsafe extern "C" {
    fn gtk_layer_is_supported() -> gboolean;
    fn gtk_layer_init_for_window(window: *mut GtkWindow);
    fn gtk_layer_set_namespace(window: *mut GtkWindow, name: *const c_char);
    fn gtk_layer_set_layer(window: *mut GtkWindow, layer: c_int);
    fn gtk_layer_set_anchor(window: *mut GtkWindow, edge: Edge, anchor: gboolean);
    fn gtk_layer_set_margin(window: *mut GtkWindow, edge: Edge, margin: c_int);
    fn gtk_layer_set_keyboard_mode(window: *mut GtkWindow, mode: Keyboard);
}

/// Whether the desktop has a layer shell (never under X11).
pub fn is_supported() -> bool {
    // The library asks only Wayland displays (asked of another, it complains).
    is_wayland() && unsafe { gtk_layer_is_supported() != 0 } // SAFETY: on the main thread, once GTK is initialized.
}

pub fn is_wayland() -> bool {
    gtk::gdk::Display::default().is_some_and(|d| d.type_().name() == "GdkWaylandDisplay")
}

/// Makes [window] a layer above the other windows, held to the [anchors] edges of its monitor's
/// free area (stretched between opposite ones). Before it's realized.
pub fn init(window: &gtk::Window, namespace: &str, anchors: &[Edge]) {
    let window = raw(window);
    let namespace = CString::new(namespace).expect("no NUL in the namespace");
    // SAFETY: a live GtkWindow, on the main thread; the library copies the namespace.
    unsafe {
        gtk_layer_init_for_window(window);
        gtk_layer_set_namespace(window, namespace.as_ptr());
        gtk_layer_set_layer(window, LAYER_TOP);
        for &edge in anchors {
            gtk_layer_set_anchor(window, edge, 1);
        }
    }
}

/// [window]'s distance from the [edge] it's held to.
pub fn set_margin(window: &gtk::Window, edge: Edge, margin: i32) {
    // SAFETY: a live layer window, on the main thread.
    unsafe { gtk_layer_set_margin(raw(window), edge, margin) }
}

/// Whether the layer takes the keyboard from the window in use.
pub fn set_keyboard(window: &gtk::Window, keyboard: Keyboard) {
    // SAFETY: a live layer window, on the main thread.
    unsafe { gtk_layer_set_keyboard_mode(raw(window), keyboard) }
}

fn raw(window: &gtk::Window) -> *mut GtkWindow {
    window.to_glib_none().0
}
