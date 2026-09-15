//! Fuzz harness for the like button.
//!
//! Two generators:
//! - `benign_log`: a well-behaved host and a world that answers `Done` in order.
//! - `adversarial_log`: proposal §4.8. Answers arrive out of order, fail or
//!   are cancelled mid-flight, arrive twice, arrive for requests that never
//!   existed, and the clock jumps. The host itself stays honest: it starts
//!   exactly what the fold desires, and nothing else.
//!
//! The test also plays the server. `server()` is a fold over the same log:
//! it learns each request's intent from `Started`, applies it on the first
//! `Done`, and ignores everything else. The agreement expectation says that
//! whenever the system is settled, client and server agree.
//!
//! Every expectation is a fold plus a predicate, so the all-prefix check is
//! one pass per expectation. The checkpoint law is checked once per log
//! over every split, and the host's own property at frame boundaries.

use std::collections::{BTreeMap, BTreeSet};

use like_button::{
    Click, Effect, Ev, LikeButton, POST, View, desired, desired_effects, events, in_scope,
    like_button, step,
};
use logfold_core::{
    Action, Event, Expectation, Fold, Index, IoResult, Log, LogView, ReqId, check_all_prefixes,
    checkpoint_law, diff_effects, in_flight,
};
use proptest::prelude::*;

// ---------- the server, as a fold ----------

/// The server's first answer per request is the truth; later answers for
/// the same request are redeliveries. (A world that says `Failed` and then
/// `Done` for one request is the timeout ambiguity, which needs a resync
/// effect and is not M0.)
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Server {
    liked: bool,
    intents: BTreeMap<ReqId, bool>,
    answered: BTreeSet<ReqId>,
}

fn server() -> Fold<'static, Ev, Server> {
    Fold::new(Server::default(), |mut s: Server, _, ev: &Ev| {
        match ev {
            Event::Started {
                effect: Effect::Post { req, intent, .. },
                ..
            } => {
                s.intents.insert(*req, *intent);
            }
            Event::Io { req, res, .. } => {
                if let Some(intent) = s.intents.get(req)
                    && s.answered.insert(*req)
                    && *res == IoResult::Done
                {
                    s.liked = *intent;
                }
            }
            _ => {}
        }
        s
    })
    .scoped(in_scope)
}

// ---------- expectations (each one fold, one pass) ----------

/// Desired effects are level-triggered: while a request is pending it is
/// desired exactly as started; when idle and dirty, one fresh `Post` for
/// the wanted value; otherwise nothing.
fn effects_match_pending() -> Expectation<'static, Ev> {
    Expectation::on("effects_match_pending", like_button(), |v| {
        let fx = desired_effects(v);
        let ok = match (&v.pending, v.dirty()) {
            (Some(p), _) => fx.len() == 1 && fx.contains(&p.effect),
            (None, true) => {
                fx.len() == 1
                    && matches!(fx.iter().next(), Some(Effect::Post { intent, .. }) if *intent == v.liked)
            }
            (None, false) => fx.is_empty(),
        };
        if ok {
            Ok(())
        } else {
            Err(format!("view = {v:?}, effects = {fx:?}"))
        }
    })
}

/// Everything the host has in flight is still desired. With the diff
/// model this is what "nothing to cancel" means at every prefix.
fn in_flight_is_desired() -> Expectation<'static, Ev> {
    Expectation::on(
        "in_flight_is_desired",
        like_button().zip(in_flight::<LikeButton>()),
        |(v, fx)| {
            let want = desired_effects(v);
            match fx.iter().find(|e| !want.contains(e)) {
                None => Ok(()),
                Some(e) => Err(format!("in flight but no longer desired: {e:?}")),
            }
        },
    )
}

/// The fold's idea of "in flight" agrees with the core's `in_flight` fold.
fn pending_matches_in_flight() -> Expectation<'static, Ev> {
    Expectation::on(
        "pending_matches_in_flight",
        like_button().zip(in_flight::<LikeButton>()),
        |(v, fx)| {
            let reqs: Vec<ReqId> = fx.iter().filter_map(|e| e.req()).collect();
            match (&v.pending, reqs.as_slice()) {
                (None, []) => Ok(()),
                (Some(p), [r]) if p.req() == *r => Ok(()),
                _ => Err(format!("pending = {:?}, in flight = {reqs:?}", v.pending)),
            }
        },
    )
}

