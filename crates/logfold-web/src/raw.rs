//! The raw boundary: the same host behind plain `extern "C"` exports, no
//! wasm-bindgen. Everything that crosses is a number; names are bytes in
//! linear memory the shim decodes once per id; a patch is a pointer and a
//! length the shim reads as a `Float64Array` view, no copy.
//!
//! One instance per module: instantiate the module twice for two apps.
//! See `www/logfold-raw.mjs` for the other side.

use logfold_core::{Component, Domain};

use crate::Host;

pub struct Raw<D: Domain, X: Clone + 'static> {
    host: Host<D, X>,
    /// The last patch, kept alive until the next call so the shim can
    /// read it in place.
    patch: Vec<f64>,
    inputs: u32,
}

impl<D: Domain, X: Clone + 'static> Raw<D, X> {
    pub fn new(component: Component<D, X>) -> Self {
        let host = Host::new(component);
        let inputs = host.inputs().count() as u32;
        Self {
            host,
            patch: Vec::new(),
            inputs,
        }
    }

    pub fn input_count(&self) -> u32 {
        self.inputs
    }

    /// The name of input `i`, in dispatch order.
    pub fn input_name(&self, i: u32) -> Option<&'static str> {
        self.host.inputs().nth(i as usize)
    }

    /// The name behind a target or slot id in a patch.
    pub fn name(&self, id: u32) -> Option<&'static str> {
        self.host.name_str(id)
    }

    fn keep(&mut self, patch: Vec<f64>) -> u32 {
        self.patch = patch;
        self.patch.len() as u32
    }

    pub fn dispatch(&mut self, input: u32) -> u32 {
        let p = self.host.dispatch(input);
        self.keep(p)
    }

    pub fn tick(&mut self, ms: f64) -> u32 {
        let p = self.host.tick(ms);
        self.keep(p)
    }

    pub fn frame(&mut self, dt: f64) -> u32 {
        let p = self.host.frame(dt);
        self.keep(p)
    }

    pub fn render_at(&mut self, n: u32) -> u32 {
        let p = self.host.render_at(n);
        self.keep(p)
    }

    /// Where the last patch lives. Read it before the next call.
    pub fn patch_ptr(&self) -> *const f64 {
        self.patch.as_ptr()
    }

    pub fn at(&self) -> u32 {
        self.host.at()
    }

    pub fn len(&self) -> u32 {
        self.host.len()
    }

    pub fn is_empty(&self) -> bool {
        self.host.is_empty()
    }

    pub fn checkpoint_for(&self, n: u32) -> i32 {
        self.host.checkpoint_for(n)
    }

    pub fn kind(&self, i: u32) -> i32 {
        self.host.kind_code(i)
    }

    pub fn tick_ms(&self, i: u32) -> f64 {
        self.host.tick_ms(i)
    }
}

/// Export a component through the raw boundary: `lf_*` functions with
/// number arguments and results, one instance per module.
#[macro_export]
macro_rules! export_raw {
    ($domain:ty, $state:ty, $component:expr) => {
        mod logfold_raw_exports {
            use std::cell::RefCell;

            type App = $crate::raw::Raw<$domain, $state>;

            thread_local! {
                static APP: RefCell<Option<App>> = const { RefCell::new(None) };
            }

            fn with<R>(f: impl FnOnce(&mut App) -> R) -> R {
                APP.with(|slot| {
                    let mut slot = slot.borrow_mut();
                    let app = slot.get_or_insert_with(|| App::new($component));
                    f(app)
                })
            }

            #[unsafe(no_mangle)]
            pub extern "C" fn lf_init() {
                with(|_| ());
            }
            #[unsafe(no_mangle)]
            pub extern "C" fn lf_input_count() -> u32 {
                with(|a| a.input_count())
            }
            #[unsafe(no_mangle)]
            pub extern "C" fn lf_input_ptr(i: u32) -> *const u8 {
                with(|a| a.input_name(i).map_or(std::ptr::null(), str::as_ptr))
            }
            #[unsafe(no_mangle)]
            pub extern "C" fn lf_input_len(i: u32) -> u32 {
                with(|a| a.input_name(i).map_or(0, |s| s.len() as u32))
            }
            #[unsafe(no_mangle)]
            pub extern "C" fn lf_name_ptr(id: u32) -> *const u8 {
                with(|a| a.name(id).map_or(std::ptr::null(), str::as_ptr))
            }
            #[unsafe(no_mangle)]
            pub extern "C" fn lf_name_len(id: u32) -> u32 {
                with(|a| a.name(id).map_or(0, |s| s.len() as u32))
            }
            #[unsafe(no_mangle)]
            pub extern "C" fn lf_dispatch(input: u32) -> u32 {
                with(|a| a.dispatch(input))
            }
            #[unsafe(no_mangle)]
            pub extern "C" fn lf_tick(ms: f64) -> u32 {
                with(|a| a.tick(ms))
            }
            #[unsafe(no_mangle)]
            pub extern "C" fn lf_frame(dt: f64) -> u32 {
                with(|a| a.frame(dt))
            }
            #[unsafe(no_mangle)]
            pub extern "C" fn lf_render_at(n: u32) -> u32 {
                with(|a| a.render_at(n))
            }
            #[unsafe(no_mangle)]
            pub extern "C" fn lf_patch_ptr() -> *const f64 {
                with(|a| a.patch_ptr())
            }
            #[unsafe(no_mangle)]
            pub extern "C" fn lf_at() -> u32 {
                with(|a| a.at())
            }
            #[unsafe(no_mangle)]
            pub extern "C" fn lf_len() -> u32 {
                with(|a| a.len())
            }
            #[unsafe(no_mangle)]
            pub extern "C" fn lf_checkpoint_for(n: u32) -> i32 {
                with(|a| a.checkpoint_for(n))
            }
            #[unsafe(no_mangle)]
            pub extern "C" fn lf_kind(i: u32) -> i32 {
                with(|a| a.kind(i))
            }
            #[unsafe(no_mangle)]
            pub extern "C" fn lf_tick_ms(i: u32) -> f64 {
                with(|a| a.tick_ms(i))
            }
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_raw_wrapper_keeps_the_patch_and_names_inputs_first() {
        let mut r = Raw::new(like_local::component());
        assert_eq!(r.input_count(), 1);
        assert_eq!(r.input_name(0), Some("toggle"));
        assert_eq!(r.name(0), Some("root"), "the manifest's names come first");
        let n = r.dispatch(0);
        assert_eq!(n, 5, "one slot changed: five numbers");
        let patch = unsafe { std::slice::from_raw_parts(r.patch_ptr(), n as usize) };
        assert_eq!(patch[4], 1.0, "data-liked = 1");
        assert_eq!(patch[1], -1.0, "not a family member");
        assert_eq!(r.name(patch[0] as u32), Some("root"));
        assert_eq!(r.name(patch[3] as u32), Some("data-liked"));
        assert_eq!(r.render_at(0), 5);
        assert!(r.kind(0) == 0 && r.kind(9) == -1);
    }
}
