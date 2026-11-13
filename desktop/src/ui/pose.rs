//! The mascot's poses, one for each thing Saambha does: silhouettes traced into SVG paths
//! (icons/states), filled in the state's colour.

use std::sync::LazyLock;

use gtk::cairo;

use crate::server::status::State;

/// The side of the square the SVGs are drawn on.
const GRID: f64 = 1024.0;

enum Step {
    Move(f64, f64),
    Line(f64, f64),
    Curve(f64, f64, f64, f64, f64, f64),
    Close,
}

pub struct Pose(Vec<Step>);

struct Poses {
    ready: Pose,
    cold: Pose,
    listening: Pose,
    working: Pose,
    asking: Pose,
    speaking: Pose,
}

static POSES: LazyLock<Poses> = LazyLock::new(|| Poses {
    ready: Pose::parse(include_str!("../../icons/states/ready.svg")),
    cold: Pose::parse(include_str!("../../icons/states/cold.svg")),
    listening: Pose::parse(include_str!("../../icons/states/listening.svg")),
    working: Pose::parse(include_str!("../../icons/states/working.svg")),
    asking: Pose::parse(include_str!("../../icons/states/asking.svg")),
    speaking: Pose::parse(include_str!("../../icons/states/speaking.svg")),
});

/// The pose for [state], or for speaking.
pub fn of(state: State, speaking: bool) -> &'static Pose {
    if speaking {
        return &POSES.speaking;
    }
    match state {
        State::Ready => &POSES.ready,
        State::Cold => &POSES.cold,
        State::Listening => &POSES.listening,
        State::Working => &POSES.working,
        State::Asking => &POSES.asking,
    }
}

impl Pose {
    /// The one path of [svg]: absolute M, L, C and Z commands, each with its letter.
    fn parse(svg: &str) -> Pose {
        let path = svg.split(" d=\"").nth(1).and_then(|rest| rest.split('"').next()).expect("a path");
        let letters: Vec<(usize, &str)> = path.match_indices(|c: char| c.is_ascii_alphabetic()).collect();
        let steps = letters.iter().enumerate().map(|(i, &(at, letter))| {
            let end = letters.get(i + 1).map_or(path.len(), |next| next.0);
            let numbers: Vec<f64> = path[at + 1..end].split_whitespace().map(|n| n.parse().expect("a number")).collect();
            match (letter, numbers.as_slice()) {
                ("M", &[x, y]) => Step::Move(x, y),
                ("L", &[x, y]) => Step::Line(x, y),
                ("C", &[x1, y1, x2, y2, x, y]) => Step::Curve(x1, y1, x2, y2, x, y),
                ("Z", []) => Step::Close,
                _ => panic!("path command {letter} with {} numbers", numbers.len()),
            }
        });
        Pose(steps.collect())
    }

    /// Fills the pose, [size] pixels across, centred on ([cx], [cy]), with the source set.
    pub fn draw(&self, cr: &cairo::Context, cx: f64, cy: f64, size: f64) {
        cr.save().ok();
        cr.translate(cx - size / 2.0, cy - size / 2.0);
        cr.scale(size / GRID, size / GRID);
        for step in &self.0 {
            match *step {
                Step::Move(x, y) => cr.move_to(x, y),
                Step::Line(x, y) => cr.line_to(x, y),
                Step::Curve(x1, y1, x2, y2, x, y) => cr.curve_to(x1, y1, x2, y2, x, y),
                Step::Close => cr.close_path(),
            }
        }
        cr.set_fill_rule(cairo::FillRule::EvenOdd);
        let _ = cr.fill();
        cr.restore().ok();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOOKS: [(State, bool); 6] = [
        (State::Ready, false),
        (State::Cold, false),
        (State::Listening, false),
        (State::Working, false),
        (State::Asking, false),
        (State::Working, true),
    ];

    /// How much of each pixel of a [side]-pixel square the pose covers, drawn [size] across in its middle.
    fn coverage(pose: &Pose, side: i32, size: f64) -> Vec<u8> {
        let mut surface = cairo::ImageSurface::create(cairo::Format::A8, side, side).unwrap();
        let stride = surface.stride() as usize;
        {
            let cr = cairo::Context::new(&surface).unwrap();
            pose.draw(&cr, f64::from(side) / 2.0, f64::from(side) / 2.0, size);
        }
        let data = surface.data().unwrap();
        data.chunks(stride).flat_map(|row| row[..side as usize].to_vec()).collect()
    }

    #[test]
    fn parses_a_path() {
        let pose = Pose::parse(r#"<svg><path fill-rule="evenodd" d="M0 0 L1024 0 C1024 512 1024 512 1024 1024 L0 1024Z M256 256 L768 256 L768 768 L256 768Z"/></svg>"#);

        let covered = coverage(&pose, 8, 8.0);

        assert_eq!(covered[0], 255, "the square is filled");
        assert_eq!(covered[4 * 8 + 4], 0, "the hole in its middle isn't");
    }

    #[test]
    fn every_look_has_its_own_pose() {
        let drawn: Vec<Vec<u8>> = LOOKS.iter().map(|&(state, speaking)| coverage(of(state, speaking), 64, 64.0)).collect();

        for (i, pose) in drawn.iter().enumerate() {
            assert!(pose.iter().filter(|&&c| c == 255).count() > 500, "{:?} draws a figure", LOOKS[i]);
            assert!(drawn[..i].iter().all(|other| other != pose), "{:?} is a pose of its own", LOOKS[i]);
        }
    }
}