/// The host never starts an effect the fold did not want at that moment.
/// A fold that carries the view *before* each event and a sticky verdict.
fn started_only_when_desired() -> Expectation<'static, Ev> {
    let judge = Fold::new(
        (View::default(), Ok::<(), String>(())),
        |(v, verdict), i, ev| {
            let verdict = verdict.and_then(|()| match ev {
                Event::Started { effect, .. } if !desired_effects(&v).contains(effect) => {
                    Err(format!("host started undesired {effect:?} at {i}"))
                }
                _ => Ok(()),
            });
            (step(v, i, ev), verdict)
        },
    )
    .scoped(in_scope)
    .map(|(_, verdict)| verdict.clone());
    Expectation::on("started_only_when_desired", judge, Clone::clone)
}

/// Whenever the system is settled (nothing in flight, nothing the fold
/// still wants started), client and server agree on `liked`.
fn client_server_agree() -> Expectation<'static, Ev> {
    let all = like_button()
        .zip(server())
        .zip(in_flight())
        .map(|((c, s), fx)| {
            (
                c.liked,
                s.liked,
                fx.is_empty() && desired_effects(c).is_empty(),
            )
        });
    Expectation::on("client_server_agree", all, |&(client, server, settled)| {
        if !settled || client == server {
            Ok(())
        } else {
            Err(format!(
                "settled but client.liked = {client}, server.liked = {server}"
            ))
        }
    })
}

fn expectations() -> Vec<Expectation<'static, Ev>> {
    vec![
        effects_match_pending(),
        in_flight_is_desired(),
        pending_matches_in_flight(),
        started_only_when_desired(),
        client_server_agree(),
    ]
}

// ---------- the host, and a property of it ----------

/// A generated log plus the indices at which each frame ended, i.e. the
/// host had acted and control returned to the user or the world.
#[derive(Clone, Debug)]
struct Played {
    log: Log<Ev>,
    frames: Vec<usize>,
}

impl Played {
    fn new() -> Self {
        Self {
            log: Log::new(),
            frames: Vec::new(),
        }
    }

    fn append(&mut self, ev: Ev) -> Index {
        self.log.append(ev)
    }

    /// The honest host: start exactly what the fold desires and is not in
    /// flight, then close the frame.
    fn host_acts(&mut self) {
        let v = self.log.view();
        let d = diff_effects(&desired().run(v), &in_flight::<LikeButton>().run(v));
        for fx in d.start {
            self.log.append(events::started(fx));
        }
        self.frames.push(self.log.len());
    }

    /// Every expectation on every prefix, the checkpoint law on every
    /// split, and the host property at every frame boundary.
    fn check(&self) -> Result<(), String> {
        let v = self.log.view();
        check_all_prefixes(v, &expectations()).map_err(|b| b.to_string())?;
        checkpoint_law(&like_button(), v)?;
        checkpoint_law(&server(), v)?;
        host_quiescent_at_frame_boundaries(self)
    }
}

/// At every frame boundary the host is quiescent: nothing left to start
/// and nothing to cancel. This is a property of the host, not of the log,
/// so it is checked at the boundaries the generator recorded rather than
/// on every prefix: a prefix ending on the world's answer is mid-frame.
fn host_quiescent_at_frame_boundaries(p: &Played) -> Result<(), String> {
    let (want, fx) = (desired(), in_flight::<LikeButton>());
    for &n in &p.frames {
        let log: LogView<'_, Ev> = p.log.prefix(n);
        let d = diff_effects(&want.run(log), &fx.run(log));
        if !d.start.is_empty() || !d.cancel.is_empty() {
            return Err(format!(
                "after frame ending at {n}: host left {:?} to start and {:?} to cancel",
                d.start, d.cancel
            ));
        }
    }
    Ok(())
}

// ---------- generators ----------

/// Benign world: after each click, optionally a tick, then `Done`.
fn benign_log() -> impl Strategy<Value = Played> {
    prop::collection::vec(any::<bool>(), 0..16).prop_map(|ticks| {
        let mut p = Played::new();
        let mut ms = 0;
        for tick_first in ticks {
            let req = p.append(events::click());
            p.host_acts();
            if tick_first {
                ms += 16;
                p.append(events::tick(ms));
                p.host_acts();
            }
            p.append(events::done(req));
            p.host_acts();
        }
        p
    })
}

