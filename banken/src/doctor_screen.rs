use std::sync::Arc;
use std::time::{Duration, Instant};

use egaku_term::crossterm::style::Color;
use egaku_term::{__re::KeyMap, AsyncApp, Buffer, Result as TermResult, Style};

use crate::aad::{DeviceCode, Poll, Transport};
use crate::doctor::Diagnosis;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DoctorAction {
    Run,
    Retry,
    Back,
    Abort,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Choice {
    Healed,
    Back,
    Quit,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Native {
    AadDevice { tenant: String, server_id: String },
    None { why: String },
}

impl Native {
    #[must_use]
    pub fn of(d: &Diagnosis) -> Self {
        match (d.provider.as_str(), d.fact("server id")) {
            ("azure", Some(sid)) => Self::AadDevice {
                tenant: d.fact("tenant").unwrap_or("organizations").to_owned(),
                server_id: sid.to_owned(),
            },
            ("azure", None) => Self::None {
                why: "the kubeconfig names no --server-id, so banken cannot ask for the AKS scope"
                    .into(),
            },
            ("nerai", _) => Self::None {
                why: "the registry refuses this name on purpose; open its target instead".into(),
            },
            (_, _) => {
                let mut why = String::from("banken has no native ");
                why.push_str(&d.label);
                why.push_str(
                    " sign-in yet, and it runs no command for you; sign in with your own tools, then press r",
                );
                Self::None { why }
            }
        }
    }

    #[must_use]
    pub fn requests(&self) -> Vec<crate::aad::Request> {
        match self {
            Self::AadDevice { tenant, server_id } => vec![
                crate::aad::device_request(tenant, server_id),
                crate::aad::token_request(tenant, ""),
            ],
            Self::None { .. } => Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Phase {
    Ready,
    Waiting {
        code: DeviceCode,
        started: Instant,
        next: Instant,
    },
    Failed(String),
}

pub struct DoctorScreen {
    diagnosis: Diagnosis,
    native: Native,
    phase: Phase,
    transport: Arc<dyn Transport>,
    keys: awase::KeyMode<DoctorAction>,
    choice: Option<Choice>,
}

impl DoctorScreen {
    #[must_use]
    pub fn new(diagnosis: Diagnosis) -> Self {
        Self::with_transport(diagnosis, Arc::new(crate::aad::Https))
    }

    #[must_use]
    pub fn with_transport(diagnosis: Diagnosis, transport: Arc<dyn Transport>) -> Self {
        Self {
            native: Native::of(&diagnosis),
            diagnosis,
            phase: Phase::Ready,
            transport,
            keys: doctor_keymap(),
            choice: None,
        }
    }

    #[must_use]
    pub fn choice(&self) -> Option<Choice> {
        self.choice
    }

    #[must_use]
    pub fn native(&self) -> &Native {
        &self.native
    }

    #[must_use]
    pub fn is_waiting(&self) -> bool {
        matches!(self.phase, Phase::Waiting { .. })
    }

    pub fn tick(&mut self) {
        self.tick_at(Instant::now());
    }

    pub fn tick_at(&mut self, now: Instant) {
        let Native::AadDevice { tenant, server_id } = &self.native else {
            return;
        };
        let Phase::Waiting { code, next, .. } = &self.phase else {
            return;
        };
        if now < *next {
            return;
        }
        let interval = Duration::from_secs(code.interval.max(1));
        match crate::aad::poll(self.transport.as_ref(), tenant, &code.device_code) {
            Poll::Pending => {
                if let Phase::Waiting { next, .. } = &mut self.phase {
                    *next = now + interval;
                }
            }
            Poll::SlowDown => {
                if let Phase::Waiting { next, .. } = &mut self.phase {
                    *next = now + interval + Duration::from_secs(5);
                }
            }
            Poll::Token(token) => {
                crate::aad::remember(server_id, token);
                self.choice = Some(Choice::Healed);
            }
            Poll::Failed(why) => self.phase = Phase::Failed(why),
        }
    }

    pub fn dispatch(&mut self, action: DoctorAction) {
        match action {
            DoctorAction::Run => {
                if self.is_waiting() {
                    return;
                }
                if let Native::AadDevice { tenant, server_id } = &self.native {
                    self.phase = match crate::aad::start(self.transport.as_ref(), tenant, server_id)
                    {
                        Ok(code) => {
                            let now = Instant::now();
                            Phase::Waiting {
                                next: now + Duration::from_secs(code.interval.max(1)),
                                code,
                                started: now,
                            }
                        }
                        Err(why) => Phase::Failed(why),
                    };
                }
            }
            DoctorAction::Retry => self.choice = Some(Choice::Healed),
            DoctorAction::Back => {
                if self.is_waiting() {
                    self.phase = Phase::Ready;
                } else {
                    self.choice = Some(Choice::Back);
                }
            }
            DoctorAction::Abort => self.choice = Some(Choice::Quit),
        }
    }

    fn lines(&self, width: u16) -> Vec<(String, Style)> {
        let d = &self.diagnosis;
        let head = Style::default().fg(Color::Cyan).bold();
        let dim = Style::default().fg(Color::DarkGrey);
        let warn = Style::default().fg(Color::Yellow).bold();
        let big = Style::default().fg(Color::Black).bg(Color::Yellow).bold();
        let bad = Style::default().fg(Color::Red).bold();
        let plain = Style::default();
        let mut lines: Vec<(String, Style)> = Vec::new();
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
        match &self.native {
            Native::AadDevice { .. } => {
                lines.push((
                    "enter sends, from banken itself (no command runs):".to_owned(),
                    head,
                ));
                for (i, r) in self.native.requests().iter().enumerate() {
                    let mut s = format!("{}. POST ", i + 1);
                    s.push_str(&r.url);
                    lines.push((s, plain));
                    for (k, v) in &r.form {
                        let mut f = String::from("     ");
                        f.push_str(k);
                        f.push('=');
                        f.push_str(if k == "device_code" {
                            "<from step 1>"
                        } else {
                            v
                        });
                        lines.push((f, dim));
                    }
                }
            }
            Native::None { why } => {
                for w in egaku_term::draw::wrap_text(why, width.saturating_sub(2)) {
                    lines.push((w, warn));
                }
                if !d.steps.is_empty() {
                    lines.push((String::new(), plain));
                    lines.push((
                        "what usually fixes it (banken does not run it):".to_owned(),
                        dim,
                    ));
                    for s in &d.steps {
                        let mut l = String::from("  ");
                        l.push_str(&s.argv.join(" "));
                        lines.push((l, dim));
                    }
                }
            }
        }
        match &self.phase {
            Phase::Ready => {}
            Phase::Waiting { code, started, .. } => {
                lines.push((String::new(), plain));
                let mut u = String::from(" open  ");
                u.push_str(&code.verification_uri);
                u.push(' ');
                lines.push((u, big));
                let mut c = String::from(" code  ");
                c.push_str(&code.user_code);
                c.push(' ');
                lines.push((c, big));
                let mut w = String::from("polling every ");
                w.push_str(&code.interval.to_string());
                w.push_str(" s  ·  waited ");
                w.push_str(&started.elapsed().as_secs().to_string());
                w.push_str(" s of ");
                w.push_str(&code.expires_in.to_string());
                lines.push((w, warn));
            }
            Phase::Failed(why) => {
                lines.push((String::new(), plain));
                let mut s = String::from("❌ ");
                s.push_str(why);
                lines.push((s, bad));
            }
        }
        lines
    }

    pub fn render(&self, buf: &mut Buffer) {
        let width = buf.width();
        let height = buf.height();
        if width == 0 || height == 0 {
            return;
        }
        for (i, (text, style)) in self.lines(width).iter().enumerate() {
            let Ok(y) = u16::try_from(i) else { break };
            if y + 1 >= height {
                break;
            }
            buf.set_stringn(1, y, text, width.saturating_sub(1), *style);
        }
        let hint = if self.is_waiting() {
            "esc: stop waiting  ·  ctrl+c: quit"
        } else if matches!(self.native, Native::AadDevice { .. }) {
            "enter: sign in  ·  r: retry the connect  ·  esc: back to clusters  ·  ctrl+c: quit"
        } else {
            "r: retry the connect  ·  esc: back to clusters  ·  ctrl+c: quit"
        };
        let style = Style::default().fg(Color::Black).bg(Color::DarkGrey);
        let y = height - 1;
        buf.blank(0, y, width, style);
        buf.set_stringn(0, y, hint, width, style);
    }
}

fn doctor_keymap() -> awase::KeyMode<DoctorAction> {
    use awase::{Binding, Hotkey, Key, Modifiers};
    let mut km: awase::KeyMode<DoctorAction> = awase::KeyMode::typed("doctor", false);
    for (hk, a) in [
        (Hotkey::new(Modifiers::NONE, Key::Return), DoctorAction::Run),
        (Hotkey::new(Modifiers::NONE, Key::R), DoctorAction::Retry),
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

    fn wake(&self) -> impl Future<Output = ()> + Send {
        let busy = self.is_waiting();
        async move {
            if busy {
                tokio::time::sleep(Duration::from_millis(500)).await;
            } else {
                std::future::pending::<()>().await;
            }
        }
    }

    fn on_wake(&mut self) -> impl Future<Output = TermResult<()>> {
        self.tick();
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
    use crate::aad::Request;
    use egaku_term::TestBackend;
    use std::sync::Mutex;

    type Answers = Vec<(u16, String)>;

    struct Recorded {
        answers: Mutex<Answers>,
        sent: Mutex<Vec<Request>>,
    }

    impl Transport for Recorded {
        fn post_form(&self, request: &Request) -> Result<(u16, String), String> {
            self.sent.lock().unwrap().push(request.clone());
            Ok(self.answers.lock().unwrap().remove(0))
        }
    }

    fn diagnosis(program: &str, args: &[&str], message: &str) -> Diagnosis {
        crate::doctor::diagnose(&crate::doctor::Facts {
            program: program.into(),
            args: args.iter().map(|a| (*a).to_owned()).collect(),
            message: message.into(),
            cluster: Some("east-staging".into()),
            server: Some("https://east.example".into()),
            target: Some("east-staging".into()),
        })
        .expect("answers")
        .expect("known")
    }

    fn azure() -> Diagnosis {
        diagnosis(
            "kubelogin",
            &["get-token", "--server-id", "sid-native"],
            "AADSTS50173: grant revoked",
        )
    }

    fn recorded(answers: Answers) -> Arc<Recorded> {
        Arc::new(Recorded {
            answers: Mutex::new(answers),
            sent: Mutex::new(Vec::new()),
        })
    }

    fn frame(s: &DoctorScreen) -> String {
        let mut b = TestBackend::new(140, 30);
        b.draw(|buf| s.render(buf));
        b.to_lines().join("\n")
    }

    #[test]
    fn the_panel_shows_every_request_before_anything_is_sent() {
        let t = recorded(Vec::new());
        let s = DoctorScreen::with_transport(azure(), t.clone());
        let f = frame(&s);
        for want in [
            "Azure credential",
            "Azure login expired",
            "server id",
            "1. POST https://login.microsoftonline.com/organizations/oauth2/v2.0/devicecode",
            "scope=sid-native/.default offline_access",
            "client_id=04b07795-8ddb-461a-bbee-02f9e1bf7b46",
            "device_code=<from step 1>",
            "2. POST https://login.microsoftonline.com/organizations/oauth2/v2.0/token",
            "no command runs",
        ] {
            assert!(f.contains(want), "`{want}` missing:\n{f}");
        }
        assert!(
            t.sent.lock().unwrap().is_empty(),
            "nothing is sent without a keypress"
        );
    }

    #[test]
    fn the_native_device_flow_shows_the_code_polls_and_heals() {
        let t = recorded(vec![
            (
                200,
                r#"{"user_code":"DXQ4KP7LM","device_code":"dc","verification_uri":"https://microsoft.com/devicelogin","interval":1,"expires_in":900}"#.into(),
            ),
            (400, r#"{"error":"authorization_pending"}"#.into()),
            (200, r#"{"access_token":"tok-native"}"#.into()),
        ]);
        let mut s = DoctorScreen::with_transport(azure(), t.clone());
        s.dispatch(DoctorAction::Run);
        assert!(s.is_waiting());
        let f = frame(&s);
        assert!(f.contains("open  https://microsoft.com/devicelogin"), "{f}");
        assert!(f.contains("code  DXQ4KP7LM"), "{f}");
        let later = Instant::now() + Duration::from_secs(2);
        s.tick_at(later);
        assert_eq!(s.choice(), None, "pending keeps waiting");
        s.tick_at(later + Duration::from_secs(2));
        assert_eq!(s.choice(), Some(Choice::Healed));
        assert_eq!(
            crate::aad::token_for("sid-native").as_deref(),
            Some("tok-native")
        );
        assert_eq!(t.sent.lock().unwrap().len(), 3);
    }

    #[test]
    fn a_refused_poll_says_why_and_stops() {
        let t = recorded(vec![
            (
                200,
                r#"{"user_code":"C","device_code":"dc","verification_uri":"https://x","interval":1,"expires_in":900}"#.into(),
            ),
            (400, r#"{"error":"expired_token","error_description":"AADSTS70020: expired"}"#.into()),
        ]);
        let mut s = DoctorScreen::with_transport(azure(), t);
        s.dispatch(DoctorAction::Run);
        s.tick_at(Instant::now() + Duration::from_secs(2));
        assert!(!s.is_waiting());
        assert!(frame(&s).contains("❌ AADSTS70020: expired"));
    }

    #[test]
    fn a_provider_without_a_native_flow_runs_nothing_and_offers_retry() {
        let t = recorded(Vec::new());
        let mut s = DoctorScreen::with_transport(
            diagnosis(
                "aws",
                &["--profile", "ops"],
                "Error when retrieving token from sso",
            ),
            t.clone(),
        );
        assert!(matches!(s.native(), Native::None { .. }));
        let f = frame(&s);
        assert!(f.contains("banken does not run it"), "{f}");
        s.dispatch(DoctorAction::Run);
        assert!(t.sent.lock().unwrap().is_empty());
        s.dispatch(DoctorAction::Retry);
        assert_eq!(s.choice(), Some(Choice::Healed));
    }

    #[test]
    fn esc_stops_waiting_first_then_goes_back() {
        let t = recorded(vec![(
            200,
            r#"{"user_code":"C","device_code":"dc","verification_uri":"https://x","interval":5,"expires_in":900}"#.into(),
        )]);
        let mut s = DoctorScreen::with_transport(azure(), t);
        s.dispatch(DoctorAction::Run);
        s.dispatch(DoctorAction::Back);
        assert!(!s.is_waiting());
        assert_eq!(s.choice(), None);
        s.dispatch(DoctorAction::Back);
        assert_eq!(s.choice(), Some(Choice::Back));
    }
}
