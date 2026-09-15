//! Browser host for Brunhilda. You are the human; the arrow keys move you.
//!
//! Two views of the same log. The *live* view draws the sim's truth,
//! including where you really are. The *scrubbed* view at index `n` draws
//! only what the log knows: her dead-reckoned position, the cells she has
//! cleaned, and where she last saw you. That gap is the point: the log is
//! her record of the world, and the replay shows what she was acting on
//! when she attacked.
//!
//! Numbers cross the boundary as flat integer arrays; no strings at all.

use brunhilda::sim::{Frame, Sim};
use brunhilda::{Brain, Cell, Cmd, Dir, Ev, H, Mode, Policy, Room, W, attacking, brain};
use logfold_core::{Checkpoints, Fold, Log};
use wasm_bindgen::prelude::*;

const CHECKPOINT_EVERY: usize = 16;

fn dir_of(code: i32) -> Option<Dir> {
    match code {
        0 => Some(Dir::N),
        1 => Some(Dir::E),
        2 => Some(Dir::S),
        3 => Some(Dir::W),
        _ => None,
    }
}

fn dir_code(d: Option<Dir>) -> i32 {
    match d {
        Some(Dir::N) => 0,
        Some(Dir::E) => 1,
        Some(Dir::S) => 2,
        Some(Dir::W) => 3,
        None => -1,
    }
}

fn mode_code(m: Mode) -> i32 {
    match m {
        Mode::Idle => 0,
        Mode::Cleaning => 1,
        Mode::Docking => 2,
        Mode::Stopped { .. } => 3,
    }
}

#[wasm_bindgen]
pub struct VacuumApp {
    log: Log<Ev>,
    brain: Fold<'static, Ev, Brain>,
    checkpoints: Checkpoints<Brain>,
    sim: Sim,
}

#[wasm_bindgen]
impl VacuumApp {
    /// `careful` picks the policy; `hx, hy` is where you start.
    #[wasm_bindgen(constructor)]
    pub fn new(careful: bool, hx: i32, hy: i32) -> Self {
        let room = Room::default();
        let policy = if careful {
            Policy::Careful
        } else {
            Policy::Naive
        };
        let human = Cell::new(hx, hy);
        let human = if room.free(human) && human != room.dock {
            human
        } else {
            Cell::new(6, 6)
        };
        Self {
            log: Log::new(),
            brain: brain(room.clone(), policy),
            checkpoints: Checkpoints::new(),
            sim: Sim::new(room, human),
        }
    }

    pub fn width(&self) -> i32 {
        W
    }
    pub fn height(&self) -> i32 {
        H
    }

    /// Furniture cells as `[x0, y0, x1, y1, …]`.
    pub fn furniture(&self) -> Vec<i32> {
        self.sim
            .room
            .furniture
            .iter()
            .flat_map(|c| [c.x, c.y])
            .collect()
    }

    pub fn dock(&self) -> Vec<i32> {
        vec![self.sim.room.dock.x, self.sim.room.dock.y]
    }

    pub fn free_count(&self) -> i32 {
        self.sim.room.free_cells().count() as i32
    }

    // ---- inputs: each one appends events ----

    pub fn start(&mut self) {
        self.sim.command(&mut self.log, &self.brain, Cmd::Start);
        self.checkpoint();
    }
    pub fn dock_cmd(&mut self) {
        self.sim.command(&mut self.log, &self.brain, Cmd::Dock);
        self.checkpoint();
    }
    pub fn estop(&mut self) {
        self.sim.command(&mut self.log, &self.brain, Cmd::EStop);
        self.checkpoint();
    }

    /// One frame. `human_dir` is 0..3 for N/E/S/W or -1 to stand still.
    /// Returns true if she ended the frame in your cell.
    pub fn frame(&mut self, human_dir: i32, dropout: bool, dt_ms: f64) -> bool {
        let f = Frame {
            human: dir_of(human_dir),
            dropout,
            dt: dt_ms as u64,
        };
        let attacked = self.sim.frame(&mut self.log, &self.brain, f);
        self.checkpoint();
        attacked
    }

    // ---- live truth, from the sim ----

    /// `[human_x, human_y, attacks, latched, bumps]`.
    pub fn truth(&self) -> Vec<i32> {
        let s = &self.sim;
        vec![
            s.human.x,
            s.human.y,
            s.attacks as i32,
            s.latched as i32,
            s.bumps as i32,
        ]
    }

    // ---- the log ----

    pub fn len(&self) -> u32 {
        self.log.len() as u32
    }
    pub fn is_empty(&self) -> bool {
        self.log.is_empty()
    }
    pub fn checkpoint_for(&self, n: u32) -> i32 {
        self.checkpoints
            .nearest(n as usize)
            .map_or(-1, |(u, _)| u as i32)
    }

    /// What she knew after the first `n` events:
    /// `[x, y, heading, mode, seen_x, seen_y, seen_age, cleaned, attacking, ticks]`
    /// with `seen_*` = -1 when she has never seen you.
    pub fn snapshot(&self, n: u32) -> Vec<i32> {
        let b = self.brain_at(n);
        let (sx, sy, sa) = b
            .sighting
            .map_or((-1, -1, -1), |s| (s.at.x, s.at.y, s.age as i32));
        vec![
            b.pos.x,
            b.pos.y,
            dir_code(b.heading),
            mode_code(b.mode),
            sx,
            sy,
            sa,
            b.cleaned.len() as i32,
            attacking(&b) as i32,
            b.ticks as i32,
        ]
    }

    /// Cleaned cells after the first `n` events, as `[x0, y0, x1, y1, …]`.
    pub fn cleaned(&self, n: u32) -> Vec<i32> {
        self.brain_at(n)
            .cleaned
            .iter()
            .flat_map(|c| [c.x, c.y])
            .collect()
    }

    fn brain_at(&self, n: u32) -> Brain {
        self.checkpoints
            .output_at(&self.brain, self.log.view(), n as usize)
    }

    fn checkpoint(&mut self) {
        let n = self.log.len();
        if crate::due(&self.checkpoints, n, CHECKPOINT_EVERY) {
            self.checkpoints.take(&self.brain, self.log.view(), n);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshots_agree_with_folding_from_zero() {
        let mut app = VacuumApp::new(true, 6, 6);
        app.start();
        for i in 0..60 {
            app.frame((i % 5) - 1, i % 7 == 0, 100.0);
        }
        assert!(app.checkpoint_for(app.len()) > 0);
        for n in 0..=app.len() {
            let from_zero = app.brain.run(app.log.prefix(n as usize));
            assert_eq!(app.brain_at(n), from_zero, "n = {n}");
        }
        let t = app.truth();
        assert_eq!(t[2], 0, "careful: no attacks");
    }
}
