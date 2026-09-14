//! The fuzzer plays the human. It walks around the room, sometimes steps
//! into her path, and the sensor sometimes drops a distant sighting.
//!
//! Four expectations, all folds over the log:
//! - she never ends a frame in the human's cell (the attack);
//! - after an e-stop she does not move until a `Start`;
//! - coverage never decreases;
//! - the host has nothing left to start at any frame boundary.
//!
//! Plus two host properties at frame boundaries: her dead-reckoned
//! position equals where the sim put her, and the attacks the log shows
//! are exactly the attacks the sim counted.
//!
//! `naive_gets_caught` asserts that the naive policy *fails* and prints
//! the shrunk scenario. `careful_survives` is the real test.

use brunhilda::sim::{Frame, Sim};
use brunhilda::{Brain, Cell, Cmd, Dir, Ev, H, Mode, Policy, Room, Vacuum, W, attacking, brain};
use logfold_core::{
    Event, Expectation, Fold, Log, check_all_prefixes, checkpoint_law, diff_effects, in_flight,
};
use proptest::prelude::*;
use proptest::test_runner::{Config, TestError, TestRunner};

// ---------- expectations ----------

fn never_attacks(b: Fold<'static, Ev, Brain>) -> Expectation<'static, Ev> {
    Expectation::on("never_attacks", b, |s| {
        if attacking(s) {
            Err(format!("she is in the human's cell {:?}", s.pos))
        } else {
            Ok(())
        }
    })
}

/// Sticky verdict: while stopped, position must equal where she stopped.
fn stops_after_estop(b: Fold<'static, Ev, Brain>) -> Expectation<'static, Ev> {
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

fn coverage_monotone(b: Fold<'static, Ev, Brain>) -> Expectation<'static, Ev> {
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

fn expectations(b: &Fold<'static, Ev, Brain>) -> Vec<Expectation<'static, Ev>> {
    vec![
        never_attacks(b.clone()),
        stops_after_estop(b.clone()),
        coverage_monotone(b.clone()),
    ]
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
    )
        .prop_map(|(human_start, steps)| Scenario { human_start, steps })
}

/// A played scenario: the log, the sim's ground truth per frame, and the
/// frame boundaries.
struct Played {
    log: Log<Ev>,
    frames: Vec<(usize, Cell, u32)>, // (log len, her true cell, attacks so far)
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
                sim.frame(
                    &mut log,
                    &b,
                    Frame {
                        human,
                        dropout,
                        dt: if jump { 5_000 } else { 100 },
                    },
                );
            }
            Step::Cmd(c) => sim.command(&mut log, &b, c),
        }
        frames.push((log.len(), sim.her, sim.attacks));
    }
    Played { log, frames, sim }
}

fn check(policy: Policy, p: &Played) -> Result<(), String> {
    let b = brain(Room::default(), policy);
    let v = p.log.view();
    check_all_prefixes(v, &expectations(&b)).map_err(|e| e.to_string())?;
    checkpoint_law(&b, v)?;
    // host properties at frame boundaries
    let fx = in_flight::<Vacuum>();
    let mut attacks_seen = 0u32;
    let mut last_end = 0;
    for &(n, her, attacks) in &p.frames {
        let log = p.log.prefix(n);
        let s = b.run(log);
        if s.pos != her {
            return Err(format!(
                "at {n}: she believes {:?}, the world says {her:?}",
                s.pos
            ));
        }
        let d = diff_effects(&s.desired_effects(), &fx.run(log));
        if !d.start.is_empty() {
            return Err(format!("at {n}: host left {:?} to start", d.start));
        }
        // count attacks visible in the log between boundaries
        attacks_seen += b
            .scan(log)
            .filter(|(k, s)| {
                *k > last_end
                    && matches!(p.log.view().get(k - 1), Some(Event::Tick { .. }))
                    && attacking(s)
            })
            .count() as u32;
        last_end = n;
        if attacks_seen != attacks {
            return Err(format!(
                "at {n}: log shows {attacks_seen} attacks, sim counted {attacks}"
            ));
        }
    }
    Ok(())
}

// ---------- tests ----------

#[test]
fn naive_gets_caught() {
    let mut runner = TestRunner::new(Config {
        cases: 2000,
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
            eprintln!(
                "naive policy caught: {reason}\nhuman starts at {:?}\nsteps = {:#?}",
                sc.human_start, sc.steps
            );
            eprintln!(
                "log = {} events; the log-derived expectation agrees: {:?}",
                p.log.len(),
                check(Policy::Naive, &p).err()
            );
            assert!(
                check(Policy::Naive, &p).is_err(),
                "the log must show the attack too"
            );
        }
        Err(e) => panic!("unexpected: {e:?}"),
        Ok(()) => panic!("the fuzzer did not catch the naive policy in 2000 cases"),
    }
}

proptest! {
    // 256 cases by default; PROPTEST_CASES=n overrides.

    #[test]
    fn careful_survives(sc in scenario()) {
        let p = play(Policy::Careful, &sc);
        if let Err(e) = check(Policy::Careful, &p) {
            prop_assert!(false, "{e}\nhuman starts at {:?}\nsteps = {:#?}", sc.human_start, sc.steps);
        }
    }
}
