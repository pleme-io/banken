//! The §III.a substrate-invariant guard — the CI-caught floor for
//! "an author re-adds an unwitnessed-mutate method to `ClusterEnv`".
//!
//! HONEST TIER (BANKEN.md §III.d): **only-mitigated → CI-caught.**
//! Nothing in Rust's type system forbids *extending* a trait, so the
//! re-add case cannot be truly-unrepresentable. This test is the
//! strengthening: it scans `src/env.rs`, extracts the method names
//! declared in the `ClusterEnv` trait, and asserts two things —
//!
//!  1. every declared method is on the sanctioned allowlist (a new
//!     method fails the build until an author consciously adds it), and
//!  2. no declared method name matches a mutating-verb denylist
//!     (`scale`/`delete`/`edit`/`apply`/`patch`/`kill`/`kubectl`/…) —
//!     so re-introducing a live-mutate method is CI-red, not silently
//!     merged.
//!
//! Do NOT read this as "imperative mutation is unrepresentable." It is
//! reviewer discipline promoted to a mechanical gate — the honest floor
//! for a class the type system cannot itself forbid.

use std::path::Path;

/// The whole allowlist of `ClusterEnv` method names. `declare` writes
/// git (a reconciler applies); `break_glass` is the one witnessed live
/// arm. Everything else is a read. There is no unwitnessed-mutate name.
const ALLOWED_METHODS: &[&str] = &[
    // OBSERVE — read-only afferent surface
    "list_resources",
    "get_resource",
    "logs",
    "events",
    "topology",
    "health_signals",
    "watch",
    // Re-reads ONE object at the moment of the act and returns a `Grip` only
    // if the uid still matches. A read — the narrowest one in the trait — and
    // it exists so a preview and an act cannot address different objects.
    "grip",
    // DECLARE — writes git, never the live cluster
    "declare",
    // BREAK-GLASS — the one witnessed live-effect arm
    "break_glass",
];

/// Mutating verbs that must NEVER appear as a `ClusterEnv` method name.
/// A method whose name contains any of these is an unwitnessed live
/// mutate — the exact leak the postigo primitive exists to forbid.
const FORBIDDEN_SUBSTRINGS: &[&str] = &[
    "scale", "delete", "edit", "apply", "patch", "kill", "kubectl", "mutate", "exec", "create",
    "replace", "remove", "restart", "drain", "cordon",
];

/// The whole allowlist of `SessionEnv` method names (the `(defbancada)`
/// seam). Exactly ONE of the four stages anything, and it demands a witness:
/// `stage_witnessed` takes a `MutatingCommand` **plus** a `WitnessedAction`.
/// A read pane is not staged at all — it is spawned as its own argv through
/// `PaneProgram::Observe`, whose `ObservedCommand` a mutating pane cannot
/// produce. A SECOND staging method would be an unwitnessed path for a
/// mutating command — the [`ClusterEnv`] re-add case, at the multiplexer seam.
///
/// This tightened on 2026-07-31: the seam used to carry an unwitnessed
/// `stage_observed` arm and lean on its argument type. The arm is gone.
const ALLOWED_SESSION_METHODS: &[&str] = &["open_session", "split", "stage_witnessed", "focus"];

/// Extract the method names declared in a trait body from the given source.
/// Cheap, dependency-free: find the trait, walk to its matching brace, and
/// collect `fn <name>` occurrences.
fn trait_methods(src: &str, trait_name: &str) -> Vec<String> {
    let mut needle = String::from("pub trait ");
    needle.push_str(trait_name);
    let Some(trait_pos) = src.find(&needle) else {
        panic!("could not find `{needle}` in the scanned source");
    };
    let after = &src[trait_pos..];
    let Some(open) = after.find('{') else {
        panic!("{trait_name} trait has no opening brace");
    };
    // Walk braces to find the trait body's matching close.
    let body_bytes = &after.as_bytes()[open..];
    let mut depth = 0i32;
    let mut end = None;
    for (i, &b) in body_bytes.iter().enumerate() {
        match b {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    end = Some(i);
                    break;
                }
            }
            _ => {}
        }
    }
    let end = end.unwrap_or_else(|| panic!("{trait_name} trait body never closes"));
    let body = &after[open..=open + end];

    // Collect `fn <ident>` — the declared methods. Comment lines
    // (`// *** ... ***`) never contain `fn ` followed by an ident+`(`.
    let mut names = Vec::new();
    let mut rest = body;
    while let Some(idx) = rest.find("fn ") {
        let tail = &rest[idx + 3..];
        let name: String = tail
            .chars()
            .take_while(|c| c.is_alphanumeric() || *c == '_')
            .collect();
        // A real method decl is `fn name(` (optionally with generics);
        // require the next non-name char to be `(` or `<` or whitespace
        // followed by `(`.
        let name_len = name.len();
        let after_name = tail[name_len..].trim_start();
        if !name.is_empty() && (after_name.starts_with('(') || after_name.starts_with('<')) {
            names.push(name);
        }
        rest = &tail[name_len..];
    }
    names
}

