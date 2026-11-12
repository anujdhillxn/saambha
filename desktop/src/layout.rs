//! Where the dot and the chat panel go on the stage: plain geometry in its pixels.

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Size {
    pub width: i32,
    pub height: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

/// Where the panel goes, and the corner it grows from: [up] when it's above the dot, [from_right]
/// when lined up with its right edge.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Panel {
    pub rect: Rect,
    pub up: bool,
    pub from_right: bool,
}

/// [rect] moved just enough to fit on [screen], [margin] from its edges.
pub fn keep_on(screen: Size, rect: Rect, margin: i32) -> Rect {
    Rect {
        x: rect.x.min(screen.width - rect.width - margin).max(margin),
        y: rect.y.min(screen.height - rect.height - margin).max(margin),
        ..rect
    }
}

/// The dot: where it was left ([saved], else in the bottom-right corner, [edge] from it), on screen.
pub fn place_dot(screen: Size, saved: Option<Point>, size: i32, edge: i32, margin: i32) -> Rect {
    let at = saved.unwrap_or(Point { x: screen.width - size - edge, y: screen.height - size - edge });
    keep_on(screen, Rect { x: at.x, y: at.y, width: size, height: size }, margin)
}

/// The panel, from the dot toward the middle of the screen, [gap] away: above it when the dot is in
/// the bottom half, and lined up with its right edge when in the right half.
pub fn place_panel(screen: Size, dot: Rect, size: Size, gap: i32, margin: i32) -> Panel {
    let up = 2 * dot.y + dot.height > screen.height;
    let from_right = 2 * dot.x + dot.width > screen.width;
    let rect = Rect {
        x: if from_right { dot.x + dot.width - size.width } else { dot.x },
        y: if up { dot.y - gap - size.height } else { dot.y + dot.height + gap },
        width: size.width,
        height: size.height,
    };
    Panel { rect: keep_on(screen, rect, margin), up, from_right }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCREEN: Size = Size { width: 1920, height: 1080 };
    const DOT: i32 = 40;
    const PANEL: Size = Size { width: 400, height: 740 };

    #[test]
    fn the_dot_starts_in_the_bottom_right_corner() {
        assert_eq!(place_dot(SCREEN, None, DOT, 24, 8), Rect { x: 1920 - 40 - 24, y: 1080 - 40 - 24, width: 40, height: 40 });
    }

    #[test]
    fn the_dot_goes_back_where_it_was_left() {
        assert_eq!(place_dot(SCREEN, Some(Point { x: 500, y: 300 }), DOT, 24, 8), Rect { x: 500, y: 300, width: 40, height: 40 });
    }

    #[test]
    fn a_dot_left_off_a_smaller_screen_comes_back_on() {
        let at = place_dot(Size { width: 1280, height: 720 }, Some(Point { x: 1800, y: -20 }), DOT, 24, 8);

        assert_eq!((at.x, at.y), (1280 - 40 - 8, 8));
    }

    #[test]
    fn a_rect_on_screen_stays_put() {
        let rect = Rect { x: 10, y: 10, width: 100, height: 100 };

        assert_eq!(keep_on(SCREEN, rect, 8), rect);
    }

    #[test]
    fn the_panel_opens_up_and_left_from_a_dot_in_the_bottom_right() {
        let dot = place_dot(SCREEN, None, DOT, 24, 8);

        let panel = place_panel(SCREEN, dot, PANEL, 12, 8);

        assert!(panel.up && panel.from_right);
        assert_eq!(panel.rect.x + panel.rect.width, dot.x + dot.width); // right edges line up
        assert_eq!(panel.rect.y + panel.rect.height, dot.y - 12); // just above the dot
    }

    #[test]
    fn the_panel_opens_down_and_right_from_a_dot_in_the_top_left() {
        let panel = place_panel(SCREEN, Rect { x: 20, y: 20, width: 40, height: 40 }, PANEL, 12, 8);

        assert_eq!(panel, Panel { rect: Rect { x: 20, y: 72, width: 400, height: 740 }, up: false, from_right: false });
    }

    #[test]
    fn the_panel_stays_on_screen_beside_a_dot_halfway_down() {
        let panel = place_panel(SCREEN, Rect { x: 30, y: 600, width: 40, height: 40 }, PANEL, 12, 8);

        assert_eq!(keep_on(SCREEN, panel.rect, 8), panel.rect);
    }
}
