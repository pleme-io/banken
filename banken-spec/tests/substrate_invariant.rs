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

/// Extract the method names declared in the `ClusterEnv` trait body from
/// the given source. Cheap, dependency-free: find the trait, walk to its
/// matching brace, and collect `fn <name>` occurrences.
fn cluster_env_methods(src: &str) -> Vec<String> {
    let Some(trait_pos) = src.find("pub trait ClusterEnv") else {
        panic!("could not find `pub trait ClusterEnv` in src/env.rs");
    };
    let after = &src[trait_pos..];
    let Some(open) = after.find('{') else {
        panic!("ClusterEnv trait has no opening brace");
    };
    // Walk braces to find the trait body's matching close.
    let body_bytes = after[open..].as_bytes();
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
    let end = end.expect("ClusterEnv trait body never closes");
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

fn read_env_source() -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/env.rs");
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()))
}

#[test]
fn cluster_env_exposes_only_allowlisted_methods() {
    let src = read_env_source();
    let methods = cluster_env_methods(&src);
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
    let methods = cluster_env_methods(&src);
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