fn read_source(rel: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(rel);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()))
}

fn read_env_source() -> String {
    read_source("src/env.rs")
}

#[test]
fn cluster_env_exposes_only_allowlisted_methods() {
    let src = read_env_source();
    let methods = trait_methods(&src, "ClusterEnv");
    assert!(
        !methods.is_empty(),
        "parser found no ClusterEnv methods — the guard would be vacuous",
    );
    for m in &methods {
        assert!(
            ALLOWED_METHODS.contains(&m.as_str()),
            "ClusterEnv declares method `{m}` which is NOT on the sanctioned \
             allowlist. If it is a read/declare/break-glass method, add it to \
             ALLOWED_METHODS in this test. If it is a live mutate, it must NOT \
             exist — the postigo primitive forbids an unwitnessed-mutate method.",
        );
    }
    // Also assert every allowlisted method is actually present (so the
    // allowlist can't drift ahead of the trait either).
    for allowed in ALLOWED_METHODS {
        assert!(
            methods.iter().any(|m| m == allowed),
            "allowlisted method `{allowed}` is missing from the ClusterEnv trait",
        );
    }
}

#[test]
fn cluster_env_has_no_mutating_verb_method() {
    let src = read_env_source();
    let methods = trait_methods(&src, "ClusterEnv");
    for m in &methods {
        for forbidden in FORBIDDEN_SUBSTRINGS {
            assert!(
                !m.contains(forbidden),
                "ClusterEnv method `{m}` contains forbidden mutating verb \
                 `{forbidden}` — an unwitnessed live mutation is the exact \
                 leak the postigo primitive forbids (BANKEN.md §III.c/§III.d).",
            );
        }
    }
}

/// The same CI-caught floor at the `(defbancada)` seam.
///
/// `SessionEnv` has ONE staging arm and it requires a `MutatingCommand` + a
/// `WitnessedAction`; a read pane reaches a pane through `PaneProgram::Observe`
/// instead, carrying an `ObservedCommand`. Both newtypes are only constructible
/// from a `PlannedPane` of the matching `CommandEffect` — which is what makes
/// "stage a mutating command unwitnessed" truly-unrepresentable *within the
/// authored surface*. What the type system cannot forbid is an author ADDING a
/// second, permissive staging method. Same honest tier as the `ClusterEnv`
/// guard above: **only-mitigated → CI-caught.**
#[test]
fn session_env_exposes_only_allowlisted_methods() {
    let src = read_source("src/bancada.rs");
    let methods = trait_methods(&src, "SessionEnv");
    assert!(
        !methods.is_empty(),
        "parser found no SessionEnv methods — the guard would be vacuous",
    );
    for m in &methods {
        assert!(
            ALLOWED_SESSION_METHODS.contains(&m.as_str()),
            "SessionEnv declares method `{m}` which is NOT on the sanctioned \
             allowlist. If it stages a command, note that the WITNESS split is \
             structural: there is exactly ONE staging arm and it takes a \
             MutatingCommand + a WitnessedAction; a read pane is spawned via \
             PaneProgram::Observe, never staged. A second staging arm is an \
             unwitnessed path for a live effect.",
        );
    }
    for allowed in ALLOWED_SESSION_METHODS {
        assert!(
            methods.iter().any(|m| m == allowed),
            "allowlisted method `{allowed}` is missing from the SessionEnv trait",
        );
    }
    // And exactly ONE staging arm — the witnessed one.
    let staging: Vec<&String> = methods.iter().filter(|m| m.starts_with("stage")).collect();
    assert_eq!(
        staging.as_slice(),
        &[&"stage_witnessed".to_string()],
        "SessionEnv must have exactly one staging arm, and it must be the \
         witnessed one — an unwitnessed staging method is a live-effect path \
         with no witness on it",
    );
}
