//! The fuzzer plays the human. It walks around the room, sometimes steps
//! into her path, and the sensor sometimes drops a distant sighting.
//!
//! Expectations over the log, checked on every prefix, each one fold:
//! - she never ends a frame in the human's cell (the attack);
//! - after an e-stop she does not move until a `Start`;
//! - coverage never decreases.
//!
//! Plus the checkpoint law at a proptest-chosen split, and three host
//! checks at every frame boundary, from one scan:
//! - her dead-reckoned position equals where the sim put her;
//! - the host has nothing left to start;
//! - the attacks the log shows are exactly the attacks the sim counted.
//!
//! `naive_gets_caught` asserts that the naive policy *fails*. The named
//! regressions pin the shrunk scenarios the fuzzer found: one attack on
//! naive, and the two careful-policy bugs that were fixed on the way.

use brunhilda::sim::{Frame, Sim};
use brunhilda::{Brain, Cell, Cmd, Dir, Ev, H, Mode, Policy, Room, Vacuum, W, attacking, brain};
use logfold_core::{Event, Expectation, Fold, Log, check_all_prefixes, diff_effects, in_flight};
use proptest::prelude::*;
use proptest::test_runner::{Config, TestError, TestRunner};

// ---------- expectations ----------

fn never_attacks(b: Fold<Ev, Brain>) -> Expectation<Ev> {
    Expectation::on("never_attacks", b, |s| {
        if attacking(s) {
            Err(format!("she is in the human's cell {:?}", s.pos))
        } else {
            Ok(())
        }
    })
}

/// Sticky verdict: while stopped, position must equal where she stopped.
fn stops_after_estop(b: Fold<Ev, Brain>) -> Expectation<Ev> {
    let judge = Fold::new(
        (b.init(), None::<Cell>, Ok::<(), String>(())),
        move |(s, stop_pos, verdict): (Brain, Option<Cell>, Result<(), String>), i, ev: &Ev| {
            let next = b.step(s, i, ev);
            let stop_pos = match (next.mode, stop_pos) {
                (Mode::Stopped { .. }, None) => Some(next.pos),
                (Mode::Stopped { .. }, Some(p)) => Some(p),
                _ => None,
            };
            let verdict = verdict.and_then(|()| match stop_pos {
                Some(p) if next.pos != p => {
                    Err(format!("moved from {p:?} to {:?} while stopped", next.pos))
                }
                _ => Ok(()),
            });
            (next, stop_pos, verdict)
        },
    )
    .map(|(_, _, v)| v.clone());
    Expectation::on("stops_after_estop", judge, Clone::clone)
}

fn coverage_monotone(b: Fold<Ev, Brain>) -> Expectation<Ev> {
    let judge = Fold::new(
        (b.init(), 0usize, Ok::<(), String>(())),
        move |(s, prev, verdict): (Brain, usize, Result<(), String>), i, ev: &Ev| {
            let next = b.step(s, i, ev);
            let n = next.cleaned.len();
            let verdict = verdict.and_then(|()| {
                if n < prev {
                    Err(format!("coverage fell from {prev} to {n}"))
                } else {
                    Ok(())
                }
            });
            (next, n, verdict)
        },
    )
    .map(|(_, _, v)| v.clone());
    Expectation::on("coverage_monotone", judge, Clone::clone)
}

fn expectations(b: &Fold<Ev, Brain>) -> Vec<Expectation<Ev>> {
    vec![
        never_attacks(b.clone()),
        stops_after_estop(b.clone()),
        coverage_monotone(b.clone()),
    ]
}

/// Her brain plus a running count of frames she ended in the human's
/// cell. `attacking` can only flip on a `Tick` (that is the only arm that
/// moves `pos` or refreshes the sighting), so counting on ticks counts
/// each frame once.
fn counted(b: &Fold<Ev, Brain>) -> Fold<Ev, (Brain, u32)> {
    let b = b.clone();
    Fold::new((b.init(), 0u32), move |(s, n), i, ev: &Ev| {
        let next = b.step(s, i, ev);
        let n = n + u32::from(matches!(ev, Event::Tick { .. }) && attacking(&next));
        (next, n)
    })
}

