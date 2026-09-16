//! The room as a host. It appends inputs and senses to the log, reads her
//! heading as an output, and honours her e-stop as a latch. It is the only
//! thing in this crate that knows where the human really is.

use std::collections::BTreeSet;

use logfold_core::{Checkpoints, Fold, Log, diff_effects, in_flight};

use crate::{Brain, Cell, Cmd, Dir, Effect, Ev, KEY, Room, Vacuum, events};

/// Everything the host needs per frame: her brain and what it has started.
type HostState = (Brain, BTreeSet<Effect>);

#[derive(Clone, Debug)]
pub struct Sim {
    pub room: Room,
    pub human: Cell,
    pub her: Cell,
    /// Motors killed by an e-stop. Cleared by `Start`.
    pub latched: bool,
    pub attacks: u32,
    pub bumps: u32,
    pub ms: u64,
    /// Host-side checkpoint policy: the host state at the end of every
    /// frame, so each frame costs the events since the last one, not the
    /// whole log. Only the latest is kept; the sim is a host, not an
    /// archive.
    checkpoints: Checkpoints<HostState>,
}

/// What the human tries to do in a frame, and what the world does to her senses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Frame {
    pub human: Option<Dir>,
    /// Drop a sighting that is beyond the reliable range.
    pub dropout: bool,
    /// Milliseconds this frame took.
    pub dt: u64,
}

// ---------- the world's rules, pure, shared with any host that simulates her ----------

/// The human's move: never into furniture or walls, never onto her.
pub fn human_move(room: &Room, human: Cell, her: Cell, d: Dir) -> Cell {
    let t = human.step(d);
    if room.free(t) && t != her { t } else { human }
}

/// Where she ends the frame given her heading, and whether she bumped.
/// A latched motor does not move.
pub fn advance(room: &Room, her: Cell, heading: Option<Dir>, latched: bool) -> (Cell, bool) {
    match heading {
        Some(d) if !latched => {
            let t = her.step(d);
            if room.free(t) {
                (t, false)
            } else {
                (her, true)
            }
        }
        _ => (her, false),
    }
}

/// What her sensor reports about the human at the end of a frame:
/// reliable within `RELIABLE_RANGE`, flaky out to `FLAKY_RANGE`.
pub fn sighting(her: Cell, human: Cell, dropout: bool) -> Option<Cell> {
    let dist = her.dist(human);
    (dist <= crate::RELIABLE_RANGE || (dist <= crate::FLAKY_RANGE && !dropout)).then_some(human)
}

/// Her brain zipped with the in-flight set: one fold, one checkpoint.
fn host_fold(brain: &Fold<Ev, Brain>) -> Fold<Ev, HostState> {
    brain.clone().zip(in_flight::<Vacuum>())
}

impl Sim {
    pub fn new(room: Room, human: Cell) -> Self {
        let her = room.dock;
        Self {
            room,
            human,
            her,
            latched: false,
            attacks: 0,
            bumps: 0,
            ms: 0,
            checkpoints: Checkpoints::new(),
        }
    }

    /// Her brain as of the end of `log`, resumed from the last checkpoint.
    pub fn brain_now(&self, log: &Log<Ev>, brain: &Fold<Ev, Brain>) -> Brain {
        self.host_now(log, brain).0
    }

    fn host_now(&self, log: &Log<Ev>, brain: &Fold<Ev, Brain>) -> HostState {
        self.checkpoints
            .output_at(&host_fold(brain), log.view(), log.len())
    }

    fn checkpoint(&mut self, log: &Log<Ev>, brain: &Fold<Ev, Brain>) {
        let n = log.len();
        let state = self.host_now(log, brain);
        self.checkpoints = Checkpoints::new();
        self.checkpoints.insert(n, state);
    }

    /// A command from the owner. Appends the input, then lets the host act.
    pub fn command(&mut self, log: &mut Log<Ev>, brain: &Fold<Ev, Brain>, cmd: Cmd) {
        log.append(events::cmd(cmd));
        if cmd == Cmd::Start {
            self.latched = false;
        }
        self.host_acts(log, brain);
    }

    /// One frame: human moves, she moves, the world reports, time ticks,
    /// the host acts. Returns true if she ended the frame in the human's cell.
    pub fn frame(&mut self, log: &mut Log<Ev>, brain: &Fold<Ev, Brain>, f: Frame) -> bool {
        // 1. the human moves, never onto her.
        if let Some(d) = f.human {
            self.human = human_move(&self.room, self.human, self.her, d);
        }
        // 2. she moves along her setpoint, unless latched.
        let heading = self.brain_now(log, brain).heading;
        let (her, bumped) = advance(&self.room, self.her, heading, self.latched);
        self.her = her;
        self.bumps += u32::from(bumped);
        // An attack is ending the frame in the human's cell, whether she
        // drove into it or sat there. Same definition as `attacking` on
        // the log side, so the two counts must agree.
        let attacked = self.her == self.human;
        self.attacks += u32::from(attacked);
        // 3. the world reports.
        if bumped {
            log.append(events::bump());
        }
        if let Some(seen) = sighting(self.her, self.human, f.dropout) {
            log.append(events::human(seen));
        }
        // 4. time.
        self.ms += f.dt;
        log.append(events::tick(self.ms));
        // 5. the host.
        self.host_acts(log, brain);
        attacked
    }

    /// Start whatever she desires that has not been started: record it in
    /// the log first, then perform it. The in-flight set comes from the
    /// same checkpoint as her brain, so nothing here refolds the log.
    pub fn host_acts(&mut self, log: &mut Log<Ev>, brain: &Fold<Ev, Brain>) {
        let (b, in_flight) = self.host_now(log, brain);
        let d = diff_effects(&b.desired_effects(), &in_flight);
        for fx in d.start {
            log.append(Ev::started(KEY, fx.clone()));
            self.perform(&fx);
        }
        self.checkpoint(log, brain);
    }

    /// The world side of an effect. Exhaustive on purpose: a new effect
    /// variant must say what the room does with it.
    fn perform(&mut self, fx: &Effect) {
        match fx {
            Effect::EStop { .. } => self.latched = true,
        }
    }
}
