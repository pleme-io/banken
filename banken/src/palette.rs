//! palette — the fleet's colours, resolved once, for the places that need RGB.
//!
//! # The distinction this module is built on
//!
//! banken draws two kinds of colour and they want opposite treatment.
//!
//! **Named ANSI slots** — `Color::Green`, `Color::Cyan`, `Color::Red` — are
//! not hand-picked values. They are *indices into the operator's own terminal
//! theme*, so a green in banken is the same green as in their shell, their
//! editor and their `ls`. Converting those to fleet hexes would look like
//! convergence and be a regression: it would override a theme the operator
//! chose, in the one program that has no business doing so. **They stay slots,
//! and that is the convergence decision, not an omission from it.**
//!
//! **Interpolated colour** is different. [`crate::ronda`]'s access ramp needs a
//! shade *between* two rungs — that is the whole point of a ramp — and an ANSI
//! slot cannot express "40% of the way from warning to success". So the ramp
//! was five hardcoded RGB triples, and those genuinely were hand-picked: five
//! magic numbers no fleet edit could reach.
//!
//! This module is the join. The ramp's anchors now come from the fleet theme's
//! own error / warning / success colours, so one `ishou` edit moves banken's
//! ladder along with every other fleet surface — and nothing that could have
//! stayed an ANSI slot was converted to a hex to achieve it.
//!
//! # Three anchors, not five
//!
//! The old ramp pinned all five rungs. Three do the same job better: the ends
//! and the middle are the only ones with a *semantic* name in the token set
//! (error, warning, success), and the two intermediate rungs then land on
//! genuine interpolations rather than on separately-tuned constants. Fewer
//! magic numbers, one source, and the two stops that used to be guesses are now
//! derived.
//!
//! # The fallback is the old ramp, not black
//!
//! If a theme ever hands back a hex this cannot parse, [`Palette::for_theme`]
//! falls back to the previously-shipped anchors. Deliberately not to a default
//! colour: a `(0, 0, 0)` fallback is the black-cell failure `ronda::ramp`
//! already had once (NaN through `f32::clamp`), and a ramp that silently goes
//! black reads as a rendering fault rather than as a theme problem.

use std::sync::OnceLock;

use ishou_tokens::{FleetTheme, ResolvedTheme};

/// The previously-shipped anchors, kept as the parse-failure floor.
///
/// Not dead code and not nostalgia: a theme is data, and data can be wrong.
/// These are a known-good ramp that reads correctly, which is what a fallback
/// has to be.
const FLOOR: (Rgb, Rgb, Rgb) = ((198, 64, 72), (202, 172, 62), (92, 200, 112));

/// An 8-bit RGB triple, in the shape `egaku_term::Color::Rgb` takes.
pub type Rgb = (u8, u8, u8);

/// The fleet colours banken needs as RGB.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Palette {
    error: Rgb,
    warning: Rgb,
    success: Rgb,
    theme_name: String,
}

impl Palette {
    /// Resolve the palette for one fleet theme.
    #[must_use]
    pub fn for_theme(theme: FleetTheme) -> Self {
        let r: ResolvedTheme = theme.resolve();
        // ANSI slot order is xterm's, so 1/3/2 are red/yellow/green and the
        // bright variants are 9/11/10. The theme authors those slots for every
        // fleet theme, which is what makes this a projection of the token set
        // rather than a second palette living here.
        Self {
            error: hex_rgb(r.ansi_16.get(1).map(String::as_str)).unwrap_or(FLOOR.0),
            warning: hex_rgb(r.ansi_16.get(3).map(String::as_str)).unwrap_or(FLOOR.1),
            success: hex_rgb(r.ansi_16.get(2).map(String::as_str)).unwrap_or(FLOOR.2),
            theme_name: r.name,
        }
    }

    /// The fleet's prescribed theme, resolved **once** per process.
    ///
    /// A `OnceLock` rather than a call per use: [`crate::ronda::ramp`] runs
    /// inside a draw, and re-resolving a `ResolvedTheme` — which allocates
    /// eighteen `String`s — for every cell would put theme parsing on the
    /// render path.
    #[must_use]
    pub fn fleet() -> &'static Self {
        static FLEET: OnceLock<Palette> = OnceLock::new();
        FLEET.get_or_init(|| Self::for_theme(FleetTheme::default()))
    }

    /// The theme this palette resolved from — for a status line and for
    /// diagnosing a surprising colour.
    #[must_use]
    pub fn theme_name(&self) -> &str {
        &self.theme_name
    }

    /// Something is wrong.
    #[must_use]
    pub fn error(&self) -> Rgb {
        self.error
    }

    /// Something is part-way, or needs attention but is not broken.
    #[must_use]
    pub fn warning(&self) -> Rgb {
        self.warning
    }

    /// Something arrived.
    #[must_use]
    pub fn success(&self) -> Rgb {
        self.success
    }

    /// The access ramp's anchors, at their positions.
    ///
    /// Three, not five — see the module docs. The intermediate rungs
    /// (`network` at 0.25, `identity` at 0.75) are interpolations of these
    /// rather than separately-authored constants, which is what makes a fleet
    /// theme edit move the *whole* ladder rather than three-fifths of it.
    #[must_use]
    pub fn ramp_stops(&self) -> [(f32, Rgb); 3] {
        [
            (0.00, self.error),
            (0.50, self.warning),
            (1.00, self.success),
        ]
    }
}

