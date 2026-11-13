//! What the app asks of X11 window managers for its windows, which GTK 4 no longer asks itself: to
//! keep them on top, out of the taskbar and the tiling, on every workspace, where they're put, and
//! whether to give them the keyboard. Xlib's functions come from the libX11 GTK has loaded.

use std::cell::Cell;
use std::ffi::{CStr, c_long};

use gdk4_x11::x11::xlib::{self, Xlib};
use gdk4_x11::{X11Display, X11Monitor, X11Surface};
use gtk::gdk;
use gtk::prelude::*;

/// The primary monitor's area that panels and docks leave free.
pub fn free_area(window: &gtk::Window) -> gdk::Rectangle {
    let display = WidgetExt::display(window).downcast::<X11Display>().expect("an X11 display");
    display.primary_monitor().downcast::<X11Monitor>().expect("an X11 monitor").workarea()
}

/// Makes [window] a dock: out of the taskbar and the tiling, on every workspace, and not given the
/// keyboard when shown. Once it's realized, before it's shown.
pub fn make_dock(window: &gtk::Window) {
    surface(window).set_user_time(0); // not to be focused when shown
    let (x, display, xid) = handles(window);
    let atom = |name: &CStr| {
        // SAFETY: a live display, and a NUL-terminated name.
        unsafe { (x.XInternAtom)(display, name.as_ptr(), xlib::False) }
    };
    let set = |property: &CStr, kind: xlib::Atom, values: &[c_long]| {
        // SAFETY: a live display and window; 32-bit X properties are passed as longs.
        unsafe {
            (x.XChangeProperty)(
                display, xid, atom(property), kind, 32, xlib::PropModeReplace,
                values.as_ptr().cast(), values.len() as i32,
            );
        }
    };
    set(c"_NET_WM_WINDOW_TYPE", xlib::XA_ATOM, &[atom(c"_NET_WM_WINDOW_TYPE_DOCK") as c_long]);
    set(c"_NET_WM_DESKTOP", xlib::XA_CARDINAL, &[0xFFFF_FFFF]); // all of them
}

/// Asks the window manager to keep [window] above the others. Once it's shown: GTK sets the
/// window's state as it shows it, so this is asked of the window manager, as of a shown window.
pub fn keep_above(window: &gtk::Window) {
    let above = intern(window, c"_NET_WM_STATE_ABOVE");
    ask(window, handles(window).2, c"_NET_WM_STATE", [1, above, 0, 1, 0]); // add it, asked by an app
}

thread_local! {
    /// The window that had the keyboard before one of the app's took it.
    static HAD_THE_KEYBOARD: Cell<Option<xlib::Window>> = const { Cell::new(None) };
}

/// Asks the window manager to give [window] the keyboard. As a pager would: asked by an app,
/// it would refuse a dock without the time of the click that led to it.
pub fn activate(window: &gtk::Window) {
    let (_, _, xid) = handles(window);
    let active = active_window(window);
    if active != Some(xid) {
        HAD_THE_KEYBOARD.set(active);
    }
    ask(window, xid, c"_NET_ACTIVE_WINDOW", [2, 0, 0, 0, 0]); // by a pager, now
}

/// Gives the keyboard back to the window that had it before [window] took it, if [window] still has it.
pub fn give_back(window: &gtk::Window) {
    let (_, _, xid) = handles(window);
    if let Some(previous) = HAD_THE_KEYBOARD.take().filter(|_| active_window(window) == Some(xid)) {
        ask(window, previous, c"_NET_ACTIVE_WINDOW", [2, 0, 0, 0, 0]);
    }
}

/// The window with the keyboard.
fn active_window(window: &gtk::Window) -> Option<xlib::Window> {
    let (x, display, _) = handles(window);
    let property = intern(window, c"_NET_ACTIVE_WINDOW") as xlib::Atom;
    let (mut kind, mut format, mut count, mut left) = (0, 0, 0, 0);
    let mut data: *mut u8 = std::ptr::null_mut();
    // SAFETY: a live display; Xlib allocates [data], freed once read.
    unsafe {
        let root = (x.XDefaultRootWindow)(display);
        let read = (x.XGetWindowProperty)(
            display, root, property, 0, 1, xlib::False, xlib::XA_WINDOW,
            &mut kind, &mut format, &mut count, &mut left, &mut data,
        );
        if read != 0 || data.is_null() {
            return None;
        }
        let active = (count == 1).then(|| *data.cast::<xlib::Window>()).filter(|&w| w != 0);
        (x.XFree)(data.cast());
        active
    }
}

/// Sends the window manager an EWMH request about the X window [xid].
fn ask(window: &gtk::Window, xid: xlib::Window, request: &CStr, data: [c_long; 5]) {
    let (x, display, _) = handles(window);
    let mut message = xlib::XClientMessageEvent {
        type_: xlib::ClientMessage,
        serial: 0,
        send_event: xlib::True,
        display,
        window: xid,
        message_type: intern(window, request) as xlib::Atom,
        format: 32,
        data: xlib::ClientMessageData::new(),
    };
    for (i, value) in data.into_iter().enumerate() {
        message.data.set_long(i, value);
    }
    let mut event = xlib::XEvent { client_message: message };
    // SAFETY: a live display, and a client message to its root window as EWMH has it.
    unsafe {
        let root = (x.XDefaultRootWindow)(display);
        (x.XSendEvent)(display, root, xlib::False, xlib::SubstructureNotifyMask | xlib::SubstructureRedirectMask, &mut event);
        (x.XFlush)(display);
    }
}

fn intern(window: &gtk::Window, name: &CStr) -> c_long {
    let (x, display, _) = handles(window);
    // SAFETY: a live display, and a NUL-terminated name.
    unsafe { (x.XInternAtom)(display, name.as_ptr(), xlib::False) as c_long }
}

/// Puts [window]'s top-left corner at ([left], [top]) on the screen.
pub fn move_to(window: &gtk::Window, left: i32, top: i32) {
    let (x, display, xid) = handles(window);
    // SAFETY: a live display and window.
    unsafe {
        (x.XMoveWindow)(display, xid, left, top);
        (x.XFlush)(display);
    }
}

/// Whether the window manager gives [window] the keyboard.
pub fn set_accept_focus(window: &gtk::Window, accept: bool) {
    let (x, display, xid) = handles(window);
    // SAFETY: a live display and window; the hints Xlib allocates are freed once set.
    unsafe {
        let mut hints = (x.XGetWMHints)(display, xid);
        if hints.is_null() {
            hints = (x.XAllocWMHints)();
        }
        (*hints).flags |= xlib::InputHint;
        (*hints).input = if accept { xlib::True } else { xlib::False };
        (x.XSetWMHints)(display, xid, hints);
        (x.XFree)(hints.cast());
    }
}

fn surface(window: &gtk::Window) -> X11Surface {
    window.surface().expect("a realized window").downcast::<X11Surface>().expect("an X11 window")
}

fn handles(window: &gtk::Window) -> (Xlib, *mut xlib::Display, xlib::Window) {
    let surface = surface(window);
    let display = surface.display().downcast::<X11Display>().expect("an X11 display");
    let x = Xlib::open().expect("libX11, which GTK uses");
    // SAFETY: the display outlives the app's windows.
    (x, unsafe { display.xdisplay() }, surface.xid())
}
