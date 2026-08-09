//! The help page — **the authored vocabulary, rendered back to the operator**.
//!
//! # Why this is derived and not written
//!
//! A hand-written help screen is a second copy of the vocabulary, and it rots
//! in the one direction nobody notices: the app grows a chord, the help text
//! does not, and the feature is invisible forever. banken already refuses that
//! for the status-line legend ([`crate::Catalog`] drives it) and for
//! `--help`'s key block. This module is the same move made once, for
//! everything, so there is exactly one place that answers "what can banken
//! do".
//!
//! # How it stays current — three mechanisms, tiered honestly
//!
//! 1. **A new authored FORM appears with no code change.** Sections are built
//!    by iterating the [`Catalog`], so landing a `(defk8saction)` or a
//!    `(defbancada)` in a spec file puts it on the help page. This is a
//!    derivation, not a convention — **truly-unrepresentable** for a form of
//!    an existing domain to be missing.
//! 2. **A new authored DOMAIN is a compile error until it has a section.**
//!    [`HelpTopic`] is a `closed_catalog!` enum and [`HelpPage::build`]
//!    matches it exhaustively, so adding a domain without deciding how it
//!    reads is `error[E0004]`. Its axis is also on
//!    [`crate::catalog::REQUIRED_AXES`], so the catalog-reflection gate covers
//!    it in both directions.
//! 3. **A chord the app cannot actually dispatch is MARKED, never
//!    advertised.** The vocabulary declares more than any one build wires
//!    ([`crate::Catalog`] knows nothing about which screen is running), so the
//!    page takes a [`Wiring`] and labels the difference. **CI-caught**, by
//!    `every_authored_chord_is_on_the_help_page` — not structural, because
//!    nothing stops a caller passing an empty `Wiring`.
//!
//! # It is data, not a screen
//!
//! [`HelpPage`] is typed values with no terminal in sight, so the overlay
//! (`banken::help`), `--help` on stdout, and anything later — a `(defk8sview)`
//! help pane, a man page — render **the same value**. Two faces that build
//! their own text are two things that can disagree, which is precisely how
//! banken's `--help` came to advertise `S` for a chord the runtime bound as
//! `shift+s`.

use crate::Catalog;
use crate::closed_catalog;
use crate::nav::NavIntent;

closed_catalog! {
    /// One authored domain the help page reflects.
    ///
    /// **This enum is the "evolves well" mechanism.** Adding a domain to
    /// banken means adding a variant here, and [`HelpPage::build`]'s match is
    /// exhaustive — so the compiler refuses a vocabulary the operator cannot
    /// be told about. Ordered as an operator reads: what keys exist, what they
    /// cross, where they lead, and only then what banken knows.
    #[serde(rename_all = "kebab-case")]
    pub enum HelpTopic {
        /// `(defnavkey)` — moving around.
        Navigation => "navigation",
        /// `(defk8saction)` — the postigo actions and the gate each crosses.
        Actions => "actions",
        /// `(defbancada)` — pre-warmed troubleshooting sessions.
        Bancadas => "bancadas",
        /// `(defk8sview)` — the navigable screens.
        Views => "views",
        /// `(defdrill)` — where a view drills to.
        Drills => "drills",
        /// `(defpathology)` — the symptom→cause rules banken can apply.
        Pathologies => "pathologies",
        /// `(defward)` — the health landing.
        Wards => "wards",
    }
}

/// What the running app has actually WIRED, as opposed to what the vocabulary
/// declares.
///
/// The catalog is the whole authored surface; a given build dispatches a
/// subset of it. Without this the help page would advertise chords that do
/// nothing — the "dead key" class, which is worse than an undocumented
/// feature because the operator presses it and concludes banken is broken.
///
/// Supplied by the app rather than inferred here on purpose: which actions a
/// screen dispatches is app knowledge, and a guess made in this crate would be
/// a second, silently-drifting copy of it.
#[derive(Debug, Clone, Copy, Default)]
pub struct Wiring<'a> {
    /// Authored action names with no dispatch arm in the running app.
    pub unbound_actions: &'a [String],
    /// Authored bancada names not launchable from the current view.
    pub unbound_bancadas: &'a [String],
}

/// One line of the help page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HelpEntry {
    /// The chord that reaches it, when a keystroke does.
    ///
    /// `None` is a real answer, not a gap: a `(defpathology)` is something
    /// banken *knows*, not something you press.
    pub keys: Option<String>,
    /// The `postigo` class this entry crosses, when it crosses one.
    ///
    /// Present exactly where a gate is real. Navigation carries `None`
    /// because typing "move the cursor" as `OBSERVE` would make the class
    /// stop meaning "this performed a cluster read".
    pub gate: Option<&'static str>,
    /// What it is — the authored name.
    pub subject: String,
    /// What it does, or the state it is in.
    pub detail: String,
}

