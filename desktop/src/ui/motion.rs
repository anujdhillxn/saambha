//! Values that change over time, eased like CSS transitions. Times are in microseconds, as GLib's
//! monotonic clock gives them.

use std::time::Duration;

/// A CSS cubic-bezier() timing function.
#[derive(Clone, Copy, Debug)]
pub struct Ease(f64, f64, f64, f64);

impl Ease {
    pub const LINEAR: Ease = Ease(0.0, 0.0, 1.0, 1.0);
    pub const EASE: Ease = Ease(0.25, 0.1, 0.25, 1.0);
    pub const IN: Ease = Ease(0.42, 0.0, 1.0, 1.0);
    pub const OUT: Ease = Ease(0.0, 0.0, 0.58, 1.0);
    pub const IN_OUT: Ease = Ease(0.42, 0.0, 0.58, 1.0);
    /// Overshoots, then settles.
    pub const SPRING: Ease = Ease(0.3, 1.6, 0.5, 1.0);
    pub const POP: Ease = Ease(0.2, 1.2, 0.4, 1.0);

    /// How far along the eased value is, [x] of the way through the time.
    pub fn at(self, x: f64) -> f64 {
        let Ease(x1, y1, x2, y2) = self;
        let bezier = |p1: f64, p2: f64, t: f64| 3.0 * (1.0 - t) * (1.0 - t) * t * p1 + 3.0 * (1.0 - t) * t * t * p2 + t * t * t;
        let x = x.clamp(0.0, 1.0);
        // x grows with t (x1 and x2 are within 0..1), so halve the range until t gives x.
        let (mut low, mut high) = (0.0, 1.0);
        for _ in 0..30 {
            let t = (low + high) / 2.0;
            if bezier(x1, x2, t) < x { low = t } else { high = t }
        }
        bezier(y1, y2, (low + high) / 2.0)
    }
}

/// A value on its way from one number to another.
#[derive(Clone, Copy, Debug)]
pub struct Tween {
    from: f64,
    to: f64,
    start: i64,
    duration: i64,
    ease: Ease,
}

impl Tween {
    pub fn still(value: f64) -> Self {
        Self { from: value, to: value, start: 0, duration: 0, ease: Ease::LINEAR }
    }

    pub fn value(&self, now: i64) -> f64 {
        if self.done(now) {
            return self.to;
        }
        self.from + (self.to - self.from) * self.ease.at((now - self.start) as f64 / self.duration as f64)
    }

    pub fn done(&self, now: i64) -> bool {
        now >= self.start + self.duration
    }

    pub fn target(&self) -> f64 {
        self.to
    }

    /// Heads to [to] from wherever it is [now].
    pub fn head(&mut self, to: f64, now: i64, duration: Duration, ease: Ease) {
        if to == self.to {
            return;
        }
        *self = Self { from: self.value(now), to, start: now, duration: duration.as_micros() as i64, ease };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECOND: i64 = 1_000_000;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-6
    }

    #[test]
    fn eases_start_at_nothing_and_end_at_everything() {
        for ease in [Ease::LINEAR, Ease::EASE, Ease::IN, Ease::OUT, Ease::IN_OUT, Ease::SPRING, Ease::POP] {
            assert!(close(ease.at(0.0), 0.0) && close(ease.at(1.0), 1.0), "{ease:?}");
        }
    }

    #[test]
    fn linear_is_halfway_halfway_through() {
        assert!(close(Ease::LINEAR.at(0.5), 0.5));
    }

    #[test]
    fn a_spring_overshoots() {
        assert!((0..10).map(|i| Ease::SPRING.at(f64::from(i) / 10.0)).any(|y| y > 1.0));
    }

    #[test]
    fn a_tween_goes_from_where_it_is_to_where_its_headed() {
        let mut tween = Tween::still(1.0);

        tween.head(3.0, 0, Duration::from_secs(1), Ease::LINEAR);

        assert!(close(tween.value(0), 1.0) && close(tween.value(SECOND / 2), 2.0) && close(tween.value(SECOND), 3.0));
        assert!(!tween.done(SECOND / 2) && tween.done(SECOND));
    }

    #[test]
    fn heading_elsewhere_midway_starts_from_where_it_got_to() {
        let mut tween = Tween::still(0.0);
        tween.head(10.0, 0, Duration::from_secs(1), Ease::LINEAR);

        tween.head(0.0, SECOND / 2, Duration::from_secs(1), Ease::LINEAR);

        assert!(close(tween.value(SECOND / 2), 5.0));
        assert!(close(tween.value(SECOND), 2.5));
    }
}