impl Default for Palette {
    fn default() -> Self {
        Self::for_theme(FleetTheme::default())
    }
}

/// Parse `#RRGGBB` (or `RRGGBB`) into a triple.
///
/// `None` on anything else, and the caller falls back to [`FLOOR`] — never to
/// a default colour. A silently-black ramp reads as a rendering fault, which
/// sends the reader to the wrong place entirely.
fn hex_rgb(hex: Option<&str>) -> Option<Rgb> {
    let h = hex?.trim().trim_start_matches('#');
    if h.len() != 6 {
        return None;
    }
    Some((
        u8::from_str_radix(&h[0..2], 16).ok()?,
        u8::from_str_radix(&h[2..4], 16).ok()?,
        u8::from_str_radix(&h[4..6], 16).ok()?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **The convergence claim, asserted rather than described.** The ramp's
    /// ends must come from the theme — so changing the theme must change them.
    /// If every theme produced the same anchors, this module would be an
    /// elaborate way to keep hardcoding.
    #[test]
    fn the_anchors_actually_follow_the_theme() {
        let bare = Palette::for_theme(FleetTheme::Bare);
        let vellum = Palette::for_theme(FleetTheme::Vellum);
        assert_ne!(
            (bare.error(), bare.warning(), bare.success()),
            (vellum.error(), vellum.warning(), vellum.success()),
            "the anchors are not reading the theme — this is hardcoding with \
             extra steps",
        );
    }

    /// Every fleet theme must resolve to a usable ramp. A theme that produced
    /// the floor everywhere would mean its ANSI slots are unparseable, which is
    /// a real defect and would otherwise be invisible.
    #[test]
    fn every_fleet_theme_resolves_without_falling_back() {
        for theme in [
            FleetTheme::Bare,
            FleetTheme::PlemeDark,
            FleetTheme::Vellum,
            FleetTheme::PolarVeil,
        ] {
            let p = Palette::for_theme(theme);
            assert!(!p.theme_name().is_empty(), "{theme:?} has no name");
            // Not the floor — i.e. the hex parsed. All three matching FLOOR
            // exactly would be an astronomically unlikely coincidence and is
            // far more likely to mean the parse failed.
            assert_ne!(
                (p.error(), p.warning(), p.success()),
                FLOOR,
                "{theme:?} fell back to the floor — its ANSI hexes did not parse",
            );
        }
    }

    /// The ramp must still read red → green, whatever the theme. This is the
    /// one property an operator relies on without being taught it, and a theme
    /// is data that could violate it.
    #[test]
    fn every_theme_keeps_the_ramp_reading_red_to_green() {
        for theme in [
            FleetTheme::Bare,
            FleetTheme::PlemeDark,
            FleetTheme::Vellum,
            FleetTheme::PolarVeil,
        ] {
            let p = Palette::for_theme(theme);
            let (er, eg, _) = p.error();
            let (sr, sg, _) = p.success();
            assert!(er > eg, "{theme:?}: the bottom must be red-dominant");
            assert!(sg > sr, "{theme:?}: the top must be green-dominant");
        }
    }

    #[test]
    fn hex_parsing_is_total_and_refuses_rather_than_guessing() {
        assert_eq!(hex_rgb(Some("#C64048")), Some((198, 64, 72)));
        assert_eq!(hex_rgb(Some("C64048")), Some((198, 64, 72)));
        assert_eq!(hex_rgb(Some("  #C64048 ")), Some((198, 64, 72)));
        for bad in ["", "#", "#FFF", "#GGGGGG", "#C6404", "#C640488", "red"] {
            assert_eq!(hex_rgb(Some(bad)), None, "must refuse `{bad}`");
        }
        assert_eq!(hex_rgb(None), None);
    }

    /// The stops are ordered and span the whole ramp — `ronda::ramp` walks them
    /// pairwise and would silently misbehave on an unsorted or short span.
    #[test]
    fn the_stops_are_ordered_and_span_zero_to_one() {
        let s = Palette::fleet().ramp_stops();
        assert!((s[0].0 - 0.0).abs() < f32::EPSILON);
        assert!((s[2].0 - 1.0).abs() < f32::EPSILON);
        assert!(s[0].0 < s[1].0 && s[1].0 < s[2].0, "unordered: {s:?}");
    }

    /// Resolving is a `OnceLock`, so the render path pays for it once.
    #[test]
    fn the_fleet_palette_is_resolved_once() {
        assert!(std::ptr::eq(Palette::fleet(), Palette::fleet()));
    }
}
