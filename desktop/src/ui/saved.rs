//! Where the dot was left, kept for next time.

use std::path::PathBuf;

use crate::ui::layout::Point;

pub struct SavedPlace {
    path: PathBuf,
}

impl SavedPlace {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    /// Where the dot was left, if it's been moved.
    pub fn load(&self) -> Option<Point> {
        serde_json::from_slice(&std::fs::read(&self.path).ok()?).ok()
    }

    /// Keeps [at]; if that fails, the dot starts in the corner next time.
    pub fn save(&self, at: Point) {
        let saved = self.path.parent().map_or(Ok(()), std::fs::create_dir_all).and_then(|()| {
            std::fs::write(&self.path, serde_json::to_vec(&at).expect("a point serializes"))
        });
        if let Err(error) = saved {
            eprintln!("Can't save where the dot is, in {}: {error}", self.path.display());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn place(test: &str) -> SavedPlace {
        let dir = std::env::temp_dir().join(format!("saambha-desktop-{}-{test}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        SavedPlace::new(dir.join("state").join("dot.json"))
    }

    #[test]
    fn nowhere_until_its_moved() {
        assert_eq!(place("unmoved").load(), None);
    }

    #[test]
    fn where_it_was_left() {
        let saved = place("moved");

        saved.save(Point { x: 500, y: 300 });

        assert_eq!(saved.load(), Some(Point { x: 500, y: 300 }));
    }

    #[test]
    fn nowhere_if_what_was_kept_cant_be_read() {
        let saved = place("garbled");
        saved.save(Point { x: 1, y: 2 });
        std::fs::write(&saved.path, "{").unwrap();

        assert_eq!(saved.load(), None);
    }
}