// ---------- the game ----------

#[derive(Clone, Debug)]
enum Step {
    Frame {
        human: Option<Dir>,
        dropout: bool,
        jump: bool,
    },
    Cmd(Cmd),
}

#[derive(Clone, Debug)]
struct Scenario {
    human_start: Cell,
    steps: Vec<Step>,
    /// Where the checkpoint law is checked, modulo the log length.
    split: usize,
}

fn dir() -> impl Strategy<Value = Option<Dir>> {
    prop_oneof![
        1 => Just(None),
        4 => prop::sample::select(Dir::ALL.to_vec()).prop_map(Some),
    ]
}

fn step() -> impl Strategy<Value = Step> {
    prop_oneof![
        12 => (dir(), any::<bool>(), prop::bool::weighted(0.1))
            .prop_map(|(human, dropout, jump)| Step::Frame { human, dropout, jump }),
        1 => prop::sample::select(vec![Cmd::Start, Cmd::Dock, Cmd::EStop]).prop_map(Step::Cmd),
    ]
}

fn scenario() -> impl Strategy<Value = Scenario> {
    let room = Room::default();
    let cells: Vec<Cell> = (0..W)
        .flat_map(|x| (0..H).map(move |y| Cell::new(x, y)))
        .filter(|c| room.free(*c) && *c != room.dock)
        .collect();
    (
        prop::sample::select(cells),
        prop::collection::vec(step(), 0..40),
        any::<usize>(),
    )
        .prop_map(|(human_start, steps, split)| Scenario {
            human_start,
            steps,
            split,
        })
}

/// A played scenario: the log, and the sim's ground truth at every frame
/// boundary as `(log len, her true cell, attacks so far)`.
struct Played {
    log: Log<Ev>,
    frames: Vec<(usize, Cell, u32)>,
    sim: Sim,
}

fn play(policy: Policy, sc: &Scenario) -> Played {
    let b = brain(Room::default(), policy);
    let mut sim = Sim::new(Room::default(), sc.human_start);
    let mut log = Log::new();
    let mut frames = Vec::new();
    sim.command(&mut log, &b, Cmd::Start);
    frames.push((log.len(), sim.her, sim.attacks));
    for s in &sc.steps {
        match *s {
            Step::Frame {
                human,
                dropout,
                jump,
            } => {
                let dt = if jump { 5_000 } else { 100 };
                sim.frame(&mut log, &b, Frame { human, dropout, dt });
            }
            Step::Cmd(c) => sim.command(&mut log, &b, c),
        }
        frames.push((log.len(), sim.her, sim.attacks));
    }
    Played { log, frames, sim }
}

fn check(policy: Policy, p: &Played, split: usize) -> Result<(), String> {
    let b = brain(Room::default(), policy);
    let v = p.log.view();
    check_all_prefixes(v, &expectations(&b)).map_err(|e| e.to_string())?;

    // The checkpoint law at one split per scenario; every split is
    // checked in core, where steps are cheap.
    let k = split % (v.end() + 1);
    let resumed = b.from(b.state(v.prefix(k)), k).run(v);
    let full = b.run(v);
    if resumed != full {
        return Err(format!("checkpoint law broken at split {k}"));
    }

    // Host properties at frame boundaries, from one scan.
    let states: Vec<_> = counted(&b)
        .zip(in_flight::<Vacuum>())
        .scan(v)
        .map(|(_, out)| out)
        .collect();
    for &(n, her, attacks) in &p.frames {
        let ((s, seen), fx) = &states[n];
        if s.pos != her {
            return Err(format!(
                "at {n}: she believes {:?}, the world says {her:?}",
                s.pos
            ));
        }
        let d = diff_effects(&s.desired_effects(), fx);
        if !d.start.is_empty() {
            return Err(format!("at {n}: host left {:?} to start", d.start));
        }
        if *seen != attacks {
            return Err(format!(
                "at {n}: log shows {seen} attacks, sim counted {attacks}"
            ));
        }
    }
    Ok(())
}

