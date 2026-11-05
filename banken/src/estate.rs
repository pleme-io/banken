use std::path::{Path, PathBuf};

pub const REFUSER: &str = "nerai-refuse";
pub const REFUSED_CLUSTER_PREFIX: &str = "nerai-refused-";

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ExecSpec {
    pub command: String,
    pub args: Vec<String>,
}

impl ExecSpec {
    #[must_use]
    pub fn program(&self) -> &str {
        Path::new(&self.command)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(&self.command)
    }

    #[must_use]
    pub fn is_refuser(&self) -> bool {
        self.program() == REFUSER
    }

    #[must_use]
    pub fn plugin(&self) -> Plugin {
        let flag = |name: &str| {
            self.args
                .iter()
                .position(|a| a == name)
                .and_then(|i| self.args.get(i + 1))
                .cloned()
        };
        match self.program() {
            REFUSER => Plugin::Nerai,
            "kubelogin" => match flag("--server-id") {
                Some(server_id) => Plugin::Kubelogin {
                    server_id,
                    tenant: flag("--tenant-id"),
                },
                None => Plugin::Unknown {
                    program: "kubelogin".into(),
                    why: "kubelogin names no --server-id, so no AKS scope can be asked for".into(),
                },
            },
            "aws" | "aws-iam-authenticator" => Plugin::AwsEks,
            "gke-gcloud-auth-plugin" => Plugin::Gke,
            other => Plugin::Unknown {
                program: other.to_owned(),
                why: "banken resolves kubelogin, aws, gke-gcloud-auth-plugin and nerai-refuse, and runs no other credential plugin".into(),
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Plugin {
    Nerai,
    Kubelogin {
        server_id: String,
        tenant: Option<String>,
    },
    AwsEks,
    Gke,
    Unknown {
        program: String,
        why: String,
    },
}

impl Plugin {
    #[must_use]
    pub fn refusal(&self) -> Option<String> {
        match self {
            Self::Unknown { program, why } => {
                let mut s = String::from("credential plugin `");
                s.push_str(program);
                s.push_str("` refused: ");
                s.push_str(why);
                Some(s)
            }
            Self::Nerai | Self::Kubelogin { .. } | Self::AwsEks | Self::Gke => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Declaration {
    pub context: String,
    pub cluster: String,
    pub server: Option<String>,
    pub exec: Option<ExecSpec>,
}

impl Declaration {
    #[must_use]
    pub fn stub(&self) -> Option<Stub> {
        let exec = self.exec.as_ref().filter(|e| e.is_refuser())?;
        Some(Stub {
            reason: exec.args.join(" "),
            target: self
                .cluster
                .strip_prefix(REFUSED_CLUSTER_PREFIX)
                .map(str::to_owned),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stub {
    pub reason: String,
    pub target: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileDeclarations {
    pub file: PathBuf,
    pub contexts: Vec<Declaration>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub name: String,
    pub file: PathBuf,
    pub sole: Result<Declaration, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Origin {
    Ambient,
    Target,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextChoice {
    pub name: String,
    pub context: String,
    pub server: Option<String>,
    pub files: Vec<PathBuf>,
    pub origin: Origin,
    pub refused: Option<String>,
    pub exec: Option<ExecSpec>,
    pub read_only: bool,
}

impl ContextChoice {
    #[must_use]
    pub fn ambient(name: &str, server: Option<&str>, files: Vec<PathBuf>) -> Self {
        Self {
            name: name.to_owned(),
            context: name.to_owned(),
            server: server.map(str::to_owned),
            files,
            origin: Origin::Ambient,
            refused: None,
            exec: None,
            read_only: false,
        }
    }

    #[must_use]
    pub fn is_ambiguous(&self) -> bool {
        self.refused.is_none() && self.files.len() > 1
    }

    #[must_use]
    pub fn is_refused(&self) -> bool {
        self.refused.is_some()
    }

    #[must_use]
    pub fn is_choosable(&self) -> bool {
        !self.is_ambiguous() && !self.is_refused()
    }

    #[must_use]
    pub fn is_target(&self) -> bool {
        self.origin == Origin::Target
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedContext {
    pub name: String,
    pub context: String,
    pub server: String,
    pub file: PathBuf,
    pub exec: Option<ExecSpec>,
    pub read_only: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContextError {
    Ambiguous {
        name: String,
        files: Vec<PathBuf>,
    },
    NotFound {
        name: String,
        available: Vec<String>,
    },
    Refused {
        name: String,
        reason: String,
        target: Option<String>,
    },
    Unreadable {
        file: PathBuf,
        message: String,
    },
}

impl std::fmt::Display for ContextError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Ambiguous { name, files } => {
                write!(
                    f,
                    "context `{name}` is declared by {} kubeconfig files",
                    files.len()
                )?;
                for (i, p) in files.iter().enumerate() {
                    write!(f, "\n  {}. {}", i + 1, p.display())?;
                    if i == 0 {
                        write!(f, "   <- would silently win")?;
                    }
                }
                write!(
                    f,
                    "\nRefusing: the name does not identify one cluster. \
                     Point KUBECONFIG at a single file, or rename one context."
                )
            }
            Self::NotFound { name, available } => {
                write!(f, "no kubeconfig context named `{name}`. Available:")?;
                for a in available {
                    write!(f, "\n  {a}")?;
                }
                Ok(())
            }
            Self::Refused {
                name,
                reason,
                target,
            } => {
                write!(f, "context `{name}` is refused: {reason}")?;
                if let Some(t) = target {
                    write!(f, "\nbanken reaches it as the registry target `{t}`.")?;
                }
                Ok(())
            }
            Self::Unreadable { file, message } => {
                write!(
                    f,
                    "kubeconfig {} could not be read: {message}",
                    file.display()
                )
            }
        }
    }
}

impl std::error::Error for ContextError {}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Estate {
    pub files: Vec<FileDeclarations>,
    pub targets: Vec<Target>,
}

impl Estate {
    fn target(&self, name: &str) -> Option<&Target> {
        self.targets.iter().find(|t| t.name == name)
    }

    fn read_only(&self, target: &str) -> bool {
        self.files.iter().any(|f| {
            f.contexts
                .iter()
                .any(|d| d.stub().and_then(|s| s.target).as_deref() == Some(target))
        })
    }

    fn live_target(&self, name: &str) -> Option<(&Target, &Declaration)> {
        let t = self.target(name)?;
        t.sole.as_ref().ok().map(|d| (t, d))
    }

    fn declarations_of<'a>(&'a self, context: &str) -> Vec<(&'a Path, &'a Declaration)> {
        self.files
            .iter()
            .flat_map(|f| {
                f.contexts
                    .iter()
                    .filter(move |d| d.context == context)
                    .map(move |d| (f.file.as_path(), d))
            })
            .collect()
    }

    fn covered(&self, d: &Declaration) -> bool {
        self.targets.iter().any(|t| {
            t.sole
                .as_ref()
                .is_ok_and(|s| s.context == d.context && s.server == d.server)
        })
    }

    fn ambient_verdict(&self, context: &str) -> Option<ContextChoice> {
        let decls = self.declarations_of(context);
        let (_, first) = decls.first()?;
        if let Some(stub) = first.stub() {
            return Some(ContextChoice {
                name: context.to_owned(),
                context: context.to_owned(),
                server: first.server.clone(),
                files: vec![decls[0].0.to_path_buf()],
                origin: Origin::Ambient,
                refused: Some(stub.reason),
                exec: first.exec.clone(),
                read_only: true,
            });
        }
        let real: Vec<&(&Path, &Declaration)> =
            decls.iter().filter(|(_, d)| d.stub().is_none()).collect();
        Some(ContextChoice {
            name: context.to_owned(),
            context: context.to_owned(),
            server: first.server.clone(),
            files: real.iter().map(|(f, _)| f.to_path_buf()).collect(),
            origin: Origin::Ambient,
            refused: first.exec.as_ref().and_then(|e| e.plugin().refusal()),
            exec: first.exec.clone(),
            read_only: false,
        })
    }

    #[must_use]
    pub fn choices(&self) -> Vec<ContextChoice> {
        let mut out: Vec<ContextChoice> = Vec::new();
        for t in &self.targets {
            out.push(match &t.sole {
                Ok(d) => ContextChoice {
                    name: t.name.clone(),
                    context: d.context.clone(),
                    server: d.server.clone(),
                    files: vec![t.file.clone()],
                    origin: Origin::Target,
                    refused: d.exec.as_ref().and_then(|e| e.plugin().refusal()),
                    exec: d.exec.clone(),
                    read_only: self.read_only(&t.name),
                },
                Err(why) => ContextChoice {
                    name: t.name.clone(),
                    context: String::new(),
                    server: None,
                    files: vec![t.file.clone()],
                    origin: Origin::Target,
                    refused: Some(why.clone()),
                    exec: None,
                    read_only: true,
                },
            });
        }
        let mut seen: Vec<&str> = Vec::new();
        for f in &self.files {
            for d in &f.contexts {
                if seen.contains(&d.context.as_str()) || self.target(&d.context).is_some() {
                    continue;
                }
                seen.push(&d.context);
                let Some(choice) = self.ambient_verdict(&d.context) else {
                    continue;
                };
                let shadows_a_target = d
                    .stub()
                    .and_then(|s| s.target)
                    .is_some_and(|t| self.live_target(&t).is_some());
                let duplicate = choice.refused.is_none()
                    && self
                        .declarations_of(&d.context)
                        .iter()
                        .all(|(_, x)| self.covered(x));
                let shadowed_duplicate = choice.refused.is_some()
                    && shadows_a_target
                    && self
                        .declarations_of(&d.context)
                        .iter()
                        .filter(|(_, x)| x.stub().is_none())
                        .all(|(_, x)| self.covered(x));
                if duplicate || shadowed_duplicate {
                    continue;
                }
                out.push(choice);
            }
        }
        out
    }

    /// # Errors
    ///
    /// The name is refused by the ambient file, declared by more than one
    /// file, or declared nowhere.
    pub fn resolve(&self, name: &str) -> Result<ResolvedContext, ContextError> {
        let r = self.resolve_declared(name)?;
        match r.exec.as_ref().and_then(|e| e.plugin().refusal()) {
            Some(reason) => Err(ContextError::Refused {
                name: name.to_owned(),
                reason,
                target: None,
            }),
            None => Ok(r),
        }
    }

    fn resolve_declared(&self, name: &str) -> Result<ResolvedContext, ContextError> {
        if let Some(t) = self.target(name) {
            return match &t.sole {
                Ok(d) => Ok(ResolvedContext {
                    name: name.to_owned(),
                    context: d.context.clone(),
                    server: d
                        .server
                        .clone()
                        .unwrap_or_else(|| "<no server declared>".to_owned()),
                    file: t.file.clone(),
                    exec: d.exec.clone(),
                    read_only: self.read_only(name),
                }),
                Err(why) => Err(ContextError::Refused {
                    name: name.to_owned(),
                    reason: why.clone(),
                    target: None,
                }),
            };
        }
        let decls = self.declarations_of(name);
        let Some((first_file, first)) = decls.first() else {
            return Err(ContextError::NotFound {
                name: name.to_owned(),
                available: self.choices().into_iter().map(|c| c.name).collect(),
            });
        };
        if let Some(stub) = first.stub() {
            let target = stub
                .target
                .filter(|t| self.live_target(t).is_some())
                .or_else(|| {
                    self.targets
                        .iter()
                        .find(|t| t.sole.as_ref().is_ok_and(|s| s.context == name))
                        .map(|t| t.name.clone())
                });
            return Err(ContextError::Refused {
                name: name.to_owned(),
                reason: stub.reason,
                target,
            });
        }
        let real: Vec<&(&Path, &Declaration)> =
            decls.iter().filter(|(_, d)| d.stub().is_none()).collect();
        if real.len() > 1 {
            return Err(ContextError::Ambiguous {
                name: name.to_owned(),
                files: real.iter().map(|(f, _)| f.to_path_buf()).collect(),
            });
        }
        Ok(ResolvedContext {
            name: name.to_owned(),
            context: name.to_owned(),
            server: first
                .server
                .clone()
                .unwrap_or_else(|| "<no server declared>".to_owned()),
            file: first_file.to_path_buf(),
            exec: first.exec.clone(),
            read_only: false,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RefusedRows {
    #[default]
    Hide,
    Dim,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decl(context: &str, cluster: &str, server: &str) -> Declaration {
        Declaration {
            context: context.into(),
            cluster: cluster.into(),
            server: Some(server.into()),
            exec: None,
        }
    }

    fn stub(context: &str, target: &str, reason: &str) -> Declaration {
        Declaration {
            context: context.into(),
            cluster: format!("{REFUSED_CLUSTER_PREFIX}{target}"),
            server: Some(format!("https://{target}.refused.invalid")),
            exec: Some(ExecSpec {
                command: "/nix/store/x-nerai-refuse/bin/nerai-refuse".into(),
                args: vec![reason.into()],
            }),
        }
    }

    fn file(path: &str, contexts: Vec<Declaration>) -> FileDeclarations {
        FileDeclarations {
            file: PathBuf::from(path),
            contexts,
        }
    }

    fn target(name: &str, context: &str, server: &str) -> Target {
        Target {
            name: name.into(),
            file: PathBuf::from(format!("/k/targets/{name}")),
            sole: Ok(decl(context, context, server)),
        }
    }

    const PROD_URL: &str = "https://aks-prod.central.example:443";

    fn workstation() -> Estate {
        Estate {
            files: vec![
                file(
                    "/k/order/00",
                    vec![
                        stub(
                            "nerai-no-target",
                            "no-target",
                            "no current context, by design",
                        ),
                        stub("oldbox", "oldbox", "oldbox is retired: gone"),
                        stub(
                            "shop-prod-aks",
                            "shop-prod",
                            "shop-prod is acme production, read-only: reach it through k-shop-prod",
                        ),
                    ],
                ),
                file(
                    "/k/order/01",
                    vec![decl("engenho", "engenho", "https://127.0.0.1:6443")],
                ),
                file(
                    "/k/order/03",
                    vec![decl("oldbox", "oldbox", "https://192.0.2.47:6443")],
                ),
                file(
                    "/k/order/05",
                    vec![
                        decl("shop-prod-aks", "shop-prod-aks", PROD_URL),
                        decl("cicd-eks", "cicd-eks", "https://cicd.eks"),
                    ],
                ),
            ],
            targets: vec![
                target("shop-prod", "shop-prod-aks", PROD_URL),
                target("cicd", "cicd-eks", "https://cicd.eks"),
            ],
        }
    }

    fn names(e: &Estate) -> Vec<String> {
        e.choices().into_iter().map(|c| c.name).collect()
    }

    #[test]
    fn each_real_cluster_appears_once() {
        assert_eq!(
            names(&workstation()),
            ["shop-prod", "cicd", "nerai-no-target", "oldbox", "engenho"]
        );
    }

    #[test]
    fn a_target_its_stub_shadows_is_read_only_and_others_mutate() {
        let e = workstation();
        assert!(e.resolve("shop-prod").expect("resolves").read_only);
        assert!(!e.resolve("cicd").expect("resolves").read_only);
        assert!(!e.resolve("engenho").expect("resolves").read_only);
        let rows = e.choices();
        assert!(rows.iter().any(|c| c.name == "shop-prod" && c.read_only));
        assert!(rows.iter().any(|c| c.name == "cicd" && !c.read_only));
    }

    #[test]
    fn a_stub_shadowing_a_registry_target_is_not_an_ambiguity() {
        let e = workstation();
        assert!(e.choices().iter().all(|c| !c.is_ambiguous()));
        let r = e.resolve("shop-prod").expect("the target resolves");
        assert_eq!(r.file, PathBuf::from("/k/targets/shop-prod"));
        assert_eq!(r.context, "shop-prod-aks");
        assert_eq!(r.server, PROD_URL);
    }

    #[test]
    fn a_refused_name_carries_the_nerai_reason() {
        let e = workstation();
        let oldbox = e
            .choices()
            .into_iter()
            .find(|c| c.name == "oldbox")
            .expect("oldbox is listed");
        assert_eq!(oldbox.refused.as_deref(), Some("oldbox is retired: gone"));
        assert!(!oldbox.is_choosable());
        match e.resolve("oldbox") {
            Err(ContextError::Refused { reason, target, .. }) => {
                assert_eq!(reason, "oldbox is retired: gone");
                assert_eq!(target, None);
            }
            other => panic!("oldbox must be refused, got {other:?}"),
        }
    }

    #[test]
    fn a_shadowed_context_points_at_its_registry_target() {
        match workstation().resolve("shop-prod-aks") {
            Err(ContextError::Refused { target, .. }) => {
                assert_eq!(target.as_deref(), Some("shop-prod"));
            }
            other => panic!("expected a refusal, got {other:?}"),
        }
    }

    #[test]
    fn two_real_declarations_are_still_ambiguous() {
        let mut e = workstation();
        e.files.push(file(
            "/k/config",
            vec![decl("engenho", "engenho", "https://remote.example:6443")],
        ));
        let row = e
            .choices()
            .into_iter()
            .find(|c| c.name == "engenho")
            .expect("listed");
        assert!(row.is_ambiguous());
        assert!(matches!(
            e.resolve("engenho"),
            Err(ContextError::Ambiguous { files, .. }) if files.len() == 2
        ));
    }

    #[test]
    fn a_stub_after_a_real_declaration_refuses_nothing() {
        let e = Estate {
            files: vec![
                file("/a", vec![decl("x", "x", "https://x")]),
                file("/b", vec![stub("x", "x", "blocked")]),
            ],
            targets: vec![],
        };
        let r = e.resolve("x").expect("the real one wins the merge");
        assert_eq!(r.server, "https://x");
        assert!(e.choices()[0].is_choosable());
    }

    #[test]
    fn an_unknown_credential_plugin_is_refused_on_its_entry_alone() {
        let mut e = workstation();
        e.files[1].contexts[0].exec = Some(ExecSpec {
            command: "/usr/local/bin/mystery-auth".into(),
            args: Vec::new(),
        });
        let rows = e.choices();
        let row = rows.iter().find(|c| c.name == "engenho").expect("listed");
        assert!(
            row.refused
                .as_deref()
                .is_some_and(|r| r.contains("`mystery-auth` refused")),
            "{row:?}"
        );
        assert!(
            rows.iter()
                .any(|c| c.name == "shop-prod" && c.is_choosable())
        );
        assert!(matches!(
            e.resolve("engenho"),
            Err(ContextError::Refused { .. })
        ));
    }

    #[test]
    fn the_known_plugins_resolve_from_their_arguments() {
        let spec = |command: &str, args: &[&str]| ExecSpec {
            command: command.into(),
            args: args.iter().map(|a| (*a).to_owned()).collect(),
        };
        assert_eq!(
            spec("kubelogin", &["--login", "azurecli", "--server-id", "sid"]).plugin(),
            Plugin::Kubelogin {
                server_id: "sid".into(),
                tenant: None
            }
        );
        assert_eq!(spec("aws", &["eks", "get-token"]).plugin(), Plugin::AwsEks);
        assert_eq!(spec("/x/gke-gcloud-auth-plugin", &[]).plugin(), Plugin::Gke);
        assert!(spec("kubelogin", &[]).plugin().refusal().is_some());
    }

    #[test]
    fn a_broken_target_file_is_refused_alone() {
        let mut e = workstation();
        e.targets.push(Target {
            name: "cache".into(),
            file: PathBuf::from("/k/targets/cache"),
            sole: Err("not a kubeconfig".into()),
        });
        let rows = e.choices();
        assert!(
            rows.iter()
                .any(|c| c.name == "shop-prod" && c.is_choosable())
        );
        let cache = rows.iter().find(|c| c.name == "cache").expect("listed");
        assert_eq!(cache.refused.as_deref(), Some("not a kubeconfig"));
    }

    #[test]
    fn an_ambient_context_with_a_different_server_is_not_hidden_by_a_target() {
        let mut e = workstation();
        e.files[3].contexts[1].server = Some("https://elsewhere".into());
        assert!(names(&e).contains(&"cicd-eks".to_owned()));
    }
}