/// One move by the world or the user in the adversarial game.
#[derive(Clone, Debug)]
enum Step {
    Click,
    /// Clock advance; `jump` selects a large step.
    Tick {
        jump: bool,
    },
    /// Answer the `pick`-th unanswered request (mod count) with `res`.
    Answer {
        pick: usize,
        res: Res,
    },
    /// Answer again a request that was already answered.
    Duplicate {
        pick: usize,
        res: Res,
    },
    /// Answer a request that never existed.
    Bogus {
        req: ReqId,
        res: Res,
    },
    /// A click on some other post: must be invisible to our fold.
    OtherPost,
}

#[derive(Clone, Copy, Debug)]
enum Res {
    Done,
    Failed,
    Cancelled,
}

impl Res {
    fn event(self, req: ReqId) -> Ev {
        let res = match self {
            Res::Done => IoResult::Done,
            Res::Failed => IoResult::Failed,
            Res::Cancelled => IoResult::Cancelled,
        };
        Event::Io {
            key: POST.to_string(),
            req,
            res,
        }
    }
}

fn res() -> impl Strategy<Value = Res> {
    prop_oneof![Just(Res::Done), Just(Res::Failed), Just(Res::Cancelled)]
}

fn step_strategy() -> impl Strategy<Value = Step> {
    prop_oneof![
        3 => Just(Step::Click),
        2 => any::<bool>().prop_map(|jump| Step::Tick { jump }),
        4 => (any::<usize>(), res()).prop_map(|(pick, res)| Step::Answer { pick, res }),
        1 => (any::<usize>(), res()).prop_map(|(pick, res)| Step::Duplicate { pick, res }),
        1 => (0u64..64, res()).prop_map(|(req, res)| Step::Bogus { req, res }),
        1 => Just(Step::OtherPost),
    ]
}

/// Play the steps against an honest host. Every step is one frame: the
/// event lands, the host acts, the frame closes.
fn play(steps: Vec<Step>) -> Played {
    let mut p = Played::new();
    let mut ms = 0u64;
    let mut unanswered: Vec<ReqId> = Vec::new();
    let mut answered: Vec<ReqId> = Vec::new();
    for s in steps {
        let before = p.log.len();
        match s {
            Step::Click => {
                p.append(events::click());
            }
            Step::Tick { jump } => {
                ms += if jump { 60_000 } else { 16 };
                p.append(events::tick(ms));
            }
            Step::Answer { pick, res } => {
                if unanswered.is_empty() {
                    continue;
                }
                let req = unanswered.remove(pick % unanswered.len());
                answered.push(req);
                p.append(res.event(req));
            }
            Step::Duplicate { pick, res } => {
                if answered.is_empty() {
                    continue;
                }
                let req = answered[pick % answered.len()];
                p.append(res.event(req));
            }
            Step::Bogus { req, res } => {
                p.append(res.event(req + 1_000));
            }
            Step::OtherPost => {
                p.append(Ev::input("post:7", Click));
            }
        }
        p.host_acts();
        for (_, e) in p.log.view().iter().skip(before) {
            if let Event::Started { effect, .. } = e
                && let Some(r) = effect.req()
            {
                unanswered.push(r);
            }
        }
    }
    p
}

fn adversarial_log() -> impl Strategy<Value = Played> {
    prop::collection::vec(step_strategy(), 0..24).prop_map(play)
}

// ---------- tests ----------

/// The steps the fuzzer shrank the original fold's failure to. Against
/// that fold they produced three concurrent requests answered out of
/// order; against this one the clicks that land mid-flight coalesce and
/// the same steps must be harmless. The original nine-event log itself is
/// not replayable, because its `Started` events were that host's output.
#[test]
fn regression_clicks_during_flight_coalesce() {
    let p = play(vec![
        Step::Click,
        Step::Click,
        Step::Answer {
            pick: 0,
            res: Res::Done,
        },
        Step::Click,
        Step::Answer {
            pick: 1,
            res: Res::Done,
        },
        Step::Answer {
            pick: 0,
            res: Res::Done,
        },
    ]);
    if let Err(e) = p.check() {
        panic!("{e}\nlog = {:#?}", p.log);
    }
}

proptest! {
    #[test]
    fn benign_sequences_hold(p in benign_log()) {
        if let Err(e) = p.check() {
            prop_assert!(false, "{e}\nlog = {:#?}", p.log);
        }
    }

    #[test]
    fn adversarial_sequences_hold(p in adversarial_log()) {
        if let Err(e) = p.check() {
            prop_assert!(false, "{e}\nlog = {:#?}", p.log);
        }
    }
}
