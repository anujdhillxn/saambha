//! The colours of the dot, the chat panel and the log window, in one place: the dot is drawn with
//! them, and the stylesheet (style.css) gets them as @define-color.

use crate::status::State;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rgba {
    pub red: f64,
    pub green: f64,
    pub blue: f64,
    pub alpha: f64,
}

impl Rgba {
    pub const fn hex(rgb: u32) -> Self {
        Self::hex_alpha(rgb, 1.0)
    }

    pub const fn hex_alpha(rgb: u32, alpha: f64) -> Self {
        Self {
            red: ((rgb >> 16) & 0xff) as f64 / 255.0,
            green: ((rgb >> 8) & 0xff) as f64 / 255.0,
            blue: (rgb & 0xff) as f64 / 255.0,
            alpha,
        }
    }

    /// This, with its alpha times [factor].
    pub fn fade(self, factor: f64) -> Self {
        Self { alpha: self.alpha * factor, ..self }
    }

    /// The colour [t] of the way from this to [to].
    pub fn mix(self, to: Self, t: f64) -> Self {
        let lerp = |a: f64, b: f64| a + (b - a) * t;
        Self {
            red: lerp(self.red, to.red),
            green: lerp(self.green, to.green),
            blue: lerp(self.blue, to.blue),
            alpha: lerp(self.alpha, to.alpha),
        }
    }

    fn css(self) -> String {
        let byte = |c: f64| (c * 255.0).round() as u8;
        format!("rgba({}, {}, {}, {})", byte(self.red), byte(self.green), byte(self.blue), self.alpha)
    }
}

pub const READY: Rgba = Rgba::hex(0x4dd180);
pub const COLD: Rgba = Rgba::hex(0x8a8a90);
pub const LISTENING: Rgba = Rgba::hex(0xf5554f);
pub const WORKING: Rgba = Rgba::hex(0xffb81a);
pub const ASKING: Rgba = Rgba::hex(0x4da8fa);
pub const STOP: Rgba = Rgba::hex(0xf5554f);

pub const SURFACE: Rgba = Rgba::hex_alpha(0x18181b, 0.97);
pub const SURFACE_DEEP: Rgba = Rgba::hex(0x141416);
pub const DISC: Rgba = Rgba::hex(0x161618);
pub const DISC_LIGHT: Rgba = Rgba::hex(0x2a2a2e);
pub const CONTROL: Rgba = Rgba::hex_alpha(0xffffff, 0.07);
pub const CONTROL_HOVER: Rgba = Rgba::hex_alpha(0xffffff, 0.15);
pub const BORDER: Rgba = Rgba::hex_alpha(0xffffff, 0.08);
pub const TEXT: Rgba = Rgba::hex(0xececec);
pub const TEXT_MUTED: Rgba = Rgba::hex_alpha(0xececec, 0.6);
pub const TEXT_FAINT: Rgba = Rgba::hex_alpha(0xececec, 0.45);
pub const ACCENT: Rgba = Rgba::hex(0x3584e4);
pub const ACCENT_HOVER: Rgba = Rgba::hex(0x4a94ee);
pub const ON_ACCENT: Rgba = Rgba::hex(0xffffff);
pub const SHADOW: Rgba = Rgba::hex_alpha(0x000000, 0.5);

/// The colour of what Saambha is doing.
pub fn state(state: State) -> Rgba {
    match state {
        State::Ready => READY,
        State::Cold => COLD,
        State::Listening => LISTENING,
        State::Working => WORKING,
        State::Asking => ASKING,
    }
}

/// The stylesheet, with the colours defined ahead of it.
pub fn stylesheet() -> String {
    let named = [
        ("ready", READY),
        ("cold", COLD),
        ("listening", LISTENING),
        ("working", WORKING),
        ("asking", ASKING),
        ("stop", STOP),
        ("surface", SURFACE),
        ("surface_deep", SURFACE_DEEP),
        ("control", CONTROL),
        ("control_hover", CONTROL_HOVER),
        ("border", BORDER),
        ("text", TEXT),
        ("text_muted", TEXT_MUTED),
        ("text_faint", TEXT_FAINT),
        ("accent", ACCENT),
        ("accent_hover", ACCENT_HOVER),
        ("on_accent", ON_ACCENT),
        ("shadow", SHADOW),
    ];
    let mut css: String = named.iter().map(|(name, color)| format!("@define-color {name} {};\n", color.css())).collect();
    css.push_str(include_str!("style.css"));
    css
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colours_from_hex() {
        assert_eq!(Rgba::hex_alpha(0xff8000, 0.5), Rgba { red: 1.0, green: 128.0 / 255.0, blue: 0.0, alpha: 0.5 });
    }

    #[test]
    fn the_stylesheet_names_the_colours() {
        assert!(stylesheet().starts_with("@define-color ready rgba(77, 209, 128, 1);\n"));
    }

    #[test]
    fn halfway_between_two_colours() {
        assert_eq!(Rgba::hex(0x000000).mix(Rgba::hex_alpha(0xffffff, 0.0), 0.5), Rgba { red: 0.5, green: 0.5, blue: 0.5, alpha: 0.5 });
    }
}
