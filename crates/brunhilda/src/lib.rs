//! Brunhilda, a robot vacuum cleaner. She attacks her owner from time to
//! time during cleaning. This crate is her brain as a fold, the room as a
//! simulated host (`sim`), and the expectation that she never does that.
//!
//! # Frame protocol
//!
//! The world runs in frames. In each frame the human moves at most one
//! cell, then Brunhilda moves at most one cell along the heading she chose
//! at the previous tick, then the world reports what it sensed (a bump, a
//! sighting of the human), then time ticks. So the log for one frame is
//! `[Sense…] Tick`, and her brain commits her move and chooses the next
//! heading on the `Tick`.
//!
//! Her position is dead reckoning: she assumes each intended move
//! happened unless a `Bump` arrived in the same frame. The sim guarantees
//! exactly that, so her belief and the world agree, and the fuzzer checks
//! it at every frame boundary.
//!
//! # Sensing
//!
//! A sighting is reliable when the human is within Chebyshev distance 2
//! and flaky out to distance 4: the sim may drop those. Beyond 4 she is
//! blind. The sim reports positions *after* the frame's moves.
//!
//! # Outputs and actions
//!
//! Her heading is an output: an idempotent function of her state that the
//! sim reads every frame and never logs. An e-stop is an action: a
//! fire-and-forget effect the host records with `Started` and honours as
//! a latch. That split is why both exist in the proposal.

pub mod sim;

use std::collections::BTreeSet;

use logfold_core::{Action, Domain, Event, Fold, IdemKey, Index, ReqId};

pub const KEY: &str = "brunhilda";
pub const W: i32 = 12;
pub const H: i32 = 8;

/// Sightings within this Chebyshev distance are always reported.
pub const RELIABLE_RANGE: i32 = 2;
/// Sightings out to this distance are reported unless dropped.
pub const FLAKY_RANGE: i32 = 4;

// ---------- domain ----------

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Vacuum;

