//! The dot: a disc with the mascot posed as what Saambha is doing, drawn with Cairo, in its own small
//! window and, while it's dragged, on the stage. It draws frames only while something about it moves
//! (a ring, a transition), so a dot left alone costs nothing.

use std::cell::RefCell;
use std::f64::consts::{FRAC_PI_2, FRAC_PI_4, PI, TAU};
use std::rc::Rc;
use std::time::Duration;

use gtk::prelude::*;
use gtk::{cairo, glib};

use crate::server::status::State;
use crate::ui::motion::{Ease, Tween};
use crate::ui::pose;
use crate::ui::theme::{self, Rgba};

/// The dot's box: the disc in its middle, the rest for its glow and rings.
pub const BOX: i32 = 72;
const RADIUS: f64 = 20.0;
const POSE: f64 = 32.0; // the mascot's square, inside the disc

const FAST: Duration = Duration::from_millis(150);
const MEDIUM: Duration = Duration::from_millis(250);
const SLOW: Duration = Duration::from_millis(500);

const HOVERED: f64 = 1.14; // its size, hovered
const PRESSED: f64 = 0.92;

/// What the dot shows.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Look {
    pub state: State,
    pub speaking: bool,
}

#[derive(Clone)]
pub struct DotView {
    area: gtk::DrawingArea,
    dot: Rc<RefCell<Dot>>,
}

impl DotView {
    /// A dot showing [look]; [away] if it starts faded away.
    pub fn new(look: Look, away: bool) -> Self {
        let area = gtk::DrawingArea::builder().content_width(BOX).content_height(BOX).build();
        let dot = Rc::new(RefCell::new(Dot::new(look, away)));
        area.set_draw_func({
            let dot = dot.clone();
            move |_, cr, width, height| dot.borrow().draw(cr, f64::from(width) / 2.0, f64::from(height) / 2.0, clock())
        });
        Self { area, dot }
    }

    pub fn widget(&self) -> &gtk::DrawingArea {
        &self.area
    }

    pub fn look(&self) -> Look {
        self.dot.borrow().look
    }

    pub fn set_look(&self, look: Look) {
        self.change(|dot, now| {
            if look.state != dot.look.state {
                dot.since = now;
            }
            dot.look = look;
        });
    }

    pub fn hovered(&self) -> bool {
        self.dot.borrow().hovered
    }

    /// Whether the pointer is over the disc.
    pub fn set_hovered(&self, hovered: bool) {
        self.change(|dot, _| dot.hovered = hovered);
    }

    /// Whether the disc is pressed.
    pub fn set_pressed(&self, pressed: bool) {
        self.change(|dot, _| dot.pressed = pressed);
    }

    /// Whether the dot fades away (while it's shown elsewhere).
    pub fn set_away(&self, away: bool) {
        self.change(|dot, now| dot.opacity.head(if away { 0.0 } else { 1.0 }, now, FAST, Ease::EASE));
    }

    /// Whether ([x], [y]) is on the disc.
    pub fn on_disc(&self, x: f64, y: f64) -> bool {
        let (cx, cy) = (f64::from(self.area.width()) / 2.0, f64::from(self.area.height()) / 2.0);
        (x - cx).hypot(y - cy) <= RADIUS * self.dot.borrow().scale.value(clock())
    }

    fn change(&self, change: impl FnOnce(&mut Dot, i64)) {
        let now = clock();
        {
            let mut dot = self.dot.borrow_mut();
            change(&mut dot, now);
            dot.retarget(now);
        }
        self.area.queue_draw();
        self.animate(now);
    }

    /// Draws a frame each time the display does, while something moves.
    fn animate(&self, now: i64) {
        let mut dot = self.dot.borrow_mut();
        if dot.ticking.is_some() || !dot.moving(now) {
            return;
        }
        let weak = Rc::downgrade(&self.dot);
        dot.ticking = Some(self.area.add_tick_callback(move |area, _| {
            let Some(dot) = weak.upgrade() else { return glib::ControlFlow::Break };
            area.queue_draw();
            let mut dot = dot.borrow_mut();
            if dot.moving(clock()) {
                glib::ControlFlow::Continue
            } else {
                dot.ticking = None;
                glib::ControlFlow::Break
            }
        }));
    }
}

fn clock() -> i64 {
    glib::monotonic_time()
}

struct Dot {
    look: Look,
    since: i64, // when the state began, for its looping animation
    hovered: bool,
    pressed: bool,
    color_from: Rgba,
    color_to: Rgba,
    color: Tween, // from color_from (0) to color_to (1)
    hover: Tween, // 1 hovered, for the glow and the ring
    scale: Tween,
    opacity: Tween,
    ticking: Option<gtk::TickCallbackId>,
}

