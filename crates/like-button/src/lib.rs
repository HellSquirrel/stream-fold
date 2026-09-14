//! M0 example: a like button. Click -> optimistic toggle -> `Post` -> `Done`/`Failed`.
//!
//! # History
//!
//! The first version of this fold was the obvious one: every click toggled
//! `liked` and became its own request; a `Failed` for the latest request
//! toggled back. The adversarial fuzzer (`tests/fuzz.rs`) broke it with a
//! nine-event log: three clicks while earlier requests were still in
//! flight, answered out of order, so the server's final state was the
//! intent of whichever request it happened to answer last. That was the
//! M0 exit criterion, and it is now a regression test.
//!
//! # This version
//!
//! Three facts: what the user *wants*, what the server has *confirmed*,
//! and the one request *in flight*. A request is desired only when wanted
//! differs from confirmed and nothing is in flight, so clicks coalesce and
//! at most one request exists at a time. The world's first answer for the
//! in-flight request is authoritative; every other `Io` is ignored. On a
//! failure the user's wish is dropped back to the confirmed value.
//!
//! The fold reads `Started` events, which lets it know a request is in
//! flight without the host telling it, and gives the UI a "sending" state.

use std::collections::BTreeSet;

use logfold_core::{Effect, Event, Fold, IdemKey, Index, IoResult, Key, Pure, ReqId, UiEvent};

/// The one post this example cares about.
pub const POST: &str = "post:42";

pub fn key() -> Key {
    POST.to_string()
}

/// What the UI renders. A plain value; rendering it is the host's job.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct View {
    /// What the user wants; shown optimistically.
    pub liked: bool,
    /// What the server last confirmed.
    pub confirmed: bool,
    /// Index of the click that last changed `liked`. Becomes the request id.
    pub wanted_since: Index,
    /// The request currently awaiting the world's first answer, if any.
    pub pending: Option<Pending>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pending {
    pub req: ReqId,
    /// The value we asked the server to store.
    pub intent: bool,
}

impl View {
    /// The user wants something the server has not confirmed.
    pub fn dirty(&self) -> bool {
        self.liked != self.confirmed
    }
}

/// Is this event addressed to our post? The fold's declared scope.
pub fn in_scope(ev: &Event) -> bool {
    match ev {
        Event::Pure(Pure::Ui { key, .. }) | Event::Io { key, .. } | Event::Started { key, .. } => {
            key == POST
        }
        Event::Pure(Pure::Tick { .. }) => false,
    }
}

/// One step of the like button. Public so expectations can look at the
/// state *before* an event without re-running the fold.
pub fn step(mut v: View, index: Index, ev: &Event) -> View {
    match ev {
        Event::Pure(Pure::Ui {
            ev: UiEvent::Click, ..
        }) => {
            v.liked = !v.liked;
            v.wanted_since = index;
        }
        Event::Started {
            effect: Effect::Post { req, intent, .. },
            ..
        } => {
            v.pending = Some(Pending {
                req: *req,
                intent: *intent,
            });
        }
        Event::Io { req, res, .. } => {
            if let Some(p) = v.pending.take_if(|p| p.req == *req) {
                match res {
                    IoResult::Done => v.confirmed = p.intent,
                    IoResult::Failed | IoResult::Cancelled => v.liked = v.confirmed,
                }
            }
            // otherwise: duplicate, bogus, or stale answer; ignored
        }
        _ => {}
    }
    v
}

/// The like button as a fold, scoped to its post.
pub fn like_button() -> Fold<'static, View> {
    Fold::new(View::default(), step).scoped(in_scope)
}

/// The set of effects that should be in flight for a view. At most one:
/// the wanted value, once nothing else is in flight.
pub fn desired_effects(v: &View) -> BTreeSet<Effect> {
    let mut out = BTreeSet::new();
    if v.dirty() && v.pending.is_none() {
        out.insert(Effect::Post {
            req: v.wanted_since,
            key: key(),
            intent: v.liked,
            idem: IdemKey {
                scope: key(),
                index: v.wanted_since,
            },
        });
    }
    out
}

/// Derived projection: desired effects as a fold.
pub fn desired() -> Fold<'static, View, BTreeSet<Effect>> {
    like_button().map(desired_effects)
}

/// Convenience constructors for tests and hosts.
pub mod events {
    use super::*;

    pub fn click() -> Event {
        Event::click(key())
    }
    /// Host bookkeeping: `effect` was started for `req`.
    pub fn started(effect: Effect) -> Event {
        let req = effect.req().expect("like-button effects expect a result");
        Event::Started {
            key: key(),
            req,
            effect,
        }
    }
    pub fn done(req: ReqId) -> Event {
        Event::Io {
            key: key(),
            req,
            res: IoResult::Done,
        }
    }
    pub fn failed(req: ReqId) -> Event {
        Event::Io {
            key: key(),
            req,
            res: IoResult::Failed,
        }
    }
    pub fn tick(ms: u64) -> Event {
        Event::tick(ms)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use logfold_core::{Log, checkpoint_law};

    /// An honest host: start whatever is desired right now.
    fn host(log: &mut Log) {
        for fx in desired().run(log.view()) {
            log.append(events::started(fx));
        }
    }

    #[test]
    fn empty_log_is_not_liked_and_idle() {
        let log = Log::new();
        assert_eq!(like_button().run(log.view()), View::default());
        assert!(desired().run(log.view()).is_empty());
    }

    #[test]
    fn click_then_done_settles() {
        let mut log = Log::new();
        log.append(events::click());
        host(&mut log);
        log.append(events::done(0));
        let v = like_button().run(log.view());
        assert!(v.liked && v.confirmed && !v.dirty());
        assert_eq!(v.pending, None);
    }

    #[test]
    fn click_then_failed_rolls_back() {
        let mut log = Log::new();
        log.append(events::click());
        host(&mut log);
        log.append(events::failed(0));
        let v = like_button().run(log.view());
        assert!(!v.liked && !v.confirmed);
        assert_eq!(v.pending, None);
    }

    #[test]
    fn clicks_coalesce_while_in_flight() {
        let mut log = Log::new();
        log.append(events::click()); // want true
        host(&mut log); // req 0 in flight
        log.append(events::click()); // want false
        log.append(events::click()); // want true again
        host(&mut log); // nothing to start: in flight
        assert_eq!(log.len(), 4, "no second request while one is in flight");
        log.append(events::done(0));
        let v = like_button().run(log.view());
        assert!(v.liked && v.confirmed && !v.dirty());
        assert!(desired().run(log.view()).is_empty());
    }

    #[test]
    fn other_posts_are_out_of_scope() {
        let log: Log = [Event::click("post:7"), events::click()]
            .into_iter()
            .collect();
        let v = like_button().run(log.view());
        assert!(v.liked, "only our click counted");
        assert_eq!(v.wanted_since, 1);
    }

    #[test]
    fn checkpoint_law_holds_mid_flight() {
        let mut log = Log::new();
        log.append(events::click());
        host(&mut log);
        log.append(events::tick(16));
        log.append(events::done(0));
        checkpoint_law(&like_button(), log.view()).unwrap();
        checkpoint_law(&desired(), log.view()).unwrap();
    }
}
