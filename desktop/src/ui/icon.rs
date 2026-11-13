//! The panel's icons: a few strokes on a 24-unit grid, drawn 16 pixels across in the text's colour.

use gtk::cairo;
use gtk::prelude::*;

const SIZE: i32 = 16;

/// Polylines, each a list of points.
pub type Strokes = &'static [&'static [(f64, f64)]];

pub const LOGS: Strokes = &[&[(5.0, 6.0), (19.0, 6.0)], &[(5.0, 12.0), (19.0, 12.0)], &[(5.0, 18.0), (14.0, 18.0)]];
pub const NEW: Strokes = &[&[(12.0, 5.0), (12.0, 19.0)], &[(5.0, 12.0), (19.0, 12.0)]];
pub const CLOSE: Strokes = &[&[(6.0, 12.0), (18.0, 12.0)]];
pub const SEND: Strokes = &[&[(12.0, 19.0), (12.0, 5.0)], &[(6.0, 11.0), (12.0, 5.0), (18.0, 11.0)]];

pub fn new(strokes: Strokes) -> gtk::DrawingArea {
    let area = gtk::DrawingArea::builder().content_width(SIZE).content_height(SIZE).can_target(false).build();
    area.set_draw_func(move |area, cr, _, _| {
        let unit = f64::from(SIZE) / 24.0;
        cr.scale(unit, unit);
        for stroke in strokes {
            for (i, &(x, y)) in stroke.iter().enumerate() {
                if i == 0 { cr.move_to(x, y) } else { cr.line_to(x, y) }
            }
        }
        let color = area.color();
        cr.set_source_rgba(color.red().into(), color.green().into(), color.blue().into(), color.alpha().into());
        cr.set_line_width(2.0);
        cr.set_line_cap(cairo::LineCap::Round);
        cr.set_line_join(cairo::LineJoin::Round);
        let _ = cr.stroke();
    });
    area
}
