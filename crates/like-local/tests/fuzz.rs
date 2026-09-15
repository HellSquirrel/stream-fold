//! Random clicks and ticks. Two properties: liked exactly when the click
//! count is odd, and resuming from any checkpoint equals folding from zero.

use like_local::{Ev, click, like};
use logfold_core::{Event, Expectation, Fold, Log, check_all_prefixes, checkpoint_law};
use proptest::prelude::*;

/// Independent click counter, so the property is not checked against itself.
fn clicks() -> Fold<Ev, u32> {
    Fold::new(0, |n, _, ev| match ev {
        Event::Input { .. } => n + 1,
        _ => n,
    })
}

fn liked_iff_odd_clicks() -> Expectation<Ev> {
    Expectation::on("liked_iff_odd_clicks", like().zip(clicks()), |(v, n)| {
        if v.liked == (n % 2 == 1) {
            Ok(())
        } else {
            Err(format!("liked = {}, clicks = {n}", v.liked))
        }
    })
}

fn log() -> impl Strategy<Value = Log<Ev>> {
    prop::collection::vec(any::<bool>(), 0..32).prop_map(|steps| {
        let mut ms = 0;
        steps
            .into_iter()
            .map(|is_click| {
                if is_click {
                    click()
                } else {
                    ms += 16;
                    Ev::tick(ms)
                }
            })
            .collect()
    })
}

proptest! {
    #[test]
    fn like_holds(log in log()) {
        let v = log.view();
        if let Err(b) = check_all_prefixes(v, &[liked_iff_odd_clicks()]) {
            prop_assert!(false, "{b}\nlog = {log:#?}");
        }
        if let Err(e) = checkpoint_law(&like(), v) {
            prop_assert!(false, "{e}\nlog = {log:#?}");
        }
    }
}
