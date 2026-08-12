//! The gate that keeps a private estate out of a public crate.
//!
//! # Why this exists
//!
//! banken is going open source, and its source tree had accumulated the
//! operator's real estate: cluster names, an internal domain, a private-range
//! apiserver address, and one comment naming the organisation a context
//! belonged to. None of it was load-bearing — every one was a *fixture*, a
//! doc example, or an anecdote in a comment — but all of it would have shipped
//! in the published crate, where a reader could reconstruct a private
//! kubernetes estate from the test data.
//!
//! Scrubbing it once fixes the instances. This fixes the class: a name that
//! comes back turns the build red.
//!
//! # Why the denylist is HASHED
//!
//! A gate that says "the string `<real-cluster-name>` must never appear" has
//! to contain `<real-cluster-name>` — so a plaintext denylist in a public repo
//! leaks precisely the names it is defending, and does it in the one file
//! guaranteed to be read by anyone auditing what was scrubbed.
//!
//! So the forbidden tokens are stored as FNV-1a-64 digests. The gate hashes
//! every identifier-shaped token in the tree and compares. It can tell you
//! that a forbidden name is present and on which line; it cannot tell a reader
//! what the name is. That is the correct trade for a public repo: the operator
//! who trips it already knows what they typed.
//!
//! FNV-1a rather than a cryptographic hash because this is not a security
//! boundary — it is an obfuscation that keeps the *defence* from being the
//! *disclosure*, with no dependency and a fully deterministic result across
//! platforms and toolchains, which a `DefaultHasher` would not give.
//!
//! # What it cannot do
//!
//! This reads the WORKING TREE. It says nothing about git history, which is
//! where every one of these names still lives and which becomes readable the
//! moment the repository is made public. Scrubbing the tip is not scrubbing
//! the history — that needs a rewrite, and it is an operator decision.
//! `pending-banken: history-scrub`.

use std::path::{Path, PathBuf};

/// FNV-1a-64 digests of tokens that must not appear in the source tree.
///
/// Estate identifiers: cluster names, node names, an internal domain, and one
/// organisation name. Deliberately opaque — see the module docs.
const FORBIDDEN: [u64; 8] = [
    0x73bc_3dd2_623c_ab9e,
    0x0b41_9c4a_250d_4cc6,
    0xb106_8514_6c45_85c5,
    0x09e4_5bee_c648_e23e,
    0x8a11_c919_6116_26e1,
    0x77a5_7719_5657_b882,
    0xce5c_0d19_877b_dde9,
    0xf5fd_e719_0cfb_347f,
];

/// The digest of one lowercased token.
fn fnv1a(token: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in token.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// Every identifier-shaped token in `text`, lowercased, with its line number.
///
/// **Splits on hyphens and dots as well as punctuation**, so a compound name
/// yields its parts. That is not tidiness — it is the whole reach of the gate:
/// cluster names are overwhelmingly `<name>-<suffix>`, and a tokenizer that
/// kept the hyphen hashed the compound as one token, so a forbidden name
/// sitting inside the commonest shape it takes went straight through.
///
/// Caught by `the_gate_detects_a_forbidden_token`, which is why that test
/// embeds its token in a hyphenated identifier rather than standing it alone.
fn tokens(text: &str) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    for (i, line) in text.lines().enumerate() {
        for token in line.split(|c: char| !c.is_ascii_alphanumeric()) {
            if token.len() >= 3 {
                out.push((i + 1, token.to_ascii_lowercase()));
            }
        }
    }
    out
}

/// Source files under the workspace, excluding build output and this gate.
fn sources() -> Vec<PathBuf> {
    // `CARGO_MANIFEST_DIR` is `<workspace>/banken`; the workspace is its parent.
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the crate lives inside the workspace")
        .to_path_buf();
    let mut out = Vec::new();
    let mut stack = vec![root];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if path.is_dir() {
                // `target` is build output and `.git` is history — neither is
                // the working tree this gate speaks about.
                if name != "target" && name != ".git" && name != "result" {
                    stack.push(path);
                }
            } else if matches!(
                path.extension().and_then(|e| e.to_str()),
                Some("rs" | "lisp" | "toml" | "md")
            ) && name != "no_estate_identifiers.rs"
            {
                out.push(path);
            }
        }
    }
    out
}

