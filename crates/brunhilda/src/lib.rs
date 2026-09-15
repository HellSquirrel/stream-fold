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
/// After giving up on cells you are standing in, she waits at the dock
/// this many ticks before trying again.
pub const RETRY_TICKS: u64 = 40;

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

    /// Every free cell in the room.
    pub fn free_cells(&self) -> impl Iterator<Item = Cell> + '_ {
        (0..W)
            .flat_map(|x| (0..H).map(move |y| Cell::new(x, y)))
            .filter(|c| self.free(*c))
    }

    /// Breadth-first distance from the nearest of `sources`, over free
    /// cells that also satisfy `passable`, with 4-neighbour moves.
    /// Unreachable cells get `None`.
    pub fn distances(
        &self,
        sources: impl IntoIterator<Item = Cell>,
        passable: impl Fn(Cell) -> bool,
    ) -> DistMap {
        let ok = |c: Cell| self.free(c) && passable(c);
        let mut d = DistMap([None; (W * H) as usize]);
        let mut queue = std::collections::VecDeque::new();
        for c in sources {
            if ok(c) && d.get(c).is_none() {
                d.set(c, 0);
                queue.push_back(c);
            }
        }
        while let Some(c) = queue.pop_front() {
            let n = d.get(c).unwrap_or(0) + 1;
            for dir in Dir::ALL {
                let t = c.step(dir);
                if ok(t) && d.get(t).is_none() {
                    d.set(t, n);
                    queue.push_back(t);
                }
            }
        }
        d
    }
}

/// Distances over the grid, from [`Room::distances`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DistMap([Option<u16>; (W * H) as usize]);

impl DistMap {
    pub fn get(&self, c: Cell) -> Option<u16> {
        if c.in_bounds() {
            self.0[(c.x * H + c.y) as usize]
        } else {
            None
        }
    }
    fn set(&mut self, c: Cell, v: u16) {
        self.0[(c.x * H + c.y) as usize] = Some(v);
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
    /// Bumps so far, at most one per frame. With `ticks`, the naive
    /// policy's source of variety.
    pub bumps: u64,
    /// She gave up on the cells around this sighting at this tick, and is
    /// waiting at the dock. Cleared when you are seen somewhere else or
    /// after `RETRY_TICKS`.
    pub blocked: Option<(Cell, u64)>,
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
            bumps: 0,
            blocked: None,
            bumped: false,
            seen: None,
        }
    }

    pub fn moving(&self) -> bool {
        matches!(self.mode, Mode::Cleaning | Mode::Docking)
    }

    /// Cells she remembers giving up on: the ring around the sighting
    /// that blocked her.
    pub fn kept_out(&self, c: Cell) -> bool {
        matches!(self.blocked, Some((at, _)) if c.dist(at) <= 1)
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

/// Two vacuums. `Naive` has no map: it drives until it bumps, turns left
/// or right, turns every so often anyway, and only refuses the one cell
/// it last saw you in. Which way it turns is pseudo-random but a pure
/// function of the log (see `wander`). The fuzzer breaks it. `Careful` knows the room, plans the nearest uncleaned
/// cell by breadth-first search, treats the ring around a fresh sighting
/// as wall, and goes back to the dock to wait when that ring holds the
/// only cells left. It survives.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Policy {
    /// No map. Bump and turn. Won't drive into the cell the human was
    /// last seen in. That's all.
    Naive,
    /// Won't drive into any cell the human could step into this frame:
    /// anything within Chebyshev 1 of a fresh sighting. And won't move at
    /// all until she has had one frame to look.
    Careful,
}

impl Policy {
    /// Would this policy refuse to drive into `target` given the last sighting?
    pub fn forbidden(self, target: Cell, sighting: Option<Sighting>) -> bool {
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
        match self {
            Policy::Naive => self.wander(b),
            Policy::Careful => self.plan(room, b),
        }
    }

