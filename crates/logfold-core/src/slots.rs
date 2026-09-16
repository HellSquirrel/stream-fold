//! The component's contract, declared once: targets, slots, their types,
//! inputs and constants. `slots!` expands to typed constants for Rust and a
//! [`Manifest`] from which the page's side is generated: `@property`
//! registrations and constants as CSS, and names and value tables the shim
//! uses to write `data-mode="cleaning"` rather than `data-mode="1"`.
//!
//! The generator writes only the contract, never appearance. The
//! stylesheet stays the designer's.
//!
//! ```ignore
//! logfold_core::slots! {
//!     pub mod slots;
//!     root {
//!         attr running: bool;                       // data-running, present or absent
//!         attr mode: enum { idle, cleaning, docking, stopped };   // data-mode="cleaning"
//!         var fps: int = 4;                         // --fps, @property <integer>, initial 4
//!         text status;                              // textContent of [data-text="status"]
//!     }
//!     family cell(96) { attr cleaned: bool; }       // targets cell-0 … cell-95
//!     family row { text title; }                    // unbounded: cloned from <template data-fold="row">
//!     inputs { run, pause }
//!     consts { room_w: 12.0, cell_px: 40.0 }
//! }
//! ```
//!
//! Names are the identifiers as written: `var her_x` is `--her_x`,
//! `attr mode` is `data-mode`. No case or dash rewriting, so what you
//! read in Rust is what you write in CSS.

use std::fmt::Write as _;

use crate::project::{Name, SlotKind};

/// What a slot's number means, so the shim can spell it for CSS.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SlotType {
    /// 1 or 0. As an attribute: present or absent.
    Bool,
    /// An index into the names. As an attribute: the name.
    Enum(&'static [&'static str]),
    /// An integer. As a variable: `@property` with `<integer>`.
    Int { initial: f64 },
    /// A number. As a variable: `@property` with `<number>`.
    Num { initial: f64 },
    /// The log index of the input event whose text to show.
    Text,
}