/// One section: a topic, its blurb, and its entries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HelpSection {
    /// Which authored domain this reflects.
    pub topic: HelpTopic,
    /// The operator-facing heading.
    pub title: &'static str,
    /// One line on what the domain is for.
    pub blurb: &'static str,
    /// The rows, in authored order.
    pub entries: Vec<HelpEntry>,
}

/// The whole help page — every authored domain, in reading order.
///
/// Fields are private and [`HelpPage::build`] is the only constructor, so a
/// page missing a topic cannot be held. Same shape as the four construction
/// seals in `banken-spec`, for the same reason: the invariant that makes the
/// value worth trusting is "it covers everything", and a public field list
/// would let a caller mint one that does not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HelpPage {
    sections: Vec<HelpSection>,
}

impl HelpPage {
    /// Build the page from the resolved catalog and what the app has wired.
    ///
    /// Every [`HelpTopic`] gets a section, in `ALL` order — so a topic cannot
    /// be silently skipped, only rendered empty (which
    /// `every_topic_has_entries` catches for the shipped vocabulary).
    #[must_use]
    pub fn build(catalog: &Catalog, wiring: Wiring<'_>) -> Self {
        let sections = HelpTopic::ALL
            .iter()
            .map(|topic| Self::section(*topic, catalog, wiring))
            .collect();
        Self { sections }
    }

    /// Every section, in reading order.
    #[must_use]
    pub fn sections(&self) -> &[HelpSection] {
        &self.sections
    }

    /// The section for one topic.
    ///
    /// # Panics
    ///
    /// Never in practice, and the `expect` says why: [`HelpPage::build`] is
    /// the only constructor and emits one section per [`HelpTopic::ALL`]
    /// entry, so the lookup is total by construction. Returning an `Option`
    /// would push a `None` arm onto every caller for a state the private
    /// field list makes unreachable.
    #[must_use]
    pub fn section_for(&self, topic: HelpTopic) -> &HelpSection {
        self.sections
            .iter()
            .find(|s| s.topic == topic)
            .expect("build() emits one section per HelpTopic::ALL")
    }

    /// **The exhaustive match that makes a new domain a compile error.**
    fn section(topic: HelpTopic, catalog: &Catalog, wiring: Wiring<'_>) -> HelpSection {
        match topic {
            HelpTopic::Navigation => HelpSection {
                topic,
                title: "NAVIGATION",
                blurb: "moving around — local UI only, no cluster read, no gate",
                entries: navigation_entries(catalog),
            },
            HelpTopic::Actions => HelpSection {
                topic,
                title: "ACTIONS",
                blurb: "every one is typed into a postigo class; there is no unwitnessed \
                        live-mutate path",
                entries: action_entries(catalog, wiring),
            },
            HelpTopic::Bancadas => HelpSection {
                topic,
                title: "BANCADAS",
                blurb: "pre-warmed troubleshooting sessions — the chord RESOLVES and \
                        previews, `confirm` opens",
                entries: bancada_entries(catalog, wiring),
            },
            HelpTopic::Views => HelpSection {
                topic,
                title: "VIEWS",
                blurb: "the navigable screens the vocabulary declares",
                entries: view_entries(catalog),
            },
            HelpTopic::Drills => HelpSection {
                topic,
                title: "DRILLS",
                blurb: "where a selected row leads",
                entries: drill_entries(catalog),
            },
            HelpTopic::Pathologies => HelpSection {
                topic,
                title: "PATHOLOGIES",
                blurb: "symptom→cause rules banken can apply — knowledge, not keystrokes",
                entries: pathology_entries(catalog),
            },
            HelpTopic::Wards => HelpSection {
                topic,
                title: "WARDS",
                blurb: "the health landing",
                entries: ward_entries(catalog),
            },
        }
    }

    /// The page as plain lines — what `--help` prints.
    ///
    /// The *same value* the overlay draws, so the two cannot drift. This is
    /// the whole reason the page is data: banken's `--help` once advertised
    /// `S` for a chord the runtime bound as `shift+s`, because the two were
    /// built separately.
    #[must_use]
    pub fn plain_lines(&self) -> Vec<String> {
        let mut out = Vec::new();
        for section in &self.sections {
            if section.entries.is_empty() {
                continue;
            }
            if !out.is_empty() {
                out.push(String::new());
            }
            let mut head = String::from(section.title);
            head.push_str("  —  ");
            head.push_str(section.blurb);
            out.push(head);
            for e in &section.entries {
                out.push(e.plain());
            }
        }
        out
    }
}

