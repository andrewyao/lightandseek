//! Fingers on a touch screen. The first finger down on the left half of
//! the screen is a joystick centred where it landed; any other finger looks
//! around by dragging. Each finger keeps its role until it lifts.

use std::collections::BTreeMap;

use game::Vec2;

/// How far, in CSS pixels, the stick finger travels for full speed.
pub const STICK_PX: f64 = 60.0;
/// Under this share of full travel the stick reads as centred.
const DEAD_ZONE: f32 = 0.12;

#[derive(Copy, Clone, Debug, PartialEq)]
enum Finger {
    Stick { origin: [f64; 2], at: [f64; 2] },
    Look { at: [f64; 2] },
}

/// Every finger on the screen, by pointer id.
#[derive(Clone, Debug, Default)]
pub struct Touches(BTreeMap<i32, Finger>);

impl Touches {
    /// A finger at `(x, y)` on a screen `width` wide.
    pub fn down(&mut self, id: i32, x: f64, y: f64, width: f64) {
        let has_stick = self.0.values().any(|f| matches!(f, Finger::Stick { .. }));
        let finger = if x < width / 2.0 && !has_stick {
            Finger::Stick {
                origin: [x, y],
                at: [x, y],
            }
        } else {
            Finger::Look { at: [x, y] }
        };
        self.0.insert(id, finger);
    }

    /// Finger `id` moved to `(x, y)`. Returns how far a looking finger
    /// dragged since it last moved, in CSS pixels.
    pub fn moved(&mut self, id: i32, x: f64, y: f64) -> Option<(f64, f64)> {
        match self.0.get_mut(&id)? {
            Finger::Stick { at, .. } => {
                *at = [x, y];
                None
            }
            Finger::Look { at } => {
                let d = (x - at[0], y - at[1]);
                *at = [x, y];
                Some(d)
            }
        }
    }

    pub fn up(&mut self, id: i32) {
        self.0.remove(&id);
    }

    /// The stick as `game::Input::mv`: up the screen is forward and right is
    /// strafe-right, at most unit length. None with no stick finger down.
    pub fn stick(&self) -> Option<Vec2> {
        let (origin, at) = self.stick_finger()?;
        let dx = ((at[0] - origin[0]) / STICK_PX) as f32;
        let dy = ((at[1] - origin[1]) / STICK_PX) as f32;
        let mv = Vec2::new(-dy, dx);
        let len = mv.length();
        Some(if len < DEAD_ZONE {
            Vec2::ZERO
        } else if len > 1.0 {
            mv * (1.0 / len)
        } else {
            mv
        })
    }

    /// Where to draw the stick: its centre and its knob, the knob kept
    /// within full travel. In CSS pixels.
    pub fn knob(&self) -> Option<([f64; 2], [f64; 2])> {
        let (origin, at) = self.stick_finger()?;
        let (dx, dy) = (at[0] - origin[0], at[1] - origin[1]);
        let len = dx.hypot(dy);
        let k = if len > STICK_PX { STICK_PX / len } else { 1.0 };
        Some((origin, [origin[0] + dx * k, origin[1] + dy * k]))
    }

    fn stick_finger(&self) -> Option<([f64; 2], [f64; 2])> {
        self.0.values().find_map(|f| match *f {
            Finger::Stick { origin, at } => Some((origin, at)),
            Finger::Look { .. } => None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const W: f64 = 1000.0;

    #[test]
    fn a_left_thumb_pushed_up_walks_forward_and_a_long_push_is_full_speed() {
        let mut t = Touches::default();
        t.down(1, 200.0, 500.0, W);
        assert_eq!(t.stick(), Some(Vec2::ZERO), "centred where it landed");
        t.moved(1, 200.0, 470.0);
        let half = t.stick().unwrap();
        assert!((half.x - 0.5).abs() < 1e-5 && half.y == 0.0, "{half:?}");
        t.moved(1, 200.0, 100.0);
        assert_eq!(t.stick(), Some(Vec2::new(1.0, 0.0)), "clamped");
        let (origin, knob) = t.knob().unwrap();
        assert_eq!(origin, [200.0, 500.0]);
        assert!(
            (knob[1] - (500.0 - STICK_PX)).abs() < 1e-9,
            "knob stays on its ring"
        );
    }

    #[test]
    fn pushing_right_strafes_right_and_a_diagonal_is_no_faster() {
        let mut t = Touches::default();
        t.down(1, 200.0, 500.0, W);
        t.moved(1, 260.0, 500.0);
        assert_eq!(t.stick(), Some(Vec2::new(0.0, 1.0)));
        t.moved(1, 400.0, 300.0);
        assert!((t.stick().unwrap().length() - 1.0).abs() < 1e-5);
    }

    #[test]
    fn a_right_side_drag_looks_and_leaves_the_stick_alone() {
        let mut t = Touches::default();
        t.down(7, 800.0, 400.0, W);
        assert_eq!(t.stick(), None);
        assert_eq!(t.moved(7, 830.0, 390.0), Some((30.0, -10.0)));
        assert_eq!(
            t.moved(7, 835.0, 390.0),
            Some((5.0, 0.0)),
            "since last move"
        );
        assert_eq!(t.stick(), None);
    }

    #[test]
    fn lifting_the_stick_finger_stops_the_avatar() {
        let mut t = Touches::default();
        t.down(1, 200.0, 500.0, W);
        t.moved(1, 200.0, 400.0);
        t.up(1);
        assert_eq!(t.stick(), None);
        assert_eq!(t.moved(1, 0.0, 0.0), None, "a lifted finger is forgotten");
    }

    #[test]
    fn stick_and_look_fingers_work_at_once() {
        let mut t = Touches::default();
        t.down(1, 200.0, 500.0, W);
        t.down(2, 700.0, 500.0, W);
        t.moved(1, 200.0, 440.0);
        assert_eq!(t.moved(2, 720.0, 500.0), Some((20.0, 0.0)));
        assert_eq!(t.stick(), Some(Vec2::new(1.0, 0.0)));
        t.up(2);
        assert_eq!(
            t.stick(),
            Some(Vec2::new(1.0, 0.0)),
            "look lifting keeps the stick"
        );
    }

    #[test]
    fn a_second_left_finger_looks_instead_of_taking_the_stick() {
        let mut t = Touches::default();
        t.down(1, 200.0, 500.0, W);
        t.down(2, 100.0, 300.0, W);
        assert_eq!(t.moved(2, 110.0, 300.0), Some((10.0, 0.0)));
        assert_eq!(t.knob().unwrap().0, [200.0, 500.0]);
    }
}
