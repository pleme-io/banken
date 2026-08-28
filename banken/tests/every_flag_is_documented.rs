//! Every flag the parser accepts must appear in `--help`.
//!
//! # The class this closes
//!
//! banken refuses an unknown flag rather than ignoring it, on the stated
//! grounds that "a dropped flag runs something other than what you asked for".
//! That care is wasted if the flag the operator *should* have typed is
//! undiscoverable: `--help` becomes a list of the flags someone remembered to
//! document, and the difference is invisible because both halves compile.
//!
//! Measured 2026-08-28: `--sole-context-of` shipped working, parsed, tested and
//! **absent from `--help`**. Nothing caught it — there was no test that related
//! the two surfaces, so the omission was found only by running `banken --help`
//! and grepping for a flag that was known to exist.
//!
//! # Why `include_str!` and not a runtime path read
//!
//! `include_str!` resolves relative to THIS SOURCE FILE at compile time.
//! `CARGO_MANIFEST_DIR` does not: under substrate's lockfile-builder it is the
//! WORKSPACE ROOT rather than the crate directory, so a path built from it
//! resolves one level too high and the test would fail only in the nix build —
//! the same layout split that broke engenho-kube-proto's build.rs the same day.

/// The argv parser, as source.
const CLI: &str = include_str!("../src/cli.rs");
/// The help printer, as source.
const MAIN: &str = include_str!("../src/main.rs");

/// Strip `//`-comment lines.
///
/// Load-bearing, not tidiness: both files DISCUSS the flags they handle in
/// prose, so an unstripped scan finds a flag "documented" because a comment
/// mentions it, or "parsed" because a doc comment names it. Either way the test
/// passes for the wrong reason.
fn code_only(src: &str) -> String {
    // Cut at the test module FIRST. Without it the scan reads the parser's own
    // unit tests, which deliberately contain misspelled flags to prove the
    // unknown-flag refusal fires — `--contxt` was duly reported as an
    // undocumented flag on the first run of this test. A source scan must
    // always stop before the tests that exercise the source.
    let src = src
        .split_once("#[cfg(test)]")
        .map_or(src, |(before, _)| before);
    src.lines()
        .filter(|l| !l.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Long-form flags the parser matches as whole literals.
///
/// Deliberately ignores the `--flag=value` prefix arms (`arg.starts_with(
/// "--context=")`): those are spellings of a flag already listed in its own
/// arm, so requiring a second help entry would demand documenting the same
/// option twice.
fn parsed_flags(cli: &str) -> Vec<String> {
    let code = code_only(cli);
    let mut out: Vec<String> = Vec::new();
    for raw in code.split('"').skip(1).step_by(2) {
        let is_flag = raw.starts_with("--")
            && raw.len() > 2
            && raw[2..].chars().all(|c| c.is_ascii_lowercase() || c == '-')
            && !raw.ends_with('=');
        if is_flag && !out.iter().any(|f| f == raw) {
            out.push(raw.to_string());
        }
    }
    out.sort();
    out
}

#[test]
fn every_parsed_flag_appears_in_help() {
    let flags = parsed_flags(CLI);

    // Anti-vacuity: an extractor that finds nothing would pass this test while
    // checking nothing at all, which is exactly the failure mode the test
    // exists to prevent one level up.
    assert!(
        flags.len() >= 4,
        "the flag extractor found only {flags:?} — it is broken, and a broken \
         extractor makes this test vacuously green"
    );
    for known in ["--context", "--fixture", "--help", "--sole-context-of"] {
        assert!(
            flags.iter().any(|f| f == known),
            "extractor missed the known flag {known} — found {flags:?}"
        );
    }

    let help = code_only(MAIN);
    let undocumented: Vec<&String> = flags.iter().filter(|f| !help.contains(*f)).collect();
    assert!(
        undocumented.is_empty(),
        "these flags parse but are absent from `--help`, so an operator cannot \
         discover them: {undocumented:?}\n\nbanken refuses unknown flags on the \
         grounds that a dropped flag runs the wrong thing — that only helps if \
         the right flag is findable."
    );
}