impl Dot {
    fn new(look: Look, away: bool) -> Self {
        let color = theme::state(look.state);
        Dot {
            look,
            since: clock(),
            hovered: false,
            pressed: false,
            color_from: color,
            color_to: color,
            color: Tween::still(1.0),
            hover: Tween::still(0.0),
            scale: Tween::still(1.0),
            opacity: Tween::still(if away { 0.0 } else { 1.0 }),
            ticking: None,
        }
    }

    /// Speaking, it's a stop button while hovered.
    fn stopping(&self) -> bool {
        self.look.speaking && self.hovered
    }

    /// Heads to what the look, the pointer and the button call for.
    fn retarget(&mut self, now: i64) {
        let color = if self.stopping() { theme::STOP } else { theme::state(self.look.state) };
        if color != self.color_to {
            self.color_from = self.current_color(now);
            self.color_to = color;
            self.color = Tween::still(0.0);
            self.color.head(1.0, now, SLOW, Ease::EASE);
        }
        self.hover.head(if self.hovered { 1.0 } else { 0.0 }, now, MEDIUM, Ease::EASE);
        let scale = if self.pressed { PRESSED } else if self.hovered { HOVERED } else { 1.0 };
        self.scale.head(scale, now, MEDIUM, Ease::SPRING);
    }

    fn current_color(&self, now: i64) -> Rgba {
        self.color_from.mix(self.color_to, self.color.value(now))
    }

    fn looping(&self) -> bool {
        match self.look.state {
            State::Listening | State::Asking => true,
            State::Working => !self.stopping(),
            State::Ready | State::Cold => false,
        }
    }

    fn moving(&self, now: i64) -> bool {
        (self.looping() && self.opacity.target() > 0.0)
            || [self.color, self.hover, self.scale, self.opacity].iter().any(|t| !t.done(now))
    }

    fn draw(&self, cr: &cairo::Context, cx: f64, cy: f64, now: i64) {
        let opacity = self.opacity.value(now);
        if opacity <= 0.0 {
            return;
        }
        let color = self.current_color(now);
        let hover = self.hover.value(now);
        let scale = self.scale.value(now);
        let r = RADIUS * scale;
        let elapsed = (now - self.since) as f64 / 1e6;

        cr.push_group();
        soft_disc(cr, cx, cy + 3.0 + hover, r, 10.0 + 4.0 * hover, theme::SHADOW);
        if hover > 0.0 {
            soft_disc(cr, cx, cy, r, 16.0, color.fade(0.45 * hover));
        }

        // The disc, lit from its top left, with a thin rim in the state's colour.
        let light = cairo::RadialGradient::new(cx - 0.3 * r, cy - 0.4 * r, 0.0, cx - 0.3 * r, cy - 0.4 * r, 1.91 * r);
        add_stop(&light, 0.0, theme::DISC_LIGHT);
        add_stop(&light, 0.7, theme::DISC);
        cr.arc(cx, cy, r, 0.0, TAU);
        let _ = cr.set_source(&light);
        let _ = cr.fill();
        set_color(cr, color.fade(0.35 + 0.35 * hover));
        cr.set_line_width(1.0);
        cr.arc(cx, cy, r - 0.5, 0.0, TAU);
        let _ = cr.stroke();

        set_color(cr, color);
        if self.stopping() {
            let side = 14.0 * scale;
            rounded_rect(cr, cx - side / 2.0, cy - side / 2.0, side, side, 3.0 * scale);
            let _ = cr.fill();
        } else {
            pose::of(self.look.state, self.look.speaking).draw(cr, cx, cy, POSE * scale);
        }

        match self.look.state {
            State::Listening => ripples(cr, cx, cy, scale, color, elapsed),
            State::Working if !self.stopping() => spinner(cr, cx, cy, scale, color, elapsed),
            State::Asking => pulse(cr, cx, cy, scale, color, elapsed),
            _ => {}
        }
        let _ = cr.pop_group_to_source();
        let _ = cr.paint_with_alpha(opacity);
    }
}

fn set_color(cr: &cairo::Context, color: Rgba) {
    cr.set_source_rgba(color.red, color.green, color.blue, color.alpha);
}

fn add_stop(gradient: &cairo::Gradient, offset: f64, color: Rgba) {
    gradient.add_color_stop_rgba(offset, color.red, color.green, color.blue, color.alpha);
}

