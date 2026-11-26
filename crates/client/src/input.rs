//! What the player wants each tick: held keys make `mv`, the mouse turns
//! `yaw`, and a fixed 50 ms clock says how many ticks are due.
//!
//! `mv.x` is forward and `mv.y` strafe-right, as `game::Input` reads them.

use std::f32::consts::{PI, TAU};

use game::Vec2;

pub const TICK_MS: f64 = 50.0;
/// Ticks owed past this are dropped. A tab that slept should not replay
/// seconds of held keys at once.
const MAX_CATCH_UP: u32 = 4;
/// Radians of turn per CSS pixel of mouse travel.
const TURN_PER_PX: f32 = 0.004;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Dir {
    Forward,
    Back,
    Left,
    Right,
}

impl Dir {
    /// From `KeyboardEvent.code`, which names the physical key, so WASD
    /// works on any layout.
    pub fn of_code(code: &str) -> Option<Dir> {
        Some(match code {
            "KeyW" | "ArrowUp" => Dir::Forward,
            "KeyS" | "ArrowDown" => Dir::Back,
            "KeyA" | "ArrowLeft" => Dir::Left,
            "KeyD" | "ArrowRight" => Dir::Right,
            _ => return None,
        })
    }
}

/// A test or debug hook steering in place of the keyboard and mouse.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Script {
    pub mv: Vec2,
    pub yaw: f32,
}

#[derive(Clone, Debug, Default)]
pub struct Controls {
    held: [bool; 4],
    yaw: f32,
    script: Option<Script>,
}

impl Controls {
    pub fn key(&mut self, dir: Dir, down: bool) {
        self.held[dir as usize] = down;
    }

    /// Lets go of every key, for when the window loses focus mid-press.
    pub fn release_keys(&mut self) {
        self.held = [false; 4];
    }

    /// Mouse travel to the right turns right.
    pub fn turn(&mut self, dx_px: f64) {
        self.set_yaw(self.yaw - dx_px as f32 * TURN_PER_PX);
    }

    pub fn set_yaw(&mut self, yaw: f32) {
        self.yaw = if (-PI..=PI).contains(&yaw) {
            yaw
        } else {
            (yaw + PI).rem_euclid(TAU) - PI
        };
    }

    pub fn script(&mut self, script: Option<Script>) {
        if let Some(s) = script {
            self.set_yaw(s.yaw);
        }
        self.script = script;
    }

    pub fn scripted(&self) -> bool {
        self.script.is_some()
    }

    /// This tick's `(mv, yaw)`.
    pub fn intent(&self) -> (Vec2, f32) {
        if let Some(s) = self.script {
            return (s.mv, s.yaw);
        }
        let axis = |pos: Dir, neg: Dir| {
            f32::from(u8::from(self.held[pos as usize]))
                - f32::from(u8::from(self.held[neg as usize]))
        };
        let mv = Vec2::new(axis(Dir::Forward, Dir::Back), axis(Dir::Right, Dir::Left));
        (mv, self.yaw)
    }
}

/// A fixed-step clock. Call `due` as often as you like; across calls it
/// returns one tick per 50 ms elapsed.
#[derive(Clone, Debug, Default)]
pub struct Ticker {
    next_ms: Option<f64>,
}

impl Ticker {
    pub fn due(&mut self, now_ms: f64) -> u32 {
        let next = *self.next_ms.get_or_insert(now_ms);
        if now_ms < next {
            return 0;
        }
        let owed = ((now_ms - next) / TICK_MS) as u32 + 1;
        let n = owed.min(MAX_CATCH_UP);
        self.next_ms = Some(if owed > MAX_CATCH_UP {
            now_ms + TICK_MS
        } else {
            next + f64::from(n) * TICK_MS
        });
        n
    }
}

#[cfg(test)]
mod tests {
    use game::geom::{facing, right};

    use super::*;

    fn held(dirs: &[Dir]) -> Vec2 {
        let mut c = Controls::default();
        for d in dirs {
            c.key(*d, true);
        }
        c.intent().0
    }

    #[test]
    fn wasd_and_arrows_build_forward_and_strafe_right() {
        assert_eq!(held(&[Dir::Forward]), Vec2::new(1.0, 0.0));
        assert_eq!(held(&[Dir::Back]), Vec2::new(-1.0, 0.0));
        assert_eq!(held(&[Dir::Right]), Vec2::new(0.0, 1.0));
        assert_eq!(held(&[Dir::Left, Dir::Forward]), Vec2::new(1.0, -1.0));
        assert_eq!(held(&[Dir::Forward, Dir::Back]), Vec2::ZERO);
        assert_eq!(Dir::of_code("KeyD"), Some(Dir::Right));
        assert_eq!(Dir::of_code("ArrowUp"), Some(Dir::Forward));
        assert_eq!(Dir::of_code("KeyQ"), None);
    }

    #[test]
    fn released_keys_stop_the_avatar() {
        let mut c = Controls::default();
        c.key(Dir::Forward, true);
        c.release_keys();
        assert_eq!(c.intent().0, Vec2::ZERO);
    }

    #[test]
    fn mouse_right_turns_toward_strafe_right() {
        for start in [0.0, 1.0, -2.5, 3.1] {
            let mut c = Controls::default();
            c.set_yaw(start);
            let before = c.intent().1;
            c.turn(40.0);
            let after = c.intent().1;
            assert!(
                facing(after).dot(right(before)) > 0.1,
                "from {start}: {before} -> {after}"
            );
            assert!((-PI..=PI).contains(&after));
        }
    }

    #[test]
    fn a_script_overrides_the_keyboard_until_released() {
        let mut c = Controls::default();
        c.key(Dir::Forward, true);
        let mv = Vec2::new(0.0, -1.0);
        c.script(Some(Script { mv, yaw: 1.0 }));
        assert_eq!(c.intent(), (mv, 1.0));
        c.script(None);
        assert_eq!(c.intent(), (Vec2::new(1.0, 0.0), 1.0), "keeps the yaw");
    }

    #[test]
    fn the_ticker_emits_one_tick_per_50_ms_however_often_it_is_asked() {
        let mut t = Ticker::default();
        let mut ticks = 0;
        let mut now = 1000.0;
        while now < 2000.0 {
            ticks += t.due(now);
            ticks += t.due(now);
            now += 16.7;
        }
        assert_eq!(ticks, 20);
    }

    #[test]
    fn the_ticker_drops_ticks_owed_after_a_long_sleep() {
        let mut t = Ticker::default();
        assert_eq!(t.due(0.0), 1);
        assert_eq!(t.due(5000.0), MAX_CATCH_UP);
        assert_eq!(t.due(5010.0), 0);
        assert_eq!(t.due(5050.0), 1);
    }
}
