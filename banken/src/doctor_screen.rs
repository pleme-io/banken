use egaku_term::crossterm::style::Color;
use egaku_term::{__re::KeyMap, AsyncApp, Buffer, Result as TermResult, Style};

use crate::doctor::Diagnosis;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DoctorAction {
    Run,
    Verify,
    Back,
    Abort,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Choice {
    Run,
    Verify,
    Back,
    Quit,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ran {
    pub argv: Vec<String>,
    pub outcome: Result<i32, String>,
}

impl Ran {
    #[must_use]
    pub fn ok(&self) -> bool {
        matches!(self.outcome, Ok(0))
    }

    #[must_use]
    pub fn line(&self) -> String {
        let mut s = String::from(if self.ok() { "✅ " } else { "❌ " });
        s.push_str(&self.argv.join(" "));
        match &self.outcome {
            Ok(0) => {}
            Ok(code) => {
                s.push_str("  (exit ");
                s.push_str(&code.to_string());
                s.push(')');
            }
            Err(why) => {
                s.push_str("  (");
                s.push_str(why);
                s.push(')');
            }
        }
        s
    }
}

pub struct DoctorScreen {
    diagnosis: Diagnosis,
    ran: Vec<Ran>,
    verified: Option<Ran>,
    keys: awase::KeyMode<DoctorAction>,
    choice: Option<Choice>,
}

impl DoctorScreen {
    #[must_use]
    pub fn new(diagnosis: Diagnosis) -> Self {
        Self {
            diagnosis,
            ran: Vec::new(),
            verified: None,
            keys: doctor_keymap(),
            choice: None,
        }
    }

    #[must_use]
    pub fn with_results(mut self, ran: Vec<Ran>, verified: Option<Ran>) -> Self {
        self.ran = ran;
        self.verified = verified;
        self
    }

    #[must_use]
    pub fn choice(&self) -> Option<Choice> {
        self.choice
    }

    #[must_use]
    pub fn diagnosis(&self) -> &Diagnosis {
        &self.diagnosis
    }

    pub fn dispatch(&mut self, action: DoctorAction) {
        self.choice = Some(match action {
            DoctorAction::Run if self.diagnosis.steps.is_empty() => return,
            DoctorAction::Run => Choice::Run,
            DoctorAction::Verify if self.diagnosis.verify.is_empty() => return,
            DoctorAction::Verify => Choice::Verify,
            DoctorAction::Back => Choice::Back,
            DoctorAction::Abort => Choice::Quit,
        });
    }

    #[allow(
        clippy::many_single_char_names,
        reason = "x/y/width are the buffer-coordinate vocabulary every draw fn in this crate shares"
    )]
    pub fn render(&self, buf: &mut Buffer) {
        let width = buf.width();
        let height = buf.height();
        if width == 0 || height == 0 {
            return;
        }
        let d = &self.diagnosis;
        let mut lines: Vec<(String, Style)> = Vec::new();
        let head = Style::default().fg(Color::Cyan).bold();
        let dim = Style::default().fg(Color::DarkGrey);
        let warn = Style::default().fg(Color::Yellow).bold();
        let plain = Style::default();
        let mut title = String::from("banken 番犬  —  ");
        title.push_str(&d.label);
        title.push_str(" credential");
        lines.push((title, head));
        lines.push((String::new(), plain));
        lines.push((d.title.clone(), warn));
        for w in egaku_term::draw::wrap_text(&d.diagnosis, width.saturating_sub(2)) {
            lines.push((w, plain));
        }
        lines.push((String::new(), plain));
        for f in &d.facts {
            let mut s = format!("{:<12}", f.label);
            s.push_str(&f.value);
            lines.push((s, dim));
        }
        lines.push((String::new(), plain));
        if d.steps.is_empty() {
            lines.push(("Nothing to run.".to_owned(), dim));
        } else {
            lines.push(("enter runs, in order:".to_owned(), head));
            for (i, step) in d.steps.iter().enumerate() {
                let done = self.ran.iter().find(|r| &r.argv == step);
                let s = match done {
                    Some(r) => r.line(),
                    None => format!("{}. {}", i + 1, step.join(" ")),
                };
                lines.push((s, plain));
            }
        }
        if !d.verify.is_empty() {
            lines.push((String::new(), plain));
            lines.push(("then checks with:".to_owned(), head));
            let s = match &self.verified {
                Some(r) => r.line(),
                None => d.verify.join(" "),
            };
            lines.push((s, plain));
        }
        for (i, (text, style)) in lines.iter().enumerate() {
            let Ok(y) = u16::try_from(i) else { break };
            if y + 1 >= height {
                break;
            }
            buf.set_stringn(1, y, text, width.saturating_sub(1), *style);
        }
        let mut hint = String::new();
        if !d.steps.is_empty() {
            hint.push_str("enter: run  ·  ");
        }
        if !d.verify.is_empty() {
            hint.push_str("v: check only  ·  ");
        }
        hint.push_str("esc: back to clusters  ·  ctrl+c: quit");
        let style = Style::default().fg(Color::Black).bg(Color::DarkGrey);
        let y = height - 1;
        buf.blank(0, y, width, style);
        buf.set_stringn(0, y, &hint, width, style);
    }
}