/// A disc of radius [r] blurred over [blur] pixels, like a CSS box-shadow.
fn soft_disc(cr: &cairo::Context, cx: f64, cy: f64, r: f64, blur: f64, color: Rgba) {
    let outer = r + blur / 2.0;
    let gradient = cairo::RadialGradient::new(cx, cy, 0.0, cx, cy, outer);
    add_stop(&gradient, (r - blur / 2.0).max(0.0) / outer, color);
    add_stop(&gradient, r / outer, color.fade(0.5));
    add_stop(&gradient, 1.0, color.fade(0.0));
    cr.arc(cx, cy, outer, 0.0, TAU);
    let _ = cr.set_source(&gradient);
    let _ = cr.fill();
}

fn rounded_rect(cr: &cairo::Context, x: f64, y: f64, width: f64, height: f64, radius: f64) {
    let r = radius.min(width / 2.0).min(height / 2.0);
    cr.new_sub_path();
    cr.arc(x + width - r, y + r, r, -FRAC_PI_2, 0.0);
    cr.arc(x + width - r, y + height - r, r, 0.0, FRAC_PI_2);
    cr.arc(x + r, y + height - r, r, FRAC_PI_2, PI);
    cr.arc(x + r, y + r, r, PI, 3.0 * FRAC_PI_2);
    cr.close_path();
}

/// Listening: rings rippling out of the disc, one after the other.
fn ripples(cr: &cairo::Context, cx: f64, cy: f64, scale: f64, color: Rgba, elapsed: f64) {
    const PERIOD: f64 = 1.4;
    for delay in [0.0, PERIOD / 2.0] {
        if elapsed < delay {
            continue;
        }
        let out = Ease::OUT.at(((elapsed - delay) / PERIOD).fract());
        let grown = scale * (1.0 + 0.75 * out);
        set_color(cr, color.fade(0.7 * (1.0 - out)));
        cr.set_line_width(2.0 * grown);
        cr.arc(cx, cy, (RADIUS - 1.0) * grown, 0.0, TAU);
        let _ = cr.stroke();
    }
}

/// Working: an arc going round the disc.
fn spinner(cr: &cairo::Context, cx: f64, cy: f64, scale: f64, color: Rgba, elapsed: f64) {
    const PERIOD: f64 = 1.1;
    let turn = TAU * (elapsed / PERIOD).fract();
    let r = 23.75 * scale;
    cr.set_line_width(2.5 * scale);
    set_color(cr, color);
    cr.arc(cx, cy, r, turn - 3.0 * FRAC_PI_4, turn - FRAC_PI_4);
    let _ = cr.stroke();
    set_color(cr, color.fade(0.4));
    cr.arc(cx, cy, r, turn - FRAC_PI_4, turn + FRAC_PI_4);
    let _ = cr.stroke();
}

/// Asking: a ring pulsing around the disc.
fn pulse(cr: &cairo::Context, cx: f64, cy: f64, scale: f64, color: Rgba, elapsed: f64) {
    const PERIOD: f64 = 0.9; // each way
    let half = elapsed / PERIOD;
    let way = if (half as u64).is_multiple_of(2) { half.fract() } else { 1.0 - half.fract() };
    let swell = Ease::IN_OUT.at(way);
    set_color(cr, color.fade(0.4 + 0.5 * swell));
    cr.set_line_width(2.0 * scale);
    cr.arc(cx, cy, 23.0 * scale * (1.0 + 0.12 * swell), 0.0, TAU);
    let _ = cr.stroke();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_pose_fits_on_the_disc_within_its_rim() {
        const ZOOM: f64 = 4.0;
        let side = BOX * ZOOM as i32;
        let centre = f64::from(side) / 2.0;
        let looks = [State::Ready, State::Cold, State::Listening, State::Working, State::Asking].map(|state| (state, false));
        for (state, speaking) in looks.into_iter().chain([(State::Working, true)]) {
            let mut surface = cairo::ImageSurface::create(cairo::Format::A8, side, side).unwrap();
            let stride = surface.stride() as usize;
            {
                let cr = cairo::Context::new(&surface).unwrap();
                pose::of(state, speaking).draw(&cr, centre, centre, POSE * ZOOM);
            }

            for (i, &covered) in surface.data().unwrap().iter().enumerate() {
                let (x, y) = ((i % stride) as f64 + 0.5 - centre, (i / stride) as f64 + 0.5 - centre);
                assert!(covered == 0 || x.hypot(y) < (RADIUS - 1.0) * ZOOM, "{state:?} (speaking: {speaking}) at {x}, {y}");
            }
        }
    }
}
