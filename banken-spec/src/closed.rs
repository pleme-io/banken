//! [`closed_catalog!`] — the one emitter for banken's closed catalog
//! enums (★★ EMITTER SUBSTRATE + ★★ CATALOG REFLECTION).
//!
//! # Why this macro exists
//!
//! Every closed axis of the banken border needs the same three things: the
//! variants, an `ALL` const enumerating them, and a stable `label()` for
//! catalog rows / telemetry. Hand-writing that triple is a *three-way*
//! duplication per enum, and the failure mode is silent: add a variant,
//! forget the `ALL` row, and the catalog under-reports forever. `types.rs`
//! guards that today with an `every_*_variant_in_all` test per axis — a
//! **CI-caught** floor.
//!
//! This macro raises the tier for the enums it emits. The variant list and
//! the label list are **the same list**, so:
//!
//! - a variant missing from `ALL` is **truly-unrepresentable** (there is no
//!   second list to forget), and
//! - a variant with no `label()` arm is a **compile error** (the generated
//!   `match` is exhaustive over the same list).
//!
//! Do not read that as "catalog reflection is now proven fleet-wide": it is
//! proven **for the axes emitted through this macro**. See the backfill note
//! below for the four that are not.
//!
//! # `pending-banken: closed-catalog-macro-backfill`
//!
//! The four pre-existing axes in [`crate::types`] (`LegalityClass`,
//! `DeclareTargetKind`, `ViewKind`, `ResourceKind`) are still hand-written
//! and still guarded by their `every_*_variant_in_all` tests. Three are
//! mechanically portable; **`ViewKind` is the real obstacle** — its serde
//! wire form is `PascalCase` (`ResourceTable`), which is what the shipped
//! `specs/views.lisp` authors, while its `label()` is kebab-case
//! (`resource-table`). This macro deliberately ties `label()` to the
//! *label* and lets the caller pick the serde repr, so porting `ViewKind`
//! is possible but changes nothing and buys nothing until the authored
//! `:kind` values are re-spelled. Left as named debt rather than churned.
//!
//! # Every new axis SHOULD go through here
//!
//! A hand-written `ALL`/`label()` pair in a new module is the duplication
//! this macro exists to delete.

/// Emit a closed catalog enum: the variants, `ALL`, and `label()`.
///
/// The caller supplies the outer attributes (docs + the serde repr) and one
/// `Variant => "label"` pair per arm. The label is the stable catalog /
/// telemetry name; where the serde repr is `kebab-case` the label and the
/// wire name coincide, which `catalog::tests::labels_are_the_wire_names`
/// asserts for every axis emitted here.
///
/// ```ignore
/// closed_catalog! {
///     /// How badly a pathology bites.
///     #[serde(rename_all = "kebab-case")]
///     pub enum Severity {
///         /// Breaks the workload.
///         Critical => "critical",
///         /// Degrades it.
///         Warning => "warning",
///     }
/// }
/// ```
#[macro_export]
macro_rules! closed_catalog {
    (
        $(#[$meta:meta])*
        $vis:vis enum $name:ident {
            $(
                $(#[$vmeta:meta])*
                $variant:ident => $label:literal
            ),+ $(,)?
        }
    ) => {
        // The derive MUST be emitted before the caller's attributes: a
        // `#[serde(...)]` helper attribute used *before* the `#[derive]`
        // that introduces it is `derive helper attribute is used before it
        // is introduced` — an error since Rust 2021 (rust-lang#79202). Doc
        // comments are order-insensitive, so putting the caller's block
        // after the derive costs nothing.
        #[derive(
            ::serde::Serialize,
            ::serde::Deserialize,
            Debug,
            Clone,
            Copy,
            PartialEq,
            Eq,
            Hash,
        )]
        $(#[$meta])*
        $vis enum $name {
            $(
                $(#[$vmeta])*
                $variant,
            )+
        }

        impl $name {
            /// Every variant of this axis — the catalog-reflection surface.
            ///
            /// Generated from the same list the variants come from, so a
            /// variant missing here is unrepresentable rather than
            /// test-caught.
            pub const ALL: &'static [$name] = &[ $( $name::$variant ),+ ];

            /// The stable catalog / telemetry label for this variant.
            #[must_use]
            pub fn label(self) -> &'static str {
                match self {
                    $( $name::$variant => $label, )+
                }
            }
        }
    };
}

#[cfg(test)]
mod tests {
    // A throwaway axis, emitted by the macro, used to prove the three
    // generated properties without depending on any real axis's shape.
    closed_catalog! {
        /// A test-only axis.
        #[serde(rename_all = "kebab-case")]
        pub enum Probe {
            /// First arm.
            FirstArm => "first-arm",
            /// Second arm.
            SecondArm => "second-arm",
        }
    }

    #[test]
    fn all_enumerates_every_variant_because_it_is_the_same_list() {
        assert_eq!(Probe::ALL, &[Probe::FirstArm, Probe::SecondArm]);
    }

    #[test]
    fn label_is_total_over_all() {
        let labels: Vec<&str> = Probe::ALL.iter().map(|p| p.label()).collect();
        assert_eq!(labels, vec!["first-arm", "second-arm"]);
    }

    /// With a `kebab-case` serde repr the label IS the wire name — the
    /// property `catalog::tests::labels_are_the_wire_names` relies on.
    #[test]
    fn kebab_repr_makes_the_label_the_wire_name() {
        for p in Probe::ALL {
            let wire = serde_json::to_string(p).expect("serializes");
            let mut expected = String::from("\"");
            expected.push_str(p.label());
            expected.push('"');
            assert_eq!(wire, expected);
        }
    }
}
