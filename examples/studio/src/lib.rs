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
    Brain, Cell, Cmd, Dir, Effect, H, KEY as ROBOT, Mode, Policy, Room, Sense, attacking,
    step as robot_step,
};
use logfold_core::{Component, Domain, Event, Fold, Index, Projection};

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

logfold_core::slots! {
    pub mod ui;
    root {
        class running;
        class dropouts;
        class policy: enum { naive, careful };
        class mode: enum { idle, cleaning, docking, stopped };
        class latched;
        class attacking;
        class heading: enum { none, north, east, south, west };
        class seen;
        class fresh;
        var fps: int = 4;
        var her_x: int;
        var her_y: int;
        var human_x: int = 6;
        var human_y: int = 6;
        var seen_x: int = -1;
        var seen_y: int = -1;
        var seen_age: int = -1;
        var attacks: int;
        var bumps: int;
        var cleaned: int;
        var ticks: int;
    }
    family cell((brunhilda::W * brunhilda::H)) {
        class cleaned;
        class furniture;
        class dock;
    }
    inputs { run, pause, naive, careful, dropouts, faster, slower, start, dock, estop, north, east, south, west }
    consts { room_w: brunhilda::W, room_h: brunhilda::H, cell_px: 40 }
}

/// A cell's index in the `cell` family: column-major, like `DistMap`.
pub fn cell_index(c: Cell) -> u32 {
    (c.x * H + c.y) as u32
}

/// Everything the page shows, as numbers on the declared slots. Booleans
/// and enums are attributes for selectors, spelled by name on the page;
/// positions and counts are variables for `calc()`; cells are a family.
pub fn project(room: &Room, s: &State) -> Projection {
    let b = &s.brain;
    let (sx, sy, sa) = b
        .sighting
        .map_or((-1, -1, -1), |t| (t.at.x, t.at.y, t.age as i32));
    let mut p = Projection::new()
        .set(ui::running.slot(), u8::from(s.running))
        .set(ui::dropouts.slot(), u8::from(s.dropouts))
        .set(
            ui::policy.slot(),
            if s.policy == Policy::Careful {
                ui::policy::careful
            } else {
                ui::policy::naive
            },
        )
        .set(
            ui::mode.slot(),
            match b.mode {
                Mode::Idle => ui::mode::idle,
                Mode::Cleaning => ui::mode::cleaning,
                Mode::Docking => ui::mode::docking,
                Mode::Stopped { .. } => ui::mode::stopped,
            },
        )
        .set(ui::latched.slot(), u8::from(s.latched))
        .set(ui::attacking.slot(), u8::from(attacking(b)))
        .set(
            ui::heading.slot(),
            match b.heading {
                None => ui::heading::none,
                Some(Dir::N) => ui::heading::north,
                Some(Dir::E) => ui::heading::east,
                Some(Dir::S) => ui::heading::south,
                Some(Dir::W) => ui::heading::west,
            },
        )
        .set(ui::seen.slot(), u8::from(b.sighting.is_some()))
        .set(ui::fresh.slot(), u8::from(sa == 0))
        .set(ui::fps.slot(), s.fps)
        .set(ui::her_x.slot(), b.pos.x)
        .set(ui::her_y.slot(), b.pos.y)
        .set(ui::human_x.slot(), s.human.x)
        .set(ui::human_y.slot(), s.human.y)
        .set(ui::seen_x.slot(), sx)
        .set(ui::seen_y.slot(), sy)
        .set(ui::seen_age.slot(), sa)
        .set(ui::attacks.slot(), s.attacks)
        .set(ui::bumps.slot(), s.bumps)
        .set(ui::cleaned.slot(), b.cleaned.len() as u32)
        .set(ui::ticks.slot(), b.ticks as u32);
    for c in &room.furniture {
        p = p.set(ui::cell::furniture.at(cell_index(*c)), 1u8);
    }
    p = p.set(ui::cell::dock.at(cell_index(room.dock)), 1u8);
    for c in &b.cleaned {
        p = p.set(ui::cell::cleaned.at(cell_index(*c)), 1u8);
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
    .manifest(&ui::MANIFEST)
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
    use logfold_core::{Log, checkpoint_law};

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
    fn the_projection_uses_the_declared_slots() {
        let c = component();
        let p = (c.project)(&c.fold.run(Log::<Ev>::new().view()));
        assert_eq!(
            p.get(ui::cell::dock.at(cell_index(Cell::new(0, 0)))),
            Some(1.0)
        );
        assert_eq!(
            p.get(ui::cell::furniture.at(cell_index(Cell::new(4, 2)))),
            Some(1.0)
        );
        assert_eq!(
            p.get(ui::cell::cleaned.at(cell_index(Cell::new(1, 1)))),
            None
        );
        assert_eq!(p.get(ui::human_x.slot()), Some(6.0));
        assert_eq!(p.get(ui::mode.slot()), Some(f64::from(ui::mode::idle)));
        assert_eq!(
            c.input_names().collect::<Vec<_>>(),
            ui::INPUTS,
            "inputs match the manifest"
        );
        assert!(
            c.manifest.is_some(),
            "the manifest is attached, so ids match the generated page"
        );
    }
}