impl HelpEntry {
    /// One rendered line: `  <keys>  <GATE> — <subject>  <detail>`.
    ///
    /// Concatenation of typed pieces, never a `format!()` of a styled string
    /// (★★ TYPED EMISSION).
    #[must_use]
    pub fn plain(&self) -> String {
        let mut s = String::from("  ");
        let keys = self.keys.as_deref().unwrap_or("");
        s.push_str(keys);
        for _ in keys.chars().count()..16 {
            s.push(' ');
        }
        if let Some(gate) = self.gate {
            s.push_str(gate);
            s.push_str(" — ");
        }
        s.push_str(&self.subject);
        if !self.detail.is_empty() {
            s.push_str("  ");
            s.push_str(&self.detail);
        }
        s
    }
}

/// Navigation, grouped by intent so `down` and `j` read as one row.
fn navigation_entries(catalog: &Catalog) -> Vec<HelpEntry> {
    NavIntent::ALL
        .iter()
        .filter_map(|intent| {
            let chords: Vec<String> = catalog
                .nav_keys()
                .iter()
                .filter(|n| n.intent == *intent)
                .map(|n| n.keys.canonical())
                .collect();
            if chords.is_empty() {
                return None;
            }
            Some(HelpEntry {
                keys: Some(chords.join(" / ")),
                gate: None,
                subject: intent.label().to_owned(),
                detail: String::new(),
            })
        })
        .collect()
}

/// The postigo actions, each carrying the gate its keystroke crosses.
fn action_entries(catalog: &Catalog, wiring: Wiring<'_>) -> Vec<HelpEntry> {
    catalog
        .actions()
        .iter()
        .map(|a| HelpEntry {
            keys: Some(a.keys.canonical()),
            gate: Some(a.legality.class().label_upper()),
            subject: a.name.clone(),
            detail: if wiring.unbound_actions.contains(&a.name) {
                String::from("(declared, not wired in this view)")
            } else {
                String::new()
            },
        })
        .collect()
}

/// The bancadas. The class shown is the **derived** one — a recipe staging a
/// live effect cannot be advertised here as a convenience.
fn bancada_entries(catalog: &Catalog, wiring: Wiring<'_>) -> Vec<HelpEntry> {
    catalog
        .bancadas()
        .iter()
        .map(|g| {
            let mut detail = String::new();
            detail.push_str(&g.panes.len().to_string());
            detail.push_str(" panes, from :");
            detail.push_str(&g.from);
            if wiring.unbound_bancadas.contains(&g.name) {
                detail.push_str("  (launches from another view)");
            }
            HelpEntry {
                keys: Some(g.keys.canonical()),
                gate: Some(g.legality().map_or("INVALID", |l| l.class().label_upper())),
                subject: g.name.clone(),
                detail,
            }
        })
        .collect()
}

fn view_entries(catalog: &Catalog) -> Vec<HelpEntry> {
    catalog
        .views()
        .iter()
        .map(|v| {
            let mut detail = String::new();
            detail.push_str(&v.columns.len().to_string());
            detail.push_str(" columns");
            HelpEntry {
                keys: None,
                gate: None,
                subject: {
                    let mut s = String::from(":");
                    s.push_str(&v.name);
                    s
                },
                detail,
            }
        })
        .collect()
}

fn drill_entries(catalog: &Catalog) -> Vec<HelpEntry> {
    catalog
        .drills()
        .iter()
        .map(|d| {
            // The whole path, not just the destination: a drill is a route,
            // and an operator deciding whether to follow it wants the steps.
            let mut detail = String::from(":");
            detail.push_str(&d.from);
            for step in &d.steps {
                detail.push_str(" → ");
                detail.push_str(step.level.label());
            }
            HelpEntry {
                keys: None,
                gate: None,
                subject: d.name.clone(),
                detail,
            }
        })
        .collect()
}

fn pathology_entries(catalog: &Catalog) -> Vec<HelpEntry> {
    catalog
        .pathologies()
        .iter()
        .map(|p| HelpEntry {
            keys: None,
            gate: None,
            subject: p.name.clone(),
            detail: {
                let mut s = String::from(p.severity.label());
                s.push_str(" → ");
                s.push_str(p.remedy.kind().label());
                s
            },
        })
        .collect()
}

