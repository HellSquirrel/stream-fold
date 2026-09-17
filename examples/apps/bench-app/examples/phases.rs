//! Where the Rust side spends its time per event, natively, in release:
//! `cargo run -q -p bench-app --example phases --release`.

use std::time::Instant;

use logfold_core::{Projection, diff};
use logfold_web::Host;

fn ms(t: Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1e3
}

fn main() {
    for n in [1_000u32, 10_000, 100_000] {
        let c = bench::component();
        let names: Vec<_> = c.input_names().collect();
        let id = |name: &str| names.iter().position(|x| *x == name).unwrap() as u32;
        let mut h = Host::new(bench::component());

        // one create, then a steady-state select
        let t = Instant::now();
        let patch = h.dispatch(id("create"), -1, n.to_string().as_bytes());
        let create = ms(t);
        let writes = patch.len() / 5;
        let t = Instant::now();
        let _ = h.dispatch(id("select"), 5, b"");
        let select = ms(t);
        let t = Instant::now();
        let _ = h.dispatch(id("select"), 6, b"");
        let _ = h.dispatch(id("select"), 7, b"");
        let _ = h.dispatch(id("select"), 8, b"");
        let select3 = ms(t) / 3.0;
        let t = Instant::now();
        let _ = h.dispatch(id("swap"), -1, b"");
        let swap = ms(t);
        let t = Instant::now();
        let _ = h.render_at(h.len());
        let rerender = ms(t);
        println!(
            "{:>9}  host: select again {select3:.2} ms each   swap {swap:.2} ms   render_at(head) {rerender:.2} ms",
            ""
        );
        // the derivative alone, for a select, on settled states
        let make = c.text_event(id("create") as usize, &n.to_string()).unwrap();
        let mut s0 = c.fold.step_one(bench::State::default(), 0, &make);
        s0.rows.take_changes();
        let pick = c.index_event(id("select") as usize, 9).unwrap();
        let mut s1 = c.fold.step_one(s0.clone(), 1, &pick);
        let t = Instant::now();
        let d = c.derive(&s0, &mut s1, &pick);
        let derive_only = ms(t);
        let t = Instant::now();
        let s2 = s0.clone();
        let clone_only = ms(t);
        drop(s2);
        println!(
            "{:>9}  derive(select) alone {derive_only:.2} ms [{} changes]   state clone {clone_only:.2} ms",
            "",
            d.map_or(0, |d| d.len())
        );

        // the phases, in isolation, on the same state
        let ev = c.text_event(id("create") as usize, &n.to_string()).unwrap();
        let t = Instant::now();
        let state = bench::step(bench::State::default(), 0, &ev);
        let step = ms(t);
        let t = Instant::now();
        let cloned = state.clone();
        let clone = ms(t);
        drop(cloned);
        let t = Instant::now();
        let p = c.render(&state);
        let project = ms(t);
        let t = Instant::now();
        let p = p.finish();
        let finish = ms(t);
        let t = Instant::now();
        let changes = diff(&Projection::new(), &p);
        let diff_full = ms(t);
        let t = Instant::now();
        let same = diff(&p, &p);
        let diff_same = ms(t);
        let t = Instant::now();
        let q = p.clone();
        let proj_clone = ms(t);
        drop(q);
        println!(
            "N={n:>7}  host: create {create:7.2} ms ({writes} writes)   select {select:7.2} ms\n\
             {:>9}  step {step:.2}   state clone {clone:.2}   render {project:.2} (+ finish {finish:.2})   \
             diff empty→p {diff_full:.2} [{} changes]   diff p→p {diff_same:.2} [{} changes]   projection clone {proj_clone:.2}",
            "",
            changes.len(),
            same.len(),
        );
    }
}
