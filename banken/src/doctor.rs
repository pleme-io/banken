use blue_lang_host::{Canonical, Embedded, Entry, Program};
use serde::{Deserialize, Serialize};

const SHINSATSU: Embedded<'static> = Embedded {
    name: "shinsatsu",
    files: &[(
        "shinsatsu.b",
        include_str!("../bidamas/shinsatsu/shinsatsu.b"),
    )],
};

const ENTRY: &str = "use(\"shinsatsu\", %i[classes_json diagnose_json incomplete_json])\n\ndef classes()\n  classes_json()\nend\n\ndef diagnosed(text)\n  diagnose_json(text)\nend\n\ndef incomplete()\n  incomplete_json()\nend\n";

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Facts {
    pub program: String,
    pub args: Vec<String>,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cluster: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Fact {
    pub label: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Diagnosis {
    pub provider: String,
    pub label: String,
    pub class: String,
    pub title: String,
    pub diagnosis: String,
    pub facts: Vec<Fact>,
    #[serde(deserialize_with = "argvs")]
    pub steps: Vec<Vec<String>>,
    pub verify: Vec<String>,
}

impl Diagnosis {
    #[must_use]
    pub fn is_refusal(&self) -> bool {
        self.provider == "nerai"
    }

    #[must_use]
    pub fn headline(&self) -> String {
        let mut s = String::new();
        if let Some(cluster) = self.fact("cluster") {
            s.push_str(cluster);
            s.push_str(": ");
        }
        s.push_str(&self.title);
        if let Some(step) = self.steps.last() {
            s.push_str(" — run `");
            s.push_str(&step.join(" "));
            s.push('`');
        }
        s
    }

    #[must_use]
    pub fn fact(&self, label: &str) -> Option<&str> {
        self.facts
            .iter()
            .find(|f| f.label == label)
            .map(|f| f.value.as_str())
    }
}

#[derive(Deserialize)]
struct Step {
    argv: Vec<String>,
}

fn argvs<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Vec<Vec<String>>, D::Error> {
    Ok(Vec::<Step>::deserialize(d)?
        .into_iter()
        .map(|s| s.argv)
        .collect())
}

fn call(function: &str, arg: Option<String>) -> Result<String, String> {
    let mut program = Program::<()>::builder(Entry::anonymous(ENTRY))
        .loader(&SHINSATSU)
        .canonical(Canonical::AsWritten)
        .load(&mut ())
        .map_err(|e| e.to_string())?;
    match arg {
        Some(a) => program.call::<String>(function, (a,), &mut ()),
        None => program.call::<String>(function, (), &mut ()),
    }
    .map_err(|e| e.to_string())
}

/// # Errors
///
/// The embedded doctor failed to load or to answer.
pub fn diagnose(facts: &Facts) -> Result<Option<Diagnosis>, String> {
    let input = serde_json::to_string(facts).map_err(|e| e.to_string())?;
    let out = call("diagnosed", Some(input))?;
    serde_json::from_str(&out).map_err(|e| e.to_string())
}

/// # Errors
///
/// The embedded doctor failed to load or to answer.
pub fn classes() -> Result<Vec<(String, String)>, String> {
    let rows: Vec<ClassRow> =
        serde_json::from_str(&call("classes", None)?).map_err(|e| e.to_string())?;
    Ok(rows.into_iter().map(|r| (r.provider, r.class)).collect())
}

#[derive(Deserialize)]
struct ClassRow {
    provider: String,
    class: String,
}

/// # Errors
///
/// The embedded doctor failed to load or to answer.
pub fn incomplete() -> Result<Vec<String>, String> {
    serde_json::from_str(&call("incomplete", None)?).map_err(|e| e.to_string())
}

#[must_use]
pub fn facts_for(
    exec: Option<&crate::estate::ExecSpec>,
    message: &str,
    cluster: &str,
    server: Option<&str>,
    target: Option<&str>,
) -> Facts {
    let (program, args) = exec.map_or_else(
        || (String::new(), Vec::new()),
        |e| (e.program().to_owned(), e.args.clone()),
    );
    Facts {
        program,
        args,
        message: message.to_owned(),
        cluster: Some(cluster.to_owned()),
        server: server.map(str::to_owned),
        target: target.map(str::to_owned),
    }
}

#[must_use]
pub fn legible(message: &str, facts: &Facts) -> String {
    match diagnose(facts) {
        Ok(Some(d)) => d.headline(),
        Ok(None) | Err(_) => message.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SERVER_ID: &str = "00000000-0000-0000-0000-00000000abcd";

    const GRANT_REVOKED: &str = r#"kubeconfig context `east-staging-aks`: exec-plugin: failed: {"error":"invalid_grant","error_description":"AADSTS50173: The provided grant has expired due to it being revoked, a fresh auth token is needed. The user might have changed or reset their password. Trace ID: 0 Correlation ID: 0","error_codes":[50173]}"#;

    fn azure(message: &str) -> Facts {
        Facts {
            program: "kubelogin".into(),
            args: vec![
                "get-token".into(),
                "--login".into(),
                "azurecli".into(),
                "--server-id".into(),
                SERVER_ID.into(),
            ],
            message: message.into(),
            cluster: Some("east-staging".into()),
            server: Some("https://east-staging.example:443".into()),
            target: Some("east-staging".into()),
        }
    }

    #[test]
    fn a_revoked_azure_grant_is_a_short_legible_status() {
        let d = diagnose(&azure(GRANT_REVOKED))
            .expect("the doctor answers")
            .expect("AADSTS50173 is a known class");
        assert_eq!(d.provider, "azure");
        assert_eq!(d.class, "grant_revoked");
        assert_eq!(d.title, "Azure login expired");
        let scope = format!("{SERVER_ID}/.default");
        assert_eq!(
            d.steps,
            vec![
                vec!["az".to_owned(), "logout".to_owned()],
                vec![
                    "az".to_owned(),
                    "login".to_owned(),
                    "--scope".to_owned(),
                    scope.clone(),
                ],
            ]
        );
        assert_eq!(d.verify[0], "kubelogin");
        let line = d.headline();
        assert_eq!(
            line,
            format!("east-staging: Azure login expired — run `az login --scope {scope}`")
        );
        assert!(!line.contains('{'), "no escaped JSON reaches the status");
    }

    #[test]
    fn a_named_tenant_reaches_the_login_command() {
        let mut f = azure(GRANT_REVOKED);
        f.args.extend(["--tenant-id".into(), "t-123".into()]);
        let d = diagnose(&f).expect("answers").expect("known");
        assert_eq!(
            d.steps[1][..4],
            ["az", "login", "--tenant", "t-123"].map(str::to_owned)
        );
        assert_eq!(d.fact("tenant"), Some("t-123"));
    }

    #[test]
    fn an_expired_aws_sso_session_names_the_profile() {
        let d = diagnose(&Facts {
            program: "aws".into(),
            args: vec![
                "eks".into(),
                "get-token".into(),
                "--cluster-name".into(),
                "c".into(),
                "--profile".into(),
                "ops".into(),
            ],
            message: "Error when retrieving token from sso: Token has expired and refresh failed"
                .into(),
            ..Facts::default()
        })
        .expect("answers")
        .expect("known");
        assert_eq!(d.class, "sso_expired");
        assert_eq!(
            d.steps,
            vec![vec!["aws", "sso", "login", "--profile", "ops"]]
        );
    }

    #[test]
    fn an_unknown_failure_is_none_and_keeps_its_own_words() {
        let f = Facts {
            program: "kubelogin".into(),
            message: "connection reset by peer".into(),
            ..Facts::default()
        };
        assert_eq!(diagnose(&f).expect("answers"), None);
        assert_eq!(
            legible("connection reset by peer", &f),
            "connection reset by peer"
        );
    }

    #[test]
    fn every_failure_class_has_a_diagnosis_and_a_remediation() {
        assert_eq!(incomplete().expect("answers"), Vec::<String>::new());
    }

    #[test]
    fn every_failure_class_is_reachable_by_its_own_marker() {
        let classes = classes().expect("answers");
        assert!(classes.len() >= 10);
        let markers: &[(&str, &str, &str)] = &[
            ("azure", "grant_revoked", "AADSTS50173"),
            ("azure", "token_expired", "AADSTS700082"),
            ("azure", "wrong_tenant", "AADSTS50020"),
            ("azure", "no_subscription", "No subscriptions found"),
            (
                "azure",
                "not_logged_in",
                "Please run 'az login' to setup account",
            ),
            ("aws", "sso_expired", "Error when retrieving token from sso"),
            ("aws", "no_credentials", "Unable to locate credentials"),
            ("gcp", "reauth", "Reauthentication failed"),
            ("gcp", "no_adc", "could not find default credentials"),
            ("nerai", "refused", "nerai: oldbox is retired"),
        ];
        for (provider, class) in &classes {
            let row = markers
                .iter()
                .find(|(p, c, _)| p == provider && c == class)
                .unwrap_or_else(|| panic!("{provider}/{class} has no test row"));
            let program = match provider.as_str() {
                "azure" => "kubelogin",
                "aws" => "aws",
                "gcp" => "gke-gcloud-auth-plugin",
                _ => "nerai-refuse",
            };
            let d = diagnose(&Facts {
                program: program.into(),
                message: row.2.into(),
                target: Some("t".into()),
                ..Facts::default()
            })
            .expect("answers")
            .unwrap_or_else(|| panic!("{provider}/{class}: its marker classifies nothing"));
            assert_eq!((&d.provider, &d.class), (provider, class));
        }
    }
}