/// Which target a declared slot lives on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TargetDecl {
    Root,
    Named(Name),
    /// One slot per member of a family of targets `name-0`, `name-1`, …:
    /// `count` of them pre-rendered on the page, or unbounded, in which
    /// case the shim clones `<template data-fold="name">` on first use.
    Family {
        name: Name,
        count: Option<u32>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SlotDecl {
    pub target: TargetDecl,
    pub kind: SlotKind,
    pub name: Name,
    pub ty: SlotType,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Manifest {
    /// Slot declarations, one group per target (root, then each family).
    pub groups: &'static [&'static [SlotDecl]],
    pub inputs: &'static [Name],
    pub consts: &'static [(Name, f64)],
}

impl Manifest {
    /// Every declared slot, root first, then each family.
    pub fn slots(&self) -> impl Iterator<Item = &'static SlotDecl> + '_ {
        self.groups.iter().flat_map(|g| g.iter())
    }

    /// Every name the host will put in a patch, in id order: `root`, then
    /// each named target and family once, then each slot name once. A host
    /// interns these first so the shim can resolve ids from the manifest.
    pub fn names(&self) -> Vec<Name> {
        let mut names: Vec<Name> = vec!["root"];
        let mut push = |n: Name| {
            if !names.contains(&n) {
                names.push(n);
            }
        };
        for s in self.slots() {
            match s.target {
                TargetDecl::Root => {}
                TargetDecl::Named(n) | TargetDecl::Family { name: n, .. } => push(n),
            }
        }
        for s in self.slots() {
            push(s.name);
        }
        names
    }

    /// The CSS side of the contract: a typed `@property` per variable and
    /// the constants on `:root`. Appearance is not generated.
    pub fn css(&self) -> String {
        let mut out = String::from("/* generated from the component's slots!; do not edit */\n");
        for s in self.slots() {
            if s.kind != SlotKind::Var {
                continue;
            }
            let (syntax, initial) = match s.ty {
                SlotType::Bool | SlotType::Enum(_) => ("<integer>", 0.0),
                SlotType::Int { initial } => ("<integer>", initial),
                SlotType::Num { initial } => ("<number>", initial),
                SlotType::Text => continue,
            };
            let _ = writeln!(
                out,
                "@property {} {{ syntax: \"{syntax}\"; inherits: true; initial-value: {initial}; }}",
                s.name
            );
        }
        if !self.consts.is_empty() {
            out.push_str(":root {");
            for (n, v) in self.consts {
                let _ = write!(out, " --{n}: {v};");
            }
            out.push_str(" }\n");
        }
        out
    }

    /// The shim's side of the contract, as an ES module: names by id,
    /// families and their sizes (`null` when unbounded), inputs in dispatch
    /// order, and how each slot's number is spelled on the page.
    pub fn mjs(&self) -> String {
        let quote = |n: &str| format!("\"{}\"", n.replace('"', "\\\""));
        let list = |v: &[&str]| v.iter().map(|n| quote(n)).collect::<Vec<_>>().join(", ");
        let mut out = String::from(
            "// generated from the component's slots!; do not edit\nexport default {\n",
        );
        let _ = writeln!(out, "  names: [{}],", list(&self.names()));
        let _ = writeln!(out, "  inputs: [{}],", list(self.inputs));
        out.push_str("  consts: {");
        for (n, v) in self.consts {
            let _ = write!(out, " {n}: {v},");
        }
        out.push_str(" },\n  families: {");
        let mut seen_families: Vec<Name> = Vec::new();
        for s in self.slots() {
            if let TargetDecl::Family { name, count } = s.target
                && !seen_families.contains(&name)
            {
                seen_families.push(name);
                match count {
                    Some(count) => {
                        let _ = write!(out, " {}: {count},", quote(name));
                    }
                    None => {
                        let _ = write!(out, " {}: null,", quote(name));
                    }
                }
            }
        }
        out.push_str(" },\n  slots: {\n");
        let mut seen: Vec<(SlotKind, Name)> = Vec::new();
        for s in self.slots() {
            if seen.contains(&(s.kind, s.name)) {
                continue;
            }
            seen.push((s.kind, s.name));
            let kind = match s.kind {
                SlotKind::Var => "var",
                SlotKind::Attr => "attr",
                SlotKind::Text => "text",
            };
            let ty = match s.ty {
                SlotType::Bool => "bool: true".to_string(),
                SlotType::Enum(values) => format!("values: [{}]", list(values)),
                SlotType::Int { .. } | SlotType::Num { .. } => "number: true".to_string(),
                SlotType::Text => "text: true".to_string(),
            };
            let _ = writeln!(out, "    {}: {{ kind: \"{kind}\", {ty} }},", quote(s.name));
        }
        out.push_str("  },\n};\n");
        out
    }
}

/// Declare a component's contract. See the module docs for the grammar.
#[macro_export]
macro_rules! slots {
    (
        $vis:vis mod $m:ident;
        $( root { $($root:tt)* } )?
        $( family $fam:ident $( ($count:expr) )? { $($famslots:tt)* } )*
        $( inputs { $($input:ident),* $(,)? } )?
        $( consts { $($cname:ident : $cval:expr),* $(,)? } )?
    ) => {
        $vis mod $m {
            #![allow(non_upper_case_globals, dead_code, unused_imports)]
            use $crate::project::SlotKind;
            use $crate::slots::{Declared, Manifest, SlotDecl, SlotType, TargetDecl};

            $crate::slots!(@consts (TargetDecl::Root) $($($root)*)?);
            $crate::slots!(@decls ROOT_DECLS [] (TargetDecl::Root) $($($root)*)?);
            $(
                pub mod $fam {
                    #![allow(non_upper_case_globals, dead_code, unused_imports)]
                    use $crate::project::SlotKind;
                    use $crate::slots::{Declared, SlotDecl, SlotType, TargetDecl};
                    pub const NAME: &str = stringify!($fam);
                    /// How many members the page pre-renders; `None` grows from a template.
                    pub const COUNT: Option<u32> = $crate::slots!(@count $($count)?);
                    const TARGET: TargetDecl = TargetDecl::Family { name: stringify!($fam), count: COUNT };
                    $crate::slots!(@consts (TARGET) $($famslots)*);
                    $crate::slots!(@decls DECLS [] (TARGET) $($famslots)*);
                }
            )*

            pub const INPUTS: &[&str] = &[ $( $( stringify!($input) ),* )? ];
            pub const CONSTS: &[(&str, f64)] = &[ $( $( (stringify!($cname), $cval as f64) ),* )? ];

            pub const MANIFEST: Manifest = Manifest {
                groups: &[ ROOT_DECLS, $( $fam::DECLS, )* ],
                inputs: INPUTS,
                consts: CONSTS,
            };
        }
    };

    (@count) => { None };
    (@count $count:expr) => { Some($count as u32) };

    // ---- one `Declared` constant per slot, plus a module of values per enum ----
    (@consts ($t:expr) attr $n:ident : bool; $($rest:tt)*) => {
        pub const $n: Declared = Declared { target: $t, kind: SlotKind::Attr, name: concat!("data-", stringify!($n)) };
        $crate::slots!(@consts ($t) $($rest)*);
    };
    (@consts ($t:expr) attr $n:ident : enum { $($v:ident),* $(,)? }; $($rest:tt)*) => {
        pub const $n: Declared = Declared { target: $t, kind: SlotKind::Attr, name: concat!("data-", stringify!($n)) };
        $crate::enum_values!($n { $($v),* });
        $crate::slots!(@consts ($t) $($rest)*);
    };
    (@consts ($t:expr) attr $n:ident : int $(= $init:expr)?; $($rest:tt)*) => {
        pub const $n: Declared = Declared { target: $t, kind: SlotKind::Attr, name: concat!("data-", stringify!($n)) };
        $crate::slots!(@consts ($t) $($rest)*);
    };
    (@consts ($t:expr) var $n:ident : int $(= $init:expr)?; $($rest:tt)*) => {
        pub const $n: Declared = Declared { target: $t, kind: SlotKind::Var, name: concat!("--", stringify!($n)) };
        $crate::slots!(@consts ($t) $($rest)*);
    };
    (@consts ($t:expr) var $n:ident : num $(= $init:expr)?; $($rest:tt)*) => {
        pub const $n: Declared = Declared { target: $t, kind: SlotKind::Var, name: concat!("--", stringify!($n)) };
        $crate::slots!(@consts ($t) $($rest)*);
    };
    (@consts ($t:expr) text $n:ident; $($rest:tt)*) => {
        pub const $n: Declared = Declared { target: $t, kind: SlotKind::Text, name: stringify!($n) };
        $crate::slots!(@consts ($t) $($rest)*);
    };
    (@consts ($t:expr)) => {};

    // ---- the manifest group: a tt-muncher accumulating one array ----
    (@decls $name:ident [$($acc:tt)*] ($t:expr) attr $n:ident : bool; $($rest:tt)*) => {
        $crate::slots!(@decls $name [$($acc)* SlotDecl { target: $t, kind: SlotKind::Attr, name: concat!("data-", stringify!($n)), ty: SlotType::Bool },] ($t) $($rest)*);
    };
    (@decls $name:ident [$($acc:tt)*] ($t:expr) attr $n:ident : enum { $($v:ident),* $(,)? }; $($rest:tt)*) => {
        $crate::slots!(@decls $name [$($acc)* SlotDecl { target: $t, kind: SlotKind::Attr, name: concat!("data-", stringify!($n)), ty: SlotType::Enum(&[ $( stringify!($v) ),* ]) },] ($t) $($rest)*);
    };
    (@decls $name:ident [$($acc:tt)*] ($t:expr) attr $n:ident : int $(= $init:expr)?; $($rest:tt)*) => {
        $crate::slots!(@decls $name [$($acc)* SlotDecl { target: $t, kind: SlotKind::Attr, name: concat!("data-", stringify!($n)), ty: SlotType::Int { initial: 0.0 $(+ $init as f64)? } },] ($t) $($rest)*);
    };
    (@decls $name:ident [$($acc:tt)*] ($t:expr) var $n:ident : int $(= $init:expr)?; $($rest:tt)*) => {
        $crate::slots!(@decls $name [$($acc)* SlotDecl { target: $t, kind: SlotKind::Var, name: concat!("--", stringify!($n)), ty: SlotType::Int { initial: 0.0 $(+ $init as f64)? } },] ($t) $($rest)*);
    };
    (@decls $name:ident [$($acc:tt)*] ($t:expr) var $n:ident : num $(= $init:expr)?; $($rest:tt)*) => {
        $crate::slots!(@decls $name [$($acc)* SlotDecl { target: $t, kind: SlotKind::Var, name: concat!("--", stringify!($n)), ty: SlotType::Num { initial: 0.0 $(+ $init as f64)? } },] ($t) $($rest)*);
    };
    (@decls $name:ident [$($acc:tt)*] ($t:expr) text $n:ident; $($rest:tt)*) => {
        $crate::slots!(@decls $name [$($acc)* SlotDecl { target: $t, kind: SlotKind::Text, name: stringify!($n), ty: SlotType::Text },] ($t) $($rest)*);
    };
    (@decls $name:ident [$($acc:tt)*] ($t:expr)) => {
        pub const $name: &[SlotDecl] = &[ $($acc)* ];
    };
}

/// Enum values as constants in a module named after the slot:
/// `mode::cleaning == 1`.
#[doc(hidden)]
#[macro_export]
macro_rules! enum_values {
    ($n:ident { $($v:ident),* }) => {
        pub mod $n {
            #![allow(non_upper_case_globals, dead_code)]
            $crate::enum_values!(@count 0u8; $($v),*);
        }
    };
    (@count $i:expr; $v:ident $(, $rest:ident)*) => {
        pub const $v: u8 = $i;
        $crate::enum_values!(@count $i + 1; $($rest),*);
    };
    (@count $i:expr;) => {};
}

/// A declared slot: a [`Slot`](crate::project::Slot) for the root or a
/// named target, or a family that becomes a slot with [`Declared::at`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Declared {
    pub target: TargetDecl,
    pub kind: SlotKind,
    pub name: Name,
}