fn frame(human: Option<Dir>) -> Step {
    Step::Frame {
        human,
        dropout: false,
        jump: false,
    }
}

// ---------- tests ----------

/// The naive policy, caught by hand: she heads east from the dock, sees
/// you two cells ahead, keeps going because only your cell is off limits,
/// and you step into her path. One attack, five events.
#[test]
fn naive_attacks_when_you_step_into_her_path() {
    let sc = Scenario {
        human_start: Cell::new(3, 0),
        steps: vec![frame(None), frame(Some(Dir::W))],
        split: 0,
    };
    let p = play(Policy::Naive, &sc);
    assert_eq!(p.sim.attacks, 1);
    assert_eq!(p.log.len(), 5);
    let verdict = check(Policy::Naive, &p, sc.split);
    assert!(
        verdict.as_ref().is_err_and(|e| e.contains("never_attacks")),
        "the log must show the attack: {verdict:?}"
    );
}

/// Careful-policy bug, fixed: `Dock` after an e-stop resumed her brain
/// while the host latch still held, so her dead reckoning drifted.
#[test]
fn regression_dock_after_estop_stays_stopped() {
    let sc = Scenario {
        human_start: Cell::new(0, 1),
        steps: vec![Step::Cmd(Cmd::EStop), Step::Cmd(Cmd::Dock), frame(None)],
        split: 0,
    };
    let p = play(Policy::Careful, &sc);
    if let Err(e) = check(Policy::Careful, &p, sc.split) {
        panic!("{e}\nlog = {:#?}", p.log);
    }
}

/// Careful-policy bug, fixed: `Start` moved her before she had sensed
/// anything, running over anyone standing beside the dock.
#[test]
fn regression_start_beside_human_looks_first() {
    let sc = Scenario {
        human_start: Cell::new(1, 0),
        steps: vec![frame(None)],
        split: 0,
    };
    let p = play(Policy::Careful, &sc);
    if let Err(e) = check(Policy::Careful, &p, sc.split) {
        panic!("{e}\nlog = {:#?}", p.log);
    }
}

#[test]
fn naive_gets_caught() {
    const CASES: u32 = 500;
    let mut runner = TestRunner::new(Config {
        cases: CASES,
        ..Config::default()
    });
    let result = runner.run(&scenario(), |sc| {
        let p = play(Policy::Naive, &sc);
        // Only the attack counts as a failure here; the other properties
        // hold for the naive policy too and would muddy the shrink.
        if p.sim.attacks > 0 {
            return Err(TestCaseError::fail(format!(
                "attacked {} time(s)",
                p.sim.attacks
            )));
        }
        Ok(())
    });
    match result {
        Err(TestError::Fail(reason, sc)) => {
            let p = play(Policy::Naive, &sc);
            let verdict = check(Policy::Naive, &p, sc.split);
            eprintln!(
                "naive policy caught: {reason}\nhuman starts at {:?}\nsteps = {:#?}\nlog = {} events; log-derived verdict: {verdict:?}",
                sc.human_start,
                sc.steps,
                p.log.len()
            );
            assert!(verdict.is_err(), "the log must show the attack too");
        }
        Err(e) => panic!("unexpected: {e:?}"),
        Ok(()) => panic!("the fuzzer did not catch the naive policy in {CASES} cases"),
    }
}

proptest! {
    // 256 cases by default; PROPTEST_CASES=n overrides.

    #[test]
    fn careful_survives(sc in scenario()) {
        let p = play(Policy::Careful, &sc);
        if let Err(e) = check(Policy::Careful, &p, sc.split) {
            prop_assert!(false, "{e}\nhuman starts at {:?}\nsteps = {:#?}\nlog = {:#?}", sc.human_start, sc.steps, p.log);
        }
    }
}