fn ward_entries(catalog: &Catalog) -> Vec<HelpEntry> {
    catalog
        .wards()
        .iter()
        .map(|w| HelpEntry {
            keys: None,
            gate: None,
            subject: w.name.clone(),
            detail: {
                let mut s = String::new();
                s.push_str(&w.lanes.len().to_string());
                s.push_str(" lanes");
                s
            },
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page() -> HelpPage {
        let catalog = crate::load_catalog().expect("the shipped vocabulary must resolve");
        HelpPage::build(&catalog, Wiring::default())
    }

    /// **THE GATE, and it is the whole point of the module.** Every chord the
    /// authored vocabulary declares — nav, postigo action, bancada — must
    /// appear on the help page. A chord an operator can press and cannot look
    /// up is an undiscoverable feature; a help page that documents a subset is
    /// the rot this module exists to prevent.
    #[test]
    fn every_authored_chord_is_on_the_help_page() {
        let catalog = crate::load_catalog().expect("resolves");
        let rendered = page().plain_lines().join("\n");

        let mut chords: Vec<String> = catalog
            .nav_keys()
            .iter()
            .map(|n| n.keys.canonical())
            .collect();
        chords.extend(catalog.actions().iter().map(|a| a.keys.canonical()));
        chords.extend(catalog.bancadas().iter().map(|g| g.keys.canonical()));
        assert!(chords.len() >= 14, "the probe must see a real vocabulary");

        for chord in chords {
            assert!(
                rendered.contains(&chord),
                "authored chord `{chord}` is not on the help page:\n{rendered}",
            );
        }
    }

    /// And every authored NAME, so a form can be reached for by name too.
    #[test]
    fn every_authored_form_is_on_the_help_page() {
        let catalog = crate::load_catalog().expect("resolves");
        let rendered = page().plain_lines().join("\n");
        let mut names: Vec<&str> = catalog.actions().iter().map(|a| a.name.as_str()).collect();
        names.extend(catalog.bancadas().iter().map(|g| g.name.as_str()));
        names.extend(catalog.pathologies().iter().map(|p| p.name.as_str()));
        names.extend(catalog.drills().iter().map(|d| d.name.as_str()));
        names.extend(catalog.wards().iter().map(|w| w.name.as_str()));
        for name in names {
            assert!(
                rendered.contains(name),
                "authored form `{name}` is undocumented:\n{rendered}",
            );
        }
    }

    /// A topic that renders empty is a section nobody sees — which would let a
    /// domain be "covered" by an entry-less heading. Every shipped domain has
    /// instances, so every topic must have rows.
    #[test]
    fn every_topic_has_entries() {
        let p = page();
        for topic in HelpTopic::ALL {
            assert!(
                !p.section_for(*topic).entries.is_empty(),
                "topic `{}` renders no entries — it would be a heading over nothing",
                topic.label(),
            );
        }
        assert_eq!(
            p.sections().len(),
            HelpTopic::ALL.len(),
            "one section per topic, no more and no fewer",
        );
    }

    /// Every ACTION entry names the gate it crosses. That column is the one
    /// thing distinguishing a read from a witnessed live effect, and a blank
    /// there is the most dangerous blank on the page.
    #[test]
    fn every_action_and_bancada_entry_names_its_gate() {
        let p = page();
        for topic in [HelpTopic::Actions, HelpTopic::Bancadas] {
            for e in &p.section_for(topic).entries {
                let gate = e.gate.unwrap_or("");
                assert!(
                    ["OBSERVE", "DECLARE", "BREAK-GLASS"].contains(&gate),
                    "`{}` shows gate {gate:?} — an action's class must be legible",
                    e.subject,
                );
            }
        }
    }

    /// Navigation carries NO gate: typing "move the cursor" as OBSERVE would
    /// make the class stop meaning "this performed a cluster read".
    #[test]
    fn navigation_entries_carry_no_gate() {
        for e in &page().section_for(HelpTopic::Navigation).entries {
            assert_eq!(
                e.gate, None,
                "`{}` must not claim a postigo class",
                e.subject
            );
        }
    }

    /// The wiring is what keeps the page from advertising a dead key.
    #[test]
    fn an_unwired_action_is_marked_rather_than_advertised() {
        let catalog = crate::load_catalog().expect("resolves");
        let unbound = vec![String::from("describe")];
        let p = HelpPage::build(
            &catalog,
            Wiring {
                unbound_actions: &unbound,
                unbound_bancadas: &[],
            },
        );
        let entry = p
            .section_for(HelpTopic::Actions)
            .entries
            .iter()
            .find(|e| e.subject == "describe")
            .expect("`describe` is an authored action");
        assert!(
            entry.detail.contains("not wired"),
            "an undispatchable chord must be marked: {:?}",
            entry.detail,
        );
        // …and it is still LISTED. Hiding it would make the vocabulary and the
        // page disagree about what exists.
        assert!(entry.keys.is_some());
    }

    /// The help key is itself authored, so the page documents the way into
    /// the page.
    #[test]
    fn the_help_chord_is_documented_on_the_help_page() {
        let rendered = page().plain_lines().join("\n");
        assert!(rendered.contains("help"), "{rendered}");
    }
}