/// **THE GATE.** No forbidden token anywhere in the working tree.
///
/// The failure names the file and line and says how many hits — never the
/// token itself, which would put it back in the repository via the assertion
/// message and, worse, via CI logs.
#[test]
fn no_estate_identifier_appears_in_the_source_tree() {
    let files = sources();
    assert!(
        files.len() > 20,
        "the walker found only {} files — it is not scanning the tree, so a \
         green result here would mean nothing",
        files.len(),
    );

    let mut hits = Vec::new();
    for path in &files {
        let Ok(text) = std::fs::read_to_string(path) else {
            continue;
        };
        for (line, token) in tokens(&text) {
            if FORBIDDEN.contains(&fnv1a(&token)) {
                hits.push(format!("{}:{line}", path.display()));
            }
        }
    }

    assert!(
        hits.is_empty(),
        "{} estate identifier(s) are back in the source tree, at:\n  {}\n\n\
         These are real cluster/node/domain/organisation names and this crate \
         is published. Replace them with fixture names — the NATO alphabet for \
         clusters, a reserved TLD (`.example`, `.invalid`, `.test`) for hosts, \
         and RFC 5737 documentation ranges (192.0.2.0/24) for addresses.",
        hits.len(),
        hits.join("\n  "),
    );
}

/// **A private address is an estate detail too** — it maps the operator's
/// network whether or not it names anything.
///
/// A pattern rather than a denylist, because the class is "an address from a
/// range that is somebody's actual network" and the instances are unbounded.
/// RFC 5737 documentation ranges are the sanctioned fixtures and are allowed.
#[test]
fn no_private_range_address_appears_in_the_source_tree() {
    let mut hits = Vec::new();
    for path in &sources() {
        let Ok(text) = std::fs::read_to_string(path) else {
            continue;
        };
        for (i, line) in text.lines().enumerate() {
            for token in line.split(|c: char| !(c.is_ascii_digit() || c == '.')) {
                let octets: Vec<&str> = token.split('.').collect();
                if octets.len() != 4 {
                    continue;
                }
                let parsed: Option<Vec<u8>> = octets.iter().map(|o| o.parse().ok()).collect();
                let Some(o) = parsed else { continue };
                let private = o[0] == 10
                    || (o[0] == 192 && o[1] == 168)
                    || (o[0] == 172 && (16..=31).contains(&o[1]));
                if private {
                    hits.push(format!("{}:{}  {token}", path.display(), i + 1));
                }
            }
        }
    }
    assert!(
        hits.is_empty(),
        "{} private-range address(es) in the source tree, at:\n  {}\n\n\
         Use RFC 5737 documentation ranges for fixtures: 192.0.2.0/24, \
         198.51.100.0/24, 203.0.113.0/24.",
        hits.len(),
        hits.join("\n  "),
    );
}

/// The gate has to be able to FAIL, or it is decoration.
///
/// Red-run in the test itself rather than recorded in a comment: a forbidden
/// token is synthesised, hashed, and checked against the same list the gate
/// uses, so a future change that emptied `FORBIDDEN` or broke `tokens` would
/// turn this red instead of leaving a green gate checking nothing.
#[test]
fn the_gate_detects_a_forbidden_token() {
    // Reconstructed at runtime so the literal is not in the file: the first
    // forbidden digest belongs to a token this assembles from parts.
    let token = String::from_utf8(vec![b'c', b'a', b'm', b'e', b'l', b'o', b't'])
        .expect("ascii is valid utf-8");
    assert!(
        FORBIDDEN.contains(&fnv1a(&token)),
        "the denylist no longer recognises a token it was built to catch",
    );

    let text = "let cluster = \"a-".to_owned() + &token + "-name\";";
    let found = tokens(&text)
        .into_iter()
        .any(|(_, t)| FORBIDDEN.contains(&fnv1a(&t)));
    assert!(
        found,
        "a forbidden token embedded in a hyphenated identifier slipped past \
         the tokenizer — which is exactly the shape a cluster name takes",
    );
}

/// And it must not fire on ordinary source, or it will be deleted the first
/// time it cries wolf.
#[test]
fn the_gate_is_quiet_on_ordinary_text() {
    let text = "// The alpha-eks fixture at https://k8s.example.invalid:6443 (192.0.2.3).";
    assert!(
        !tokens(text)
            .into_iter()
            .any(|(_, t)| FORBIDDEN.contains(&fnv1a(&t))),
        "the sanctioned fixture vocabulary must pass cleanly",
    );
}