    /// No map. Keep going; after a bump, turn left or right (never
    /// straight back), and every so often turn anyway. Which way is a
    /// hash of her own tick and bump counts: pseudo-random to the eye,
    /// but a pure function of the log, so replay is exact.
    fn wander(self, b: &Brain) -> Option<Dir> {
        let noise = (b.ticks.wrapping_mul(0x9E37_79B9_7F4A_7C15)
            ^ b.bumps.wrapping_mul(0xC2B2_AE3D_27D4_EB4F))
            >> 17;
        let turn = |d: Dir, k: u64| (0..k % 4).fold(d, |d, _| d.clockwise());
        let mut d = b.heading.unwrap_or(Dir::E);
        if b.bumped {
            d = turn(d, if noise & 1 == 0 { 1 } else { 3 });
        } else if b.ticks > 0 && noise.is_multiple_of(7) {
            d = turn(d, 1 + noise / 8 % 3);
        }
        (0..4)
            .map(|_| {
                let cur = d;
                d = d.clockwise();
                cur
            })
            .find(|d| !self.forbidden(b.pos.step(*d), b.sighting))
    }

    /// With a map: breadth-first search to the nearest goal, treating the
    /// ring around a fresh sighting as wall. Goals: uncleaned cells while
    /// cleaning, else the dock. If no uncleaned cell is reachable, the
    /// dock is the goal too: she goes home and waits for you to move.
    fn plan(self, room: &Room, b: &Brain) -> Option<Dir> {
        // No sighting after a tick means nobody is within reliable range.
        // No sighting before the first tick means nothing at all: look first.
        if b.ticks == 0 {
            return None;
        }
        // Wall: the ring around a fresh sighting, plus the ring she
        // remembers giving up on.
        let passable = |c: Cell| !self.forbidden(c, b.sighting) && !b.kept_out(c);
        let to_dock = || room.distances([room.dock], passable);
        let goal = match b.mode {
            Mode::Docking => to_dock(),
            _ if b.blocked.is_some() => to_dock(),
            _ => {
                // Her own cell is cleaned on this tick regardless; not a goal.
                let work = room.distances(
                    room.free_cells()
                        .filter(|c| !b.cleaned.contains(c) && *c != b.pos),
                    passable,
                );
                if work.get(b.pos).is_some() {
                    work
                } else {
                    to_dock()
                }
            }
        };
        if goal.get(b.pos) == Some(0) {
            return None; // already there
        }
        // Candidate order: keep heading, then clockwise, so ties don't zigzag.
        let mut d = b.heading.unwrap_or(Dir::E);
        (0..4)
            .map(|_| {
                let cur = d;
                d = d.clockwise();
                cur
            })
            .filter(|d| room.free(b.pos.step(*d)) && passable(b.pos.step(*d)))
            .filter_map(|d| goal.get(b.pos.step(d)).map(|dist| (dist, d)))
            .min_by_key(|(dist, _)| *dist)
            .map(|(_, d)| d)
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
                if room.free_cells().all(|c| b.cleaned.contains(&c)) {
                    b.mode = Mode::Docking; // job done
                }
            }
            if policy == Policy::Careful {
                b.blocked = match (b.blocked, b.sighting) {
                    // you were seen somewhere else: the way may be clear
                    (Some((at, _)), Some(s)) if s.age == 0 && s.at.dist(at) > 1 => None,
                    // waited long enough: try again
                    (Some((_, since)), _) if b.ticks + 1 - since > RETRY_TICKS => None,
                    (Some(bl), _) => Some(bl),
                    // nothing left to clean except the ring around you: give up for now
                    (None, Some(s)) if s.age == 0 && b.mode == Mode::Cleaning => {
                        let passable = |c: Cell| !policy.forbidden(c, b.sighting);
                        let work = room.distances(
                            room.free_cells()
                                .filter(|c| !b.cleaned.contains(c) && *c != b.pos),
                            passable,
                        );
                        if work.get(b.pos).is_none() {
                            Some((s.at, b.ticks + 1))
                        } else {
                            None
                        }
                    }
                    (None, _) => None,
                };
            }
            if b.mode == Mode::Docking && b.pos == room.dock {
                b.mode = Mode::Idle;
            }
            b.bumps += u64::from(b.bumped); // at most one per frame, like the sim
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
        checkpoint_law(&b, log.view()).unwrap();
    }

    #[test]
    fn naive_has_no_map_and_bumps_its_way_around() {
        use crate::sim::{Frame, Sim};
        let room = Room::default();
        let b = brain(room.clone(), Policy::Naive);
        let mut sim = Sim::new(room, Cell::new(11, 7));
        let mut log = Log::new();
        sim.command(&mut log, &b, Cmd::Start);
        for _ in 0..40 {
            sim.frame(
                &mut log,
                &b,
                Frame {
                    human: None,
                    dropout: false,
                    dt: 100,
                },
            );
        }
        assert!(sim.bumps > 0, "she should have hit the east wall by now");
        let s = b.run(log.view());
        assert_eq!(s.pos, sim.her, "and still know where she is");
        assert_eq!(
            s.bumps,
            u64::from(sim.bumps),
            "and count her bumps like the sim does"
        );
    }

    #[test]
    fn naive_drives_next_to_you_careful_does_not() {
        // You were just seen two cells east of her; the cell between you is
        // one you could step into this frame.
        let seen = Some(Sighting {
            at: Cell::new(3, 0),
            age: 0,
        });
        let between = Cell::new(2, 0);
        assert!(
            !Policy::Naive.forbidden(between, seen),
            "naive: happy to drive to (2,0)"
        );
        assert!(
            Policy::Careful.forbidden(between, seen),
            "careful: (2,0) is within your reach"
        );
        assert!(
            Policy::Naive.forbidden(Cell::new(3, 0), seen),
            "naive: but not into you"
        );
        let stale = Some(Sighting {
            at: Cell::new(3, 0),
            age: 1,
        });
        assert!(
            !Policy::Careful.forbidden(between, stale),
            "careful: a stale sighting is no wall"
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

    #[test]
    fn covers_the_room_then_docks() {
        use crate::sim::{Frame, Sim};
        let room = Room::default();
        let b = brain(room.clone(), Policy::Careful);
        // Park the human in the far corner; she must clean everything
        // she can reach without coming within one cell of him.
        let human = Cell::new(11, 7);
        let mut sim = Sim::new(room.clone(), human);
        let mut log = Log::new();
        sim.command(&mut log, &b, Cmd::Start);
        let mut frames_at_dock_after_plateau = 0;
        for i in 0..400 {
            sim.frame(
                &mut log,
                &b,
                Frame {
                    human: None,
                    dropout: false,
                    dt: 100,
                },
            );
            if i >= 200 && sim.her == room.dock {
                frames_at_dock_after_plateau += 1;
            }
        }
        let s = b.run(log.view());
        let expected: BTreeSet<Cell> = room.free_cells().filter(|c| c.dist(human) > 1).collect();
        let missing: Vec<Cell> = expected.difference(&s.cleaned).copied().collect();
        assert!(missing.is_empty(), "uncleaned: {missing:?}");
        assert_eq!(sim.attacks, 0);
        assert!(
            frames_at_dock_after_plateau > 100,
            "blocked: should mostly wait at the dock, was there {frames_at_dock_after_plateau}/200 frames"
        );
        // Now move the human away from both the corner and the dock and let
        // her finish: the job completes and she docks.
        sim.human = Cell::new(6, 7);
        let mut moved = sim.clone();
        for _ in 0..200 {
            moved.frame(
                &mut log,
                &b,
                Frame {
                    human: None,
                    dropout: false,
                    dt: 100,
                },
            );
        }
        let s = b.run(log.view());
        assert_eq!(
            s.cleaned.len(),
            room.free_cells().count(),
            "everything cleaned"
        );
        assert_eq!((s.mode, s.pos), (Mode::Idle, room.dock), "docked and idle");
    }
}
