//! Brunhilda's studio: two applications of the framework in one app, on
//! one log.
//!
//! The *panel* is UI state: run or pause, which policy, sensor dropouts,
//! frame rate. The *robot* is Brunhilda's brain, unchanged from
//! `brunhilda`, driven by the policy the panel chose. The *world* is what
//! the sim used to hold in memory: where you are, whether her motors are
//! latched, how many bumps and attacks. All three are one fold over one
//! log, because your arrow keys are inputs like everything else, so the
//! timeline scrubs the whole session, you included.
//!
//! The world's rules (`brunhilda::sim`) run as the component's simulated
//! world: each frame they report what her sensor saw, the host ticks, and
//! her brain commits its move. Nothing here touches a screen; the studio
//! page renders 96 cells and two dots from numbers.

use std::collections::BTreeSet;

use brunhilda::sim::{advance, human_move, sighting};
use brunhilda::{
    Brain, Cell, Cmd, Dir, Effect, H, KEY as ROBOT, Mode, Policy, Room, Sense, W, attacking,
    step as robot_step,
};
use logfold_core::{Component, Domain, Event, Fold, Index, Name, Projection};

pub const KEY: &str = "studio";

pub struct App;

impl Domain for App {
    type Input = Input;
    type Sense = Sense;
    type Effect = Effect;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Input {
    Panel(Panel),
    Robot(Cmd),
    Human(Dir),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Panel {
    Run,
    Pause,
    Naive,
    Careful,
    ToggleDropouts,
    Faster,
    Slower,
}

pub type Ev = Event<App>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct State {
    pub policy: Policy,
    pub running: bool,
    pub dropouts: bool,
    /// Frames per second the page should run at while `running`.
    pub fps: u8,
    pub brain: Brain,
    pub human: Cell,
    pub latched: bool,
    pub attacks: u32,
    pub bumps: u32,
}

impl State {
    pub fn new(room: &Room) -> Self {
        Self {
            policy: Policy::Careful,
            running: false,
            dropouts: false,
            fps: 4,
            brain: Brain::at(room.dock),
            human: Cell::new(6, 6),
            latched: false,
            attacks: 0,
            bumps: 0,
        }
    }
}

/// The robot's view of a studio event: her commands, senses, ticks and
/// started effects, under her own key. Panel and human inputs are not hers.
fn as_robot(ev: &Ev) -> Option<Event<brunhilda::Vacuum>> {
    Some(match ev {
        Event::Tick { ms } => Event::Tick { ms: *ms },
        Event::Input {
            input: Input::Robot(c),
            ..
        } => Event::input(ROBOT, *c),
        Event::Input { .. } => return None,
        Event::Sense { sense, .. } => Event::sense(ROBOT, *sense),
        Event::Io { req, res, .. } => Event::io(ROBOT, *req, res.clone()),
        Event::Started { effect, .. } => Event::started(ROBOT, effect.clone()),
    })
}

pub fn step(room: &Room, mut s: State, i: Index, ev: &Ev) -> State {
    match ev {
        Event::Input {
            input: Input::Panel(p),
            ..
        } => match p {
            Panel::Run => s.running = true,
            Panel::Pause => s.running = false,
            Panel::Naive => s.policy = Policy::Naive,
            Panel::Careful => s.policy = Policy::Careful,
            Panel::ToggleDropouts => s.dropouts = !s.dropouts,
            Panel::Faster => s.fps = (s.fps * 2).min(16),
            Panel::Slower => s.fps = (s.fps / 2).max(1),
        },
        Event::Input {
            input: Input::Human(d),
            ..
        } => {
            s.human = human_move(room, s.human, s.brain.pos, *d);
        }
        Event::Input {
            input: Input::Robot(Cmd::Start),
            ..
        } => s.latched = false,
        Event::Started {
            effect: Effect::EStop { .. },
            ..
        } => s.latched = true,
        Event::Sense {
            sense: Sense::Bump, ..
        } => s.bumps += 1,
        _ => {}
    }
    if let Some(rev) = as_robot(ev) {
        s.brain = robot_step(room, s.policy, s.brain, i, &rev);
        if matches!(ev, Event::Tick { .. }) {
            s.attacks += u32::from(attacking(&s.brain));
        }
    }
    s
}

/// The world's report for one frame: she moves along her heading (unless
/// latched), bumps if she cannot, and her sensor sees you or not. Dropouts,
/// when enabled, are a hash of her tick count: pseudo-random, replayable.
pub fn simulate(room: &Room, s: &State, _dt: u64) -> Vec<Ev> {
    let (her, bumped) = advance(room, s.brain.pos, s.brain.heading, s.latched);
    let dropout = s.dropouts && (s.brain.ticks.wrapping_mul(0x9E37_79B9_7F4A_7C15) >> 61) < 3;
    let mut out = Vec::new();
    if bumped {
        out.push(Event::sense(KEY, Sense::Bump));
    }
    if let Some(seen) = sighting(her, s.human, dropout) {
        out.push(Event::sense(KEY, Sense::Human(seen)));
    }
    out
}

/// A cell's target name, `c-x-y`. A static table rather than `format!`,
/// which would link string formatting into the bundle; column-major, the
/// same order as `DistMap`.
pub fn cell_name(c: Cell) -> Name {
    const NAMES: [&str; (W * H) as usize] = [
        "c-0-0", "c-0-1", "c-0-2", "c-0-3", "c-0-4", "c-0-5", "c-0-6", "c-0-7", "c-1-0", "c-1-1",
        "c-1-2", "c-1-3", "c-1-4", "c-1-5", "c-1-6", "c-1-7", "c-2-0", "c-2-1", "c-2-2", "c-2-3",
        "c-2-4", "c-2-5", "c-2-6", "c-2-7", "c-3-0", "c-3-1", "c-3-2", "c-3-3", "c-3-4", "c-3-5",
        "c-3-6", "c-3-7", "c-4-0", "c-4-1", "c-4-2", "c-4-3", "c-4-4", "c-4-5", "c-4-6", "c-4-7",
        "c-5-0", "c-5-1", "c-5-2", "c-5-3", "c-5-4", "c-5-5", "c-5-6", "c-5-7", "c-6-0", "c-6-1",
        "c-6-2", "c-6-3", "c-6-4", "c-6-5", "c-6-6", "c-6-7", "c-7-0", "c-7-1", "c-7-2", "c-7-3",
        "c-7-4", "c-7-5", "c-7-6", "c-7-7", "c-8-0", "c-8-1", "c-8-2", "c-8-3", "c-8-4", "c-8-5",
        "c-8-6", "c-8-7", "c-9-0", "c-9-1", "c-9-2", "c-9-3", "c-9-4", "c-9-5", "c-9-6", "c-9-7",
        "c-10-0", "c-10-1", "c-10-2", "c-10-3", "c-10-4", "c-10-5", "c-10-6", "c-10-7", "c-11-0",
        "c-11-1", "c-11-2", "c-11-3", "c-11-4", "c-11-5", "c-11-6", "c-11-7",
    ];
    NAMES[(c.x * H + c.y) as usize]
}

fn mode_code(m: Mode) -> u8 {
    match m {
        Mode::Idle => 0,
        Mode::Cleaning => 1,
        Mode::Docking => 2,
        Mode::Stopped { .. } => 3,
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

/// Everything the page shows, as numbers. Booleans and enums are
/// attributes on the root for selectors; positions and counts are
/// variables for `calc()`; cells are targets of their own.
pub fn project(room: &Room, s: &State) -> Projection {
    let b = &s.brain;
    let (sx, sy, sa) = b
        .sighting
        .map_or((-1, -1, -1), |t| (t.at.x, t.at.y, t.age as i32));
    let mut p = Projection::new()
        .attr("root", "data-running", u8::from(s.running))
        .attr("root", "data-dropouts", u8::from(s.dropouts))
        .attr("root", "data-policy", u8::from(s.policy == Policy::Careful))
        .attr("root", "data-mode", mode_code(b.mode))
        .attr("root", "data-latched", u8::from(s.latched))
        .attr("root", "data-attacking", u8::from(attacking(b)))
        .attr("root", "data-heading", dir_code(b.heading))
        .attr("root", "data-seen", u8::from(b.sighting.is_some()))
        .attr("root", "data-fresh", u8::from(sa == 0))
        .var("root", "--fps", s.fps)
        .var("root", "--her-x", b.pos.x)
        .var("root", "--her-y", b.pos.y)
        .var("root", "--human-x", s.human.x)
        .var("root", "--human-y", s.human.y)
        .var("root", "--seen-x", sx)
        .var("root", "--seen-y", sy)
        .var("root", "--seen-age", sa)
        .var("root", "--attacks", s.attacks)
        .var("root", "--bumps", s.bumps)
        .var("root", "--cleaned", b.cleaned.len() as u32)
        .var("root", "--ticks", b.ticks as u32);
    for c in &room.furniture {
        p = p.attr(cell_name(*c), "data-furniture", 1u8);
    }
    p = p.attr(cell_name(room.dock), "data-dock", 1u8);
    for c in &b.cleaned {
        p = p.attr(cell_name(*c), "data-cleaned", 1u8);
    }
    p
}

pub fn component() -> Component<App, State> {
    let room = Room::default();
    let (r1, r2, r3) = (room.clone(), room.clone(), room.clone());
    Component::new(
        KEY,
        Fold::new(State::new(&room), move |s, i, ev| step(&r1, s, i, ev)),
    )
    .project(move |s| project(&r2, s))
    .effects(|s| s.brain.desired_effects())
    .simulate(move |s, dt| simulate(&r3, s, dt))
    .input("run", Input::Panel(Panel::Run))
    .input("pause", Input::Panel(Panel::Pause))
    .input("naive", Input::Panel(Panel::Naive))
    .input("careful", Input::Panel(Panel::Careful))
    .input("dropouts", Input::Panel(Panel::ToggleDropouts))
    .input("faster", Input::Panel(Panel::Faster))
    .input("slower", Input::Panel(Panel::Slower))
    .input("start", Input::Robot(Cmd::Start))
    .input("dock", Input::Robot(Cmd::Dock))
    .input("estop", Input::Robot(Cmd::EStop))
    .input("north", Input::Human(Dir::N))
    .input("east", Input::Human(Dir::E))
    .input("south", Input::Human(Dir::S))
    .input("west", Input::Human(Dir::W))
}

/// The effects the studio can start, for hosts that want the type.
pub fn effects(s: &State) -> BTreeSet<Effect> {
    s.brain.desired_effects()
}

#[cfg(test)]
mod tests {
    use super::*;
    use logfold_core::{Log, Slot, checkpoint_law};

    fn by_name(c: &Component<App, State>, name: &str) -> Ev {
        let id = c.input_names().position(|n| n == name).unwrap();
        c.input_event(id).unwrap()
    }

    #[test]
    fn the_panel_and_the_human_are_state_too() {
        let c = component();
        let log: Log<Ev> = [
            "run", "naive", "dropouts", "faster", "north", "north", "west",
        ]
        .into_iter()
        .map(|n| by_name(&c, n))
        .collect();
        let s = c.fold.run(log.view());
        assert!(s.running && s.dropouts);
        assert_eq!(s.policy, Policy::Naive);
        assert_eq!(s.fps, 8);
        assert_eq!(s.human, Cell::new(5, 4));
        checkpoint_law(&c.fold, log.view()).unwrap();
    }

    #[test]
    fn the_projection_names_cells() {
        let c = component();
        let p = (c.project)(&c.fold.run(Log::<Ev>::new().view()));
        assert_eq!(p.get(Slot::attr("c-0-0", "data-dock")), Some(1.0));
        assert_eq!(p.get(Slot::attr("c-4-2", "data-furniture")), Some(1.0));
        assert_eq!(p.get(Slot::attr("c-1-1", "data-cleaned")), None);
        assert_eq!(p.get(Slot::var("root", "--human-x")), Some(6.0));
    }
}
