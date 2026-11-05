use banken_spec::env::{ClusterEnv, GlassAct, Posture};
use banken_spec::interp::{Outcome, Selection, apply_with};
use banken_spec::types::OperatorId;

use crate::action::{ActionResult, RowAction};
use crate::app::EditTarget;

pub const MARK: &str = "# banken: ";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EditorChoice {
    #[default]
    Embedded,
    External,
}

pub struct Unbuilt;

impl Operator for Unbuilt {
    fn edit(&mut self, _text: &str) -> Result<Edited, String> {
        Err(
            "the embedded editor (an escriba pane) is not built yet, and banken spawns no \
             editor unless asked: set config(:edit, {editor: :external}) in banken.b to use \
             $VISUAL / $EDITOR"
                .into(),
        )
    }

    fn confirm(&mut self, _question: &str, _diff: &str) -> bool {
        false
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Edited {
    Unchanged,
    Text(String),
}

pub trait Operator {
    /// # Errors
    ///
    /// The editor could not run or the file could not be read back.
    fn edit(&mut self, text: &str) -> Result<Edited, String>;
    fn confirm(&mut self, question: &str, diff: &str) -> bool;
}

#[must_use]
pub fn with_header(error: &str, body: &str) -> String {
    let mut s = String::new();
    for line in error.lines() {
        s.push_str(MARK);
        s.push_str(line);
        s.push('\n');
    }
    s.push_str(MARK);
    s.push_str("fix the manifest and save, or save it unchanged to give up\n");
    s.push_str(body);
    s
}

#[must_use]
pub fn without_header(text: &str) -> String {
    text.lines()
        .filter(|l| !l.starts_with(MARK))
        .map(|l| {
            let mut s = l.to_owned();
            s.push('\n');
            s
        })
        .collect()
}

#[must_use]
pub fn diff(current: &str, proposed: &str) -> String {
    similar::TextDiff::from_lines(current, proposed)
        .unified_diff()
        .context_radius(3)
        .header("live", "after apply (server dry run)")
        .to_string()
}

fn selection(t: &EditTarget) -> Selection {
    Selection {
        kind: t.kind,
        name: t.name.clone(),
        namespace: t.namespace.clone(),
        current: Vec::new(),
    }
}

fn place(t: &EditTarget) -> String {
    let mut s = String::from(t.kind.label());
    s.push(' ');
    if let Some(ns) = &t.namespace {
        s.push_str(ns);
        s.push('/');
    }
    s.push_str(&t.name);
    s.push_str(" on ");
    s.push_str(&t.cluster);
    s
}

pub fn run<E: ClusterEnv>(
    env: &E,
    operator: &OperatorId,
    target: &EditTarget,
    hand: &mut dyn Operator,
) -> ActionResult {
    if env.posture() == Posture::ReadOnly {
        let mut m = String::from("refused: ");
        m.push_str(&place(target));
        m.push_str(" is read-only, so banken will not edit it");
        return ActionResult::Error(m);
    }
    let ns = target.namespace.as_deref();
    let live = match env.manifest(target.kind, &target.name, ns) {
        Ok(m) => m,
        Err(e) => return ActionResult::Error(e.to_string()),
    };
    let mut buffer = live.clone();
    loop {
        let edited = match hand.edit(&buffer) {
            Ok(Edited::Text(t)) => without_header(&t),
            Ok(Edited::Unchanged) => return ActionResult::Error("edit: nothing changed".into()),
            Err(e) => return ActionResult::Error(e),
        };
        if edited == live {
            return ActionResult::Error("edit: nothing changed".into());
        }
        let proposed = match env.preview_change(target.kind, &target.name, ns, &edited) {
            Ok(p) => p,
            Err(e) => {
                buffer = with_header(&e.to_string(), &edited);
                continue;
            }
        };
        let shown = diff(&live, &proposed);
        let mut question = String::from("apply this to ");
        question.push_str(&place(target));
        question.push_str("? (BREAK-GLASS, witnessed)");
        if !hand.confirm(&question, &shown) {
            return ActionResult::Error("edit: not applied".into());
        }
        let act = GlassAct::Apply {
            kind: target.kind,
            name: target.name.clone(),
            namespace: target.namespace.clone(),
            manifest: edited.clone(),
        };
        match apply_with(
            &RowAction::Edit.spec(operator),
            &selection(target),
            env,
            act,
        ) {
            Ok(Outcome::GlassLogged(record)) => {
                return ActionResult::BreakGlassRecord {
                    witness: record.action.witness.as_str().to_owned(),
                    runbook: record.action.runbook.0,
                    selector: record.action.selector,
                    record_id: record.record_id,
                };
            }
            Ok(_) => return ActionResult::Error("edit: the gate did not witness it".into()),
            Err(e) => buffer = with_header(&e.to_string(), &edited),
        }
    }
}

pub struct Terminal;

impl Operator for Terminal {
    fn edit(&mut self, text: &str) -> Result<Edited, String> {
        let path = std::env::temp_dir().join(format!("banken-edit-{}.yaml", std::process::id()));
        std::fs::write(&path, text).map_err(|e| e.to_string())?;
        let editor = std::env::var("VISUAL")
            .or_else(|_| std::env::var("EDITOR"))
            .unwrap_or_else(|_| "vi".to_owned());
        let mut words = editor.split_whitespace();
        let program = words.next().unwrap_or("vi").to_owned();
        let status = std::process::Command::new(&program)
            .args(words)
            .arg(&path)
            .status()
            .map_err(|e| format!("{program}: {e}"))?;
        let out = std::fs::read_to_string(&path).map_err(|e| e.to_string());
        let _ = std::fs::remove_file(&path);
        if !status.success() {
            return Err(format!("{program} exited with {status}"));
        }
        let out = out?;
        Ok(if out == text {
            Edited::Unchanged
        } else {
            Edited::Text(out)
        })
    }

    fn confirm(&mut self, question: &str, diff: &str) -> bool {
        use std::io::{BufRead, Write};
        let mut out = std::io::stdout();
        let _ = writeln!(out, "\n{diff}\n{question} [y/N] ");
        let _ = out.flush();
        let mut line = String::new();
        let _ = std::io::stdin().lock().read_line(&mut line);
        matches!(line.trim(), "y" | "Y" | "yes")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixture::FixtureClusterEnv;
    use banken_spec::types::ResourceKind;

    struct Scripted {
        edits: Vec<Result<Edited, String>>,
        seen: Vec<String>,
        answer: bool,
        diffs: Vec<String>,
    }

    impl Operator for Scripted {
        fn edit(&mut self, text: &str) -> Result<Edited, String> {
            self.seen.push(text.to_owned());
            self.edits.remove(0)
        }

        fn confirm(&mut self, _question: &str, diff: &str) -> bool {
            self.diffs.push(diff.to_owned());
            self.answer
        }
    }

    fn target() -> EditTarget {
        EditTarget {
            cluster: "east".into(),
            kind: ResourceKind::Pod,
            namespace: Some("catch".into()),
            name: "catch-api-7d9f".into(),
        }
    }

    fn op() -> OperatorId {
        OperatorId::new("drzzln").expect("non-blank")
    }

    #[test]
    fn a_confirmed_edit_is_applied_through_the_witnessed_gate() {
        let env = FixtureClusterEnv::new();
        let live = env
            .manifest(ResourceKind::Pod, "catch-api-7d9f", Some("catch"))
            .expect("fixture manifest");
        let mut hand = Scripted {
            edits: vec![Ok(Edited::Text(
                live.replace("catch-api", "catch-api\n  labels: {a: b}\n  x"),
            ))],
            seen: Vec::new(),
            answer: true,
            diffs: Vec::new(),
        };
        let r = run(&env, &op(), &target(), &mut hand);
        assert!(matches!(r, ActionResult::BreakGlassRecord { .. }), "{r:?}");
        assert!(
            hand.diffs[0].contains("+  labels: {a: b}"),
            "{}",
            hand.diffs[0]
        );
        assert!(!hand.seen[0].contains("managedFields"));
    }

    #[test]
    fn a_refused_manifest_reopens_the_editor_with_the_error_and_the_edit_kept() {
        let env = FixtureClusterEnv::new();
        let mut hand = Scripted {
            edits: vec![
                Ok(Edited::Text(
                    "kind: Pod\nmetadata: {name: invalid}\n".into(),
                )),
                Ok(Edited::Unchanged),
            ],
            seen: Vec::new(),
            answer: true,
            diffs: Vec::new(),
        };
        let r = run(&env, &op(), &target(), &mut hand);
        assert_eq!(hand.seen.len(), 2, "the editor opened again");
        assert!(hand.seen[1].starts_with(MARK), "{}", hand.seen[1]);
        assert!(hand.seen[1].contains("invalid field"));
        assert!(
            hand.seen[1].contains("metadata: {name: invalid}"),
            "the edit is kept"
        );
        assert!(matches!(r, ActionResult::Error(_)));
    }

    #[test]
    fn a_declined_diff_applies_nothing() {
        let env = FixtureClusterEnv::new();
        let mut hand = Scripted {
            edits: vec![Ok(Edited::Text("kind: Pod\nmetadata: {name: x}\n".into()))],
            seen: Vec::new(),
            answer: false,
            diffs: Vec::new(),
        };
        assert_eq!(
            run(&env, &op(), &target(), &mut hand),
            ActionResult::Error("edit: not applied".into())
        );
    }

    #[test]
    fn the_default_editor_spawns_nothing_and_says_how_to_opt_in() {
        let env = FixtureClusterEnv::new();
        match run(&env, &op(), &target(), &mut Unbuilt) {
            ActionResult::Error(m) => {
                assert!(
                    m.contains("not built") && m.contains("editor: :external"),
                    "{m}"
                );
            }
            other => panic!("expected the refusal, got {other:?}"),
        }
        assert_eq!(EditorChoice::default(), EditorChoice::Embedded);
    }

    #[test]
    fn the_header_is_stripped_before_the_manifest_reaches_the_server() {
        let text = with_header("line one\nline two", "kind: Pod\n");
        assert_eq!(without_header(&text), "kind: Pod\n");
    }
}