fn doctor_keymap() -> awase::KeyMode<DoctorAction> {
    use awase::{Binding, Hotkey, Key, Modifiers};
    let mut km: awase::KeyMode<DoctorAction> = awase::KeyMode::typed("doctor", false);
    for (hk, a) in [
        (Hotkey::new(Modifiers::NONE, Key::Return), DoctorAction::Run),
        (Hotkey::new(Modifiers::NONE, Key::V), DoctorAction::Verify),
        (
            Hotkey::new(Modifiers::NONE, Key::Escape),
            DoctorAction::Back,
        ),
        (Hotkey::new(Modifiers::NONE, Key::H), DoctorAction::Back),
        (Hotkey::new(Modifiers::CTRL, Key::C), DoctorAction::Abort),
    ] {
        let _prev = km.add_binding(Binding::new(hk, a));
    }
    km
}

#[must_use]
pub fn run_step(argv: &[String]) -> Ran {
    let outcome = match argv.split_first() {
        None => Err("empty command".to_owned()),
        Some((program, rest)) => {
            println!("\n$ {}", argv.join(" "));
            std::process::Command::new(program)
                .args(rest)
                .status()
                .map(|s| s.code().unwrap_or(-1))
                .map_err(|e| e.to_string())
        }
    };
    Ran {
        argv: argv.to_vec(),
        outcome,
    }
}

#[must_use]
pub fn verify_quietly(argv: &[String]) -> Ran {
    let outcome = match argv.split_first() {
        None => Err("empty command".to_owned()),
        Some((program, rest)) => std::process::Command::new(program)
            .args(rest)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.code().unwrap_or(-1))
            .map_err(|e| e.to_string()),
    };
    Ran {
        argv: argv.to_vec(),
        outcome,
    }
}

impl AsyncApp for DoctorScreen {
    type Action = DoctorAction;

    fn keymap(&self) -> &KeyMap<DoctorAction> {
        static EMPTY: std::sync::OnceLock<KeyMap<DoctorAction>> = std::sync::OnceLock::new();
        EMPTY.get_or_init(KeyMap::new)
    }

    fn hotkey_map(&self) -> Option<&awase::KeyMode<DoctorAction>> {
        Some(&self.keys)
    }

    fn unclaimed(&self) -> egaku_term::app::Unclaimed {
        egaku_term::app::Unclaimed::Consume
    }

    fn handle(&mut self, action: &DoctorAction) -> impl Future<Output = TermResult<()>> {
        self.dispatch(*action);
        std::future::ready(Ok(()))
    }

    fn draw(&self, frame: &mut Buffer) -> impl Future<Output = TermResult<()>> {
        self.render(frame);
        std::future::ready(Ok(()))
    }

    fn should_quit(&self) -> bool {
        self.choice.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egaku_term::TestBackend;

    fn grant_revoked() -> Diagnosis {
        crate::doctor::diagnose(&crate::doctor::Facts {
            program: "kubelogin".into(),
            args: vec!["get-token".into(), "--server-id".into(), "sid".into()],
            message: "AADSTS50173: grant revoked".into(),
            cluster: Some("east-staging".into()),
            server: Some("https://east.example".into()),
            target: Some("east-staging".into()),
        })
        .expect("answers")
        .expect("known")
    }

    fn frame(s: &DoctorScreen) -> String {
        let mut b = TestBackend::new(100, 24);
        b.draw(|buf| s.render(buf));
        b.to_lines().join("\n")
    }

    #[test]
    fn the_panel_shows_every_command_before_anything_runs() {
        let s = DoctorScreen::new(grant_revoked());
        let f = frame(&s);
        for want in [
            "Azure credential",
            "Azure login expired",
            "east-staging",
            "server id",
            "1. az logout",
            "2. az login --scope sid/.default",
            "kubelogin get-token --server-id sid",
            "enter: run",
        ] {
            assert!(f.contains(want), "`{want}` missing:\n{f}");
        }
        assert_eq!(s.choice(), None, "nothing is chosen without a keypress");
    }

    #[test]
    fn enter_chooses_run_and_esc_goes_back() {
        let mut s = DoctorScreen::new(grant_revoked());
        s.dispatch(DoctorAction::Run);
        assert_eq!(s.choice(), Some(Choice::Run));
        let mut s = DoctorScreen::new(grant_revoked());
        s.dispatch(DoctorAction::Back);
        assert_eq!(s.choice(), Some(Choice::Back));
    }

    #[test]
    fn results_replace_the_numbered_steps() {
        let d = grant_revoked();
        let ran = vec![Ran {
            argv: d.steps[0].clone(),
            outcome: Ok(0),
        }];
        let verified = Ran {
            argv: d.verify.clone(),
            outcome: Ok(1),
        };
        let f = frame(&DoctorScreen::new(d).with_results(ran, Some(verified)));
        assert!(f.contains("✅ az logout"), "{f}");
        assert!(
            f.contains("❌ kubelogin get-token --server-id sid  (exit 1)"),
            "{f}"
        );
    }

    #[test]
    fn a_step_that_cannot_start_says_why() {
        let r = verify_quietly(&["/nonexistent/banken-probe".to_owned()]);
        assert!(!r.ok());
        assert!(r.line().starts_with("❌ /nonexistent/banken-probe  ("));
    }
}