impl Declared {
    /// The slot for a root or named target. Panics for a family: use `at`.
    pub const fn slot(self) -> crate::project::Slot {
        let target = match self.target {
            TargetDecl::Root => crate::project::Target::Root,
            TargetDecl::Named(n) => crate::project::Target::Named(n),
            TargetDecl::Family { .. } => panic!("a family slot needs an index: use .at(i)"),
        };
        crate::project::Slot {
            target,
            kind: self.kind,
            name: self.name,
        }
    }

    /// The slot for member `i` of a family.
    pub const fn at(self, i: u32) -> crate::project::Slot {
        match self.target {
            TargetDecl::Family { name, .. } => crate::project::Slot {
                target: crate::project::Target::Indexed(name, i),
                kind: self.kind,
                name: self.name,
            },
            _ => panic!("not a family slot: use .slot()"),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::project::{Projection, Target};

    crate::slots! {
        pub mod s;
        root {
            attr running: bool;
            attr mode: enum { idle, cleaning, docking, stopped };
            var fps: int = 4;
            var t: num;
            text status;
        }
        family cell(6) { attr cleaned: bool; text label; }
        family row { text title; }
        inputs { run, pause }
        consts { w: 3.0, px: 40.0 }
    }

    #[test]
    fn constants_and_manifest_agree() {
        assert_eq!(
            s::running.slot(),
            crate::project::Slot::attr(Target::Root, "data-running")
        );
        assert_eq!(s::fps.slot().name, "--fps");
        assert_eq!(s::mode::cleaning, 1);
        assert_eq!(s::cell::cleaned.at(5).target, Target::Indexed("cell", 5));
        assert_eq!(s::INPUTS, ["run", "pause"]);
        assert_eq!(
            s::MANIFEST.names(),
            [
                "root",
                "cell",
                "row",
                "data-running",
                "data-mode",
                "--fps",
                "--t",
                "status",
                "data-cleaned",
                "label",
                "title"
            ]
        );
        assert_eq!(s::cell::COUNT, Some(6));
        assert_eq!(s::row::COUNT, None, "unbounded");
        assert_eq!(s::row::title.at(40).target, Target::Indexed("row", 40));
        assert_eq!(
            s::status.slot(),
            crate::project::Slot::text(Target::Root, "status")
        );
        assert_eq!(s::cell::label.at(1).kind, crate::project::SlotKind::Text);
        let p = Projection::new()
            .set(s::mode.slot(), s::mode::stopped)
            .set(s::cell::cleaned.at(2), 1);
        assert_eq!(p.get(s::mode.slot()), Some(3.0));
        assert_eq!(p.len(), 2);
    }

    #[test]
    fn css_and_mjs_are_the_contract_only() {
        let css = s::MANIFEST.css();
        assert!(css.contains(
            "@property --fps { syntax: \"<integer>\"; inherits: true; initial-value: 4; }"
        ));
        assert!(css.contains("@property --t { syntax: \"<number>\""));
        assert!(css.contains(":root { --w: 3; --px: 40; }"));
        assert!(!css.contains("data-"), "attributes need no registration");
        assert!(!css.contains("status"), "text slots need no registration");
        let mjs = s::MANIFEST.mjs();
        assert!(mjs.contains("\"status\": { kind: \"text\", text: true }"));
        assert!(mjs.contains("\"data-mode\": { kind: \"attr\", values: [\"idle\", \"cleaning\", \"docking\", \"stopped\"] }"));
        assert!(mjs.contains("\"data-running\": { kind: \"attr\", bool: true }"));
        assert!(mjs.contains("families: { \"cell\": 6, \"row\": null, }"));
        assert!(mjs.contains("inputs: [\"run\", \"pause\"]"));
    }
}