impl Domain for Vacuum {
    type Input = Cmd;
    type Sense = Sense;
    type Effect = Effect;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Cmd {
    Start,
    Dock,
    EStop,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Sense {
    /// She tried to move and hit a wall or furniture.
    Bump,
    /// The human is at this cell, as of the end of this frame.
    Human(Cell),
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Effect {
    /// Kill the motors. Fire-and-forget; the host latches until `Start`.
    EStop { idem: IdemKey },
}

impl Action for Effect {
    fn req(&self) -> Option<ReqId> {
        None
    }
}

pub type Ev = Event<Vacuum>;

// ---------- geometry ----------

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Cell {
    pub x: i32,
    pub y: i32,
}

impl Cell {
    pub const fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }

    pub fn step(self, d: Dir) -> Cell {
        let (dx, dy) = d.delta();
        Cell::new(self.x + dx, self.y + dy)
    }

    pub fn in_bounds(self) -> bool {
        (0..W).contains(&self.x) && (0..H).contains(&self.y)
    }

    /// Chebyshev distance: king moves.
    pub fn dist(self, o: Cell) -> i32 {
        (self.x - o.x).abs().max((self.y - o.y).abs())
    }

    pub fn manhattan(self, o: Cell) -> i32 {
        (self.x - o.x).abs() + (self.y - o.y).abs()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Dir {
    N,
    E,
    S,
    W,
}

impl Dir {
    pub const ALL: [Dir; 4] = [Dir::N, Dir::E, Dir::S, Dir::W];

    pub fn delta(self) -> (i32, i32) {
        match self {
            Dir::N => (0, -1),
            Dir::E => (1, 0),
            Dir::S => (0, 1),
            Dir::W => (-1, 0),
        }
    }

    pub fn clockwise(self) -> Dir {
        match self {
            Dir::N => Dir::E,
            Dir::E => Dir::S,
            Dir::S => Dir::W,
            Dir::W => Dir::N,
        }
    }
}

/// The room. Static and known to her brain; a real robot would have
/// mapped it on a previous run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Room {
    pub furniture: BTreeSet<Cell>,
    pub dock: Cell,
}

impl Default for Room {
    fn default() -> Self {
        let mut furniture = BTreeSet::new();
        for x in 3..=5 {
            furniture.insert(Cell::new(x, 2)); // sofa
        }
        for x in 8..=9 {
            for y in 4..=5 {
                furniture.insert(Cell::new(x, y)); // table
            }
        }
        Self {
            furniture,
            dock: Cell::new(0, 0),
        }
    }
}

impl Room {
    pub fn free(&self, c: Cell) -> bool {
        c.in_bounds() && !self.furniture.contains(&c)
    }
}

// ---------- her brain ----------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Idle,
    Cleaning,
    Docking,
    Stopped { since: Index },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sighting {
    pub at: Cell,
    /// Ticks since it was reported. 0 means this frame.
    pub age: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Brain {
    pub pos: Cell,
    /// The setpoint for the next frame. This is her output.
    pub heading: Option<Dir>,
    pub mode: Mode,
    pub cleaned: BTreeSet<Cell>,
    pub sighting: Option<Sighting>,
    pub ticks: u64,
    /// A `Bump` arrived since the last tick.
    bumped: bool,
    /// A sighting arrived since the last tick.
    seen: Option<Cell>,
}

impl Brain {
    pub fn at(pos: Cell) -> Self {
        Self {
            pos,
            heading: None,
            mode: Mode::Idle,
            cleaned: BTreeSet::new(),
            sighting: None,
            ticks: 0,
            bumped: false,
            seen: None,
        }
    }

    pub fn moving(&self) -> bool {
        matches!(self.mode, Mode::Cleaning | Mode::Docking)
    }

    /// The set of effects she wants in flight. An e-stop, once, per stop.
    pub fn desired_effects(&self) -> BTreeSet<Effect> {
        match self.mode {
            Mode::Stopped { since } => [Effect::EStop {
                idem: IdemKey::new(KEY, since),
            }]
            .into(),
            _ => BTreeSet::new(),
        }
    }
}

/// How she picks her next heading. `Naive` is the controller you would
/// write first; the fuzzer breaks it. `Careful` survives.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Policy {
    /// Won't drive into the cell the human was last seen in. That's all.
    Naive,
    /// Won't drive into any cell the human could step into this frame:
    /// anything within Chebyshev 1 of a fresh sighting. And won't move at
    /// all until she has had one frame to look.
    Careful,
}

impl Policy {
    fn forbidden(self, target: Cell, sighting: Option<Sighting>) -> bool {
        match (self, sighting) {
            (_, None) => false,
            (Policy::Naive, Some(s)) => s.age == 0 && target == s.at,
            (Policy::Careful, Some(s)) => s.age == 0 && target.dist(s.at) <= 1,
        }
    }

    fn choose(self, room: &Room, b: &Brain) -> Option<Dir> {
        if !b.moving() {
            return None;
        }
        // No sighting after a tick means nobody is within reliable range.
        // No sighting before the first tick means nothing at all: look first.
        if self == Policy::Careful && b.ticks == 0 {
            return None;
        }
        let mut order: Vec<Dir> = Vec::with_capacity(4);
        let mut d = b.heading.unwrap_or(Dir::E);
        if b.bumped {
            d = d.clockwise();
        }
        for _ in 0..4 {
            order.push(d);
            d = d.clockwise();
        }
        if b.mode == Mode::Docking {
            order.sort_by_key(|d| b.pos.step(*d).manhattan(room.dock));
        }
        order
            .into_iter()
            .find(|d| room.free(b.pos.step(*d)) && !self.forbidden(b.pos.step(*d), b.sighting))
    }
}

/// One step of her brain. Public so expectations can look at the state
/// before an event without re-running the fold.
pub fn step(room: &Room, policy: Policy, mut b: Brain, index: Index, ev: &Ev) -> Brain {
    match ev {
        Event::Input { input, .. } => {
            // After an e-stop only `Start` resumes; the host's latch has
            // the same rule, and the fuzzer checks the two agree.
            b.mode = match (input, b.mode) {
                (Cmd::Start, _) => Mode::Cleaning,
                (Cmd::Dock, Mode::Stopped { .. }) => b.mode,
                (Cmd::Dock, _) => Mode::Docking,
                (Cmd::EStop, Mode::Stopped { .. }) => b.mode,
                (Cmd::EStop, _) => Mode::Stopped { since: index },
            };
            b.heading = policy.choose(room, &b);
        }
        Event::Sense {
            sense: Sense::Bump, ..
        } => b.bumped = true,
        Event::Sense {
            sense: Sense::Human(c),
            ..
        } => b.seen = Some(*c),
        Event::Tick { .. } => {
            if let (Some(d), false) = (b.heading, b.bumped) {
                b.pos = b.pos.step(d);
            }
            b.sighting = match (b.seen.take(), b.sighting) {
                (Some(at), _) => Some(Sighting { at, age: 0 }),
                (None, Some(s)) => Some(Sighting {
                    age: s.age + 1,
                    ..s
                }),
                (None, None) => None,
            };
            if b.mode == Mode::Cleaning {
                b.cleaned.insert(b.pos);
            }
            if b.mode == Mode::Docking && b.pos == room.dock {
                b.mode = Mode::Idle;
            }
            b.heading = policy.choose(room, &b);
            b.bumped = false;
            b.ticks += 1;
        }
        _ => {}
    }
    b
}

/// Her brain as a fold, scoped to her key, starting at the dock.
pub fn brain(room: Room, policy: Policy) -> Fold<'static, Ev, Brain> {
    let start = room.dock;
    Fold::new(Brain::at(start), move |b, i, ev| {
        step(&room, policy, b, i, ev)
    })
    .scoped(in_scope)
}

pub fn in_scope(ev: &Ev) -> bool {
    matches!(ev, Event::Tick { .. }) || ev.key() == Some(KEY)
}

/// Her output: the heading the world should apply next frame.
pub fn setpoint(b: &Brain) -> Option<Dir> {
    b.heading
}

/// Did she just end a frame in the human's cell? True only in the state
/// right after a `Tick`, which is the only time `pos` and a fresh
/// sighting describe the same instant.
pub fn attacking(b: &Brain) -> bool {
    matches!(b.sighting, Some(s) if s.age == 0 && s.at == b.pos)
}

pub mod events {
    use super::*;

    pub fn cmd(c: Cmd) -> Ev {
        Ev::input(KEY, c)
    }
    pub fn bump() -> Ev {
        Ev::sense(KEY, Sense::Bump)
    }
    pub fn human(at: Cell) -> Ev {
        Ev::sense(KEY, Sense::Human(at))
    }
    pub fn tick(ms: u64) -> Ev {
        Ev::tick(ms)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use logfold_core::{Log, checkpoint_law};

    #[test]
    fn idle_until_started_then_heads_east() {
        let b = brain(Room::default(), Policy::Careful);
        let log: Log<Ev> = [events::tick(16), events::cmd(Cmd::Start)]
            .into_iter()
            .collect();
        assert_eq!(b.run(log.prefix(1)).heading, None);
        assert_eq!(b.run(log.view()).heading, Some(Dir::E));
    }

    #[test]
    fn dead_reckoning_commits_on_tick_unless_bumped() {
        // Naive moves on the first tick; the mechanics under test are the same.
        let b = brain(Room::default(), Policy::Naive);
        let log: Log<Ev> = [
            events::cmd(Cmd::Start),
            events::tick(16),
            events::bump(),
            events::tick(32),
        ]
        .into_iter()
        .collect();
        assert_eq!(b.run(log.prefix(2)).pos, Cell::new(1, 0));
        let after_bump = b.run(log.view());
        assert_eq!(after_bump.pos, Cell::new(1, 0), "bump: no move");
        assert_eq!(after_bump.heading, Some(Dir::S), "bump: turned clockwise");
        checkpoint_law(&b, log.view()).unwrap();
    }

    #[test]
    fn naive_drives_next_to_you_careful_does_not() {
        let log: Log<Ev> = [
            events::cmd(Cmd::Start),
            events::human(Cell::new(3, 0)), // two cells east of her at (1,0) after this frame
            events::tick(16),
        ]
        .into_iter()
        .collect();
        let naive = brain(Room::default(), Policy::Naive).run(log.view());
        let careful = brain(Room::default(), Policy::Careful).run(log.view());
        assert_eq!(naive.pos, Cell::new(1, 0));
        assert_eq!(
            naive.heading,
            Some(Dir::E),
            "naive: happy to drive to (2,0)"
        );
        assert_ne!(
            careful.heading,
            Some(Dir::E),
            "careful: (2,0) is within reach of the human"
        );
    }

    #[test]
    fn estop_stops_and_wants_exactly_one_effect() {
        let b = brain(Room::default(), Policy::Careful);
        let log: Log<Ev> = [
            events::cmd(Cmd::Start),
            events::tick(16),
            events::cmd(Cmd::EStop),
        ]
        .into_iter()
        .collect();
        let s = b.run(log.view());
        assert_eq!(s.heading, None);
        assert_eq!(s.desired_effects().len(), 1);
        assert_eq!(s.mode, Mode::Stopped { since: 2 });
    }
}
