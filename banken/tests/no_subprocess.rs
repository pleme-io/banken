use std::path::{Path, PathBuf};

const SPAWN_WORDS: &[&str] = &[
    "process::Command",
    "Command::new(",
    "std::process::Child",
    "exec_capture(",
    "process_spawn(",
];

const EDITOR_SEAM: &str = "src/edit.rs";

fn sources(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir)
        .expect("readable source dir")
        .flatten()
    {
        let p = entry.path();
        if p.is_dir() {
            sources(&p, out);
        } else if p.extension().is_some_and(|e| e == "rs" || e == "b") {
            out.push(p);
        }
    }
}

fn spawning(root: &Path) -> Vec<String> {
    let mut files = Vec::new();
    sources(&root.join("src"), &mut files);
    let mut hits = Vec::new();
    for f in files {
        let rel = f.strip_prefix(root).unwrap_or(&f).display().to_string();
        let text = std::fs::read_to_string(&f).unwrap_or_default();
        for (n, line) in text.lines().enumerate() {
            if SPAWN_WORDS.iter().any(|w| line.contains(w)) {
                hits.push(format!("{rel}:{}", n + 1));
            }
        }
    }
    hits
}

#[test]
fn banken_spawns_no_process_outside_the_external_editor_seam() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let hits = spawning(&root);
    let outside: Vec<&String> = hits
        .iter()
        .filter(|h| !h.starts_with(EDITOR_SEAM))
        .collect();
    assert!(
        outside.is_empty(),
        "banken talks to every backend on the wire and spawns no process; found: {outside:?}"
    );
    let seam: Vec<&String> = hits.iter().filter(|h| h.starts_with(EDITOR_SEAM)).collect();
    assert_eq!(
        seam.len(),
        1,
        "the external editor is ONE direct argv spawn: {seam:?}"
    );
}

#[test]
fn the_gate_sees_a_spawn_when_one_is_there() {
    let dir = std::env::temp_dir().join(format!("banken-gate-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("src")).expect("scratch");
    std::fs::write(
        dir.join("src/sneak.rs"),
        "fn f() { let _ = std::process::Command::new(\"kubectl\"); }\n",
    )
    .expect("write");
    let hits = spawning(&dir);
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(hits, vec!["src/sneak.rs:1".to_owned()]);
}
