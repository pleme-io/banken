//! declare — a DECLARE becomes a branch, a full manifest, and a pull request.
//!
//! # What this closes
//!
//! DECLARE lowered a full manifest and stopped. The preview is the hard half —
//! `banken_spec::interp::lower_to_full_manifest` already serializes the *whole*
//! spec through serde rather than emitting a targeted patch — and what remained
//! was delivery: resolve which repository and path own the resource, put the
//! manifest on a branch, and open a PR a human reviews.
//!
//! # Why it is a PR and never an apply
//!
//! This is the postigo gate's whole point restated at the delivery layer. A
//! DECLARE mutates **git**; a reconciler (FluxCD, pangea-operator) then
//! converges the cluster. banken never touches the apiserver, so the change is
//! reviewable, revertable and attributable by construction rather than by
//! policy. Three refusals make that structural rather than aspirational:
//!
//! 1. It refuses a head branch equal to the base. Direct-to-main is not a
//!    configuration mistake to be caught in review — there is no plan value
//!    that expresses it.
//! 2. It refuses an empty manifest. An empty document is a well-formed YAML
//!    value (`null`), so a lowering that produced nothing would otherwise open
//!    a PR that deletes the file's contents and reads as intentional.
//!
//! # "Full manifest, never a patch" is not checked here, and that is stronger
//!
//! A first draft of this module refused a non-`Full` [`ManifestScope`]. That
//! check was **vacuous**: `ManifestScope` is a single-variant enum, so a patch
//! scope has no value and the guard could never fire. A guard that cannot fail
//! reads as protection and provides none, which is worse than its absence
//! because it stops anyone looking for the real one.
//!
//! The real guarantee is the type, and the honest tier is
//! **truly-unrepresentable** rather than parse-time-rejected. What this module
//! adds is a forcing function:
//! `adding_a_manifest_scope_variant_must_revisit_this_module` matches the enum
//! exhaustively, so landing a `Partial` variant breaks *this* build and makes
//! the decision explicit rather than letting a patch quietly become expressible.
//!
//! # The branch name is content-addressed, and that is load-bearing
//!
//! The head branch carries a short digest of the manifest bytes. Declaring the
//! same change twice therefore targets the *same* branch and updates the
//! existing PR, rather than opening a second one that a reviewer has to
//! reconcile against the first. An operator retrying after a network failure is
//! the common case, and a tool that answers a retry with a duplicate PR trains
//! people not to retry.
//!
//! # The seam
//!
//! [`DeclareEnv`] is the mockable border (TYPED-SPEC triplet). Every test below
//! runs the whole submit path against a recording mock with no network and no
//! GitHub token, which is what lets the refusals be *proven* rather than
//! asserted about.

use std::path::{Path, PathBuf};

use banken_spec::env::{ChangeRef, DeclareChange};
use banken_spec::error::SpecError;
use banken_spec::types::DeclareTarget;

/// Where a DECLARE lands: a repository and a path inside it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitOpsRoute {
    /// `owner/name`.
    pub repo: String,
    /// The branch a PR targets.
    pub base: String,
    /// The file the manifest is written to, repo-relative.
    pub path: PathBuf,
}

/// The reviewable plan — everything the submit needs, decided before anything
/// is written.
///
/// A struct rather than a sequence of calls so the whole change can be shown to
/// the operator, logged, and diffed *before* it exists anywhere. A plan that
/// cannot be printed cannot be confirmed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeclarePlan {
    /// Where it lands.
    pub route: GitOpsRoute,
    /// The branch the change is pushed to. Never equal to `route.base`.
    pub head: String,
    /// The entire manifest document.
    pub manifest: String,
    /// The commit subject.
    pub commit_message: String,
    /// The PR title.
    pub title: String,
    /// The PR body.
    pub body: String,
}

impl DeclarePlan {
    /// Build a plan from a lowered change, refusing every shape that would
    /// make the resulting PR misleading.
    ///
    /// # Errors
    ///
    /// `SpecError::Interp { phase: "declare" }` when the manifest is empty or
    /// the resolved head branch equals the base. The scope is NOT checked —
    /// see the module docs: `ManifestScope` has one variant, so a patch is
    /// unrepresentable rather than refused.
    pub fn new(route: GitOpsRoute, change: &DeclareChange) -> Result<Self, SpecError> {
        if change.full_manifest.trim().is_empty() {
            return Err(interp(
                "the lowered manifest is empty — an empty document is valid \
                 YAML (`null`), so this would open a PR that blanks the file \
                 and reads as deliberate",
            ));
        }

        let head = format!(
            "banken/declare/{}-{}",
            slug(&change.target),
            digest12(&change.full_manifest),
        );
        if head == route.base {
            return Err(interp(
                "the head branch resolved to the base branch — banken opens a \
                 reviewed PR and never commits to the base directly",
            ));
        }

        let what = route
            .path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("manifest");
        Ok(Self {
            head,
            commit_message: format!("declare: {} via {}", what, slug(&change.target)),
            title: format!("declare: {what}"),
            body: pr_body(&route, change),
            manifest: change.full_manifest.clone(),
            route,
        })
    }
}

/// The mockable border for the git-forge side of a DECLARE.
///
/// Deliberately four small verbs rather than one `open_pr(everything)`: each is
/// separately failable, and a submit that dies after `create_branch` must be
/// distinguishable from one that never started — otherwise a retry cannot know
/// whether it is resuming or duplicating.
pub trait DeclareEnv {
    /// The base branch's current commit sha — what the head branch forks from.
    ///
    /// # Errors
    /// A `SpecError::Interp { phase: "declare" }` when the base cannot be read.
    fn base_sha(&self, repo: &str, base: &str) -> Result<String, SpecError>;

    /// Create the head branch at `from_sha`.
    ///
    /// Implementations MUST treat an already-existing branch as success — the
    /// branch name is content-addressed, so an existing one carries *this*
    /// manifest and re-declaring is a retry, not a collision.
    ///
    /// # Errors
    /// A `SpecError::Interp { phase: "declare" }` on failure.
    fn ensure_branch(&self, repo: &str, head: &str, from_sha: &str) -> Result<(), SpecError>;

    /// Write the whole manifest to `path` on `head`.
    ///
    /// # Errors
    /// A `SpecError::Interp { phase: "declare" }` on failure.
    fn put_file(
        &self,
        repo: &str,
        head: &str,
        path: &Path,
        contents: &str,
        message: &str,
    ) -> Result<(), SpecError>;

    /// Open (or return the existing) pull request from `head` into `base`.
    ///
    /// # Errors
    /// A `SpecError::Interp { phase: "declare" }` on failure.
    fn open_pr(
        &self,
        repo: &str,
        head: &str,
        base: &str,
        title: &str,
        body: &str,
    ) -> Result<ChangeRef, SpecError>;
}

/// Run a plan against a forge.
///
/// The order is fixed and is the safe one: branch, then file, then PR. A
/// failure part-way leaves a branch (harmless, and reused by the retry because
/// the name is content-addressed) rather than a PR describing a file that was
/// never written.
///
/// # Errors
///
/// Whatever the env returns, unchanged — a typed refusal from the forge is the
/// operator's answer, and wrapping it would only bury the reason.
pub fn submit<E: DeclareEnv>(env: &E, plan: &DeclarePlan) -> Result<ChangeRef, SpecError> {
    let base_sha = env.base_sha(&plan.route.repo, &plan.route.base)?;
    env.ensure_branch(&plan.route.repo, &plan.head, &base_sha)?;
    env.put_file(
        &plan.route.repo,
        &plan.head,
        &plan.route.path,
        &plan.manifest,
        &plan.commit_message,
    )?;
    env.open_pr(
        &plan.route.repo,
        &plan.head,
        &plan.route.base,
        &plan.title,
        &plan.body,
    )
}

/// Resolve which repository and path own a declare target.
///
/// # The honest limit
///
/// Only [`DeclareTarget::FluxHelmValues`] carries its own path — the authored
/// rail names the `release.yaml` it edits — so only it resolves without
/// external knowledge. The other four rails name a *reference* (a band, a
/// promessa, a stack, a mergeflow) whose owning repository is a fact about the
/// fleet's `GitOps` tree that banken does not hold. Those return a typed refusal
/// naming what is missing rather than guessing at a path, because a DECLARE
/// written to a plausible-but-wrong file is a PR that looks correct and
/// reconciles nothing.
///
/// # Errors
///
/// `SpecError::Interp { phase: "declare" }` for a rail whose owning repository
/// banken cannot resolve.
pub fn route_for(target: &DeclareTarget, repo: &str, base: &str) -> Result<GitOpsRoute, SpecError> {
    match target {
        DeclareTarget::FluxHelmValues { release_path } => Ok(GitOpsRoute {
            repo: repo.to_owned(),
            base: base.to_owned(),
            path: release_path.clone(),
        }),
        other => Err(interp(&format!(
            "the `{}` rail names a reference, not a path — banken cannot \
             resolve which repository owns it, and writing to a guessed path \
             would open a PR that looks right and reconciles nothing. \
             pending-banken: declare-rail-routing",
            slug(other),
        ))),
    }
}

/// The real forge: GitHub, over its REST API, in-process.
///
/// # No shell, and no local clone either
///
/// The fleet's NO-SHELL law rules out `git`/`gh` subprocesses, and `octocrab`
/// is the fleet's established GitHub client (five repos). But the sharper
/// reason to use the API rather than `gix` is that a DECLARE needs no working
/// tree at all: create-ref, put-contents and create-pull are three HTTP calls
/// against a repository banken never has to clone, on a machine that may not
/// have room for it. A cluster navigator is not a git client.
///
/// # Authentication
///
/// A `GITHUB_TOKEN` from the environment. Deliberately not a config field:
/// banken's config is authored in Lisp and lands in a readable file, and a
/// token in one is a token in a backup and in a screen share. `cofre` is the
/// fleet's typed secret plane and is the destination —
/// `pending-banken: declare-token-via-cofre`.
#[cfg(feature = "gitops")]
pub struct GitHubForge {
    client: octocrab::Octocrab,
    runtime: tokio::runtime::Handle,
}

#[cfg(feature = "gitops")]
impl GitHubForge {
    /// Build a forge from `GITHUB_TOKEN`, bound to the current runtime.
    ///
    /// # Errors
    ///
    /// `SpecError::Interp { phase: "declare" }` when no token is present or the
    /// client cannot be built. A refusal rather than an anonymous client: an
    /// unauthenticated GitHub client can *read*, so it would fail at the first
    /// write with a permissions error that reads like a repository problem
    /// rather than a missing credential.
    pub fn from_env() -> Result<Self, SpecError> {
        let token = std::env::var("GITHUB_TOKEN").map_err(|_| {
            interp(
                "no GITHUB_TOKEN in the environment — banken will not open a PR \
                 anonymously, because an anonymous client can read and would \
                 fail at the first write with an error that reads like a \
                 repository problem rather than a missing credential",
            )
        })?;
        let client = octocrab::Octocrab::builder()
            .personal_token(token)
            .build()
            .map_err(|e| interp(&format!("cannot build a GitHub client: {e}")))?;
        let runtime = tokio::runtime::Handle::try_current()
            .map_err(|e| interp(&format!("no tokio runtime for the GitHub client: {e}")))?;
        Ok(Self { client, runtime })
    }

    /// Split `owner/name`, refusing anything else.
    fn split(repo: &str) -> Result<(&str, &str), SpecError> {
        repo.split_once('/')
            .filter(|(owner, name)| !owner.is_empty() && !name.is_empty())
            .ok_or_else(|| interp(&format!("`{repo}` is not an `owner/name` repository")))
    }

    /// Run a future on the ambient runtime.
    ///
    /// `block_in_place` rather than `block_on`: this is called from inside a
    /// runtime worker (the TUI's), and `block_on` there panics.
    fn block<F: std::future::Future>(&self, fut: F) -> F::Output {
        let handle = self.runtime.clone();
        tokio::task::block_in_place(move || handle.block_on(fut))
    }
}

#[cfg(feature = "gitops")]
impl DeclareEnv for GitHubForge {
    fn base_sha(&self, repo: &str, base: &str) -> Result<String, SpecError> {
        let (owner, name) = Self::split(repo)?;
        let reference = octocrab::params::repos::Reference::Branch(base.to_owned());
        let got = self.block(self.client.repos(owner, name).get_ref(&reference));
        match got.map_err(|e| interp(&format!("cannot read `{repo}` branch `{base}`: {e}")))? {
            octocrab::models::repos::Ref {
                object: octocrab::models::repos::Object::Commit { sha, .. },
                ..
            } => Ok(sha),
            // A branch ref pointing at a tag object is legal git and useless
            // here — naming it beats a generic failure three calls later.
            other => Err(interp(&format!(
                "`{repo}` branch `{base}` does not point at a commit: {:?}",
                other.object
            ))),
        }
    }

    fn ensure_branch(&self, repo: &str, head: &str, from_sha: &str) -> Result<(), SpecError> {
        let (owner, name) = Self::split(repo)?;
        let created = self.block(self.client.repos(owner, name).create_ref(
            &octocrab::params::repos::Reference::Branch(head.to_owned()),
            from_sha,
        ));
        match created {
            Ok(_) => Ok(()),
            // 422 is "reference already exists". The branch name is
            // content-addressed, so an existing one carries THIS manifest —
            // that is a retry resuming, not a collision, and treating it as an
            // error would make every retry fail.
            Err(octocrab::Error::GitHub { source, .. })
                if source.status_code == http::StatusCode::UNPROCESSABLE_ENTITY =>
            {
                Ok(())
            }
            Err(e) => Err(interp(&format!("cannot create branch `{head}`: {e}"))),
        }
    }

    fn put_file(
        &self,
        repo: &str,
        head: &str,
        path: &Path,
        contents: &str,
        message: &str,
    ) -> Result<(), SpecError> {
        let (owner, name) = Self::split(repo)?;
        let p = path
            .to_str()
            .ok_or_else(|| interp("the manifest path is not valid UTF-8"))?;
        let handler = self.client.repos(owner, name);

        // An UPDATE needs the blob's current sha; a CREATE must not carry one.
        // Reading first is what makes this work on both a new file and an
        // existing one — without it, the second declare against the same path
        // fails with "sha wasn't supplied" and reads as a permissions problem.
        let existing = self.block(handler.get_content().path(p).r#ref(head).send());
        let sha = existing
            .ok()
            .and_then(|c| c.items.into_iter().next())
            .map(|i| i.sha);

        // Two different calls, not one with an optional sha: octocrab models
        // create and update separately because GitHub does. Passing a sha for a
        // file that does not exist is a 422, and omitting one for a file that
        // does is a different 422 — both of which read as permission problems.
        let builder = match sha.as_deref() {
            Some(sha) => handler.update_file(p, message, contents, sha),
            None => handler.create_file(p, message, contents),
        };
        self.block(builder.branch(head).send())
            .map(|_| ())
            .map_err(|e| interp(&format!("cannot write `{p}` on `{head}`: {e}")))
    }

    fn open_pr(
        &self,
        repo: &str,
        head: &str,
        base: &str,
        title: &str,
        body: &str,
    ) -> Result<ChangeRef, SpecError> {
        let (owner, name) = Self::split(repo)?;
        let opened = self.block(
            self.client
                .pulls(owner, name)
                .create(title, head, base)
                .body(body)
                .send(),
        );
        match opened {
            Ok(pr) => Ok(ChangeRef(
                pr.html_url
                    .map_or_else(|| format!("{repo}#{}", pr.number), |u| u.to_string()),
            )),
            // A PR already open from this head is the retry case again — the
            // put_file above updated it, so the existing PR is the answer.
            Err(octocrab::Error::GitHub { source, .. })
                if source.status_code == http::StatusCode::UNPROCESSABLE_ENTITY =>
            {
                let found = self.block(
                    self.client
                        .pulls(owner, name)
                        .list()
                        .head(format!("{owner}:{head}"))
                        .send(),
                );
                found
                    .ok()
                    .and_then(|page| page.items.into_iter().next())
                    .and_then(|pr| pr.html_url.map(|u| ChangeRef(u.to_string())))
                    .ok_or_else(|| {
                        interp(&format!(
                            "a pull request already exists for `{head}` but could \
                             not be read back — the branch and manifest ARE \
                             written; find the PR on `{repo}`"
                        ))
                    })
            }
            Err(e) => Err(interp(&format!("cannot open a pull request: {e}"))),
        }
    }
}

fn pr_body(route: &GitOpsRoute, change: &DeclareChange) -> String {
    let mut b = String::new();
    b.push_str("Opened by **banken** — an OBSERVE-first cluster navigator.\n\n");
    b.push_str("This is a DECLARE: the whole manifest, written to git, for a\n");
    b.push_str("reconciler to converge. banken did not touch the cluster.\n\n");
    b.push_str("| | |\n|---|---|\n");
    b.push_str("| rail | `");
    b.push_str(slug(&change.target));
    b.push_str("` |\n| path | `");
    b.push_str(&route.path.display().to_string());
    b.push_str("` |\n| scope | `full-manifest` |\n\n");
    b.push_str("The manifest is the **entire** document, not a patch — a patch\n");
    b.push_str("drops sibling fields silently, and the drop is invisible here.\n");
    b
}

fn slug(target: &DeclareTarget) -> &'static str {
    target.kind().label()
}

/// A short, stable, hex digest of the manifest — the content address the head
/// branch is named for.
///
/// FNV-1a rather than a cryptographic hash: this names a branch, it does not
/// authenticate anything, and pulling a hashing crate in for a branch suffix
/// would be a dependency bought with no security to spend it on. Collisions
/// cost a shared branch between two different manifests, which the PR diff
/// makes immediately visible.
fn digest12(s: &str) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.as_bytes() {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{h:012x}")
}

fn interp(message: &str) -> SpecError {
    SpecError::Interp {
        phase: "declare".into(),
        message: message.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use banken_spec::types::ManifestScope;
    use std::cell::RefCell;

    /// A recording forge — no network, no token, every call captured.
    #[derive(Default)]
    struct MockForge {
        calls: RefCell<Vec<String>>,
        fail_at: Option<&'static str>,
    }

    impl MockForge {
        fn failing_at(step: &'static str) -> Self {
            Self {
                calls: RefCell::new(Vec::new()),
                fail_at: Some(step),
            }
        }
        fn note(&self, step: &'static str, detail: &str) -> Result<(), SpecError> {
            self.calls.borrow_mut().push(format!("{step}:{detail}"));
            if self.fail_at == Some(step) {
                return Err(interp("mock failure"));
            }
            Ok(())
        }
        fn calls(&self) -> Vec<String> {
            self.calls.borrow().clone()
        }
    }

    impl DeclareEnv for MockForge {
        fn base_sha(&self, repo: &str, base: &str) -> Result<String, SpecError> {
            self.note("base_sha", &format!("{repo}@{base}"))?;
            Ok("basesha".into())
        }
        fn ensure_branch(&self, _r: &str, head: &str, from: &str) -> Result<(), SpecError> {
            self.note("ensure_branch", &format!("{head}<-{from}"))
        }
        fn put_file(
            &self,
            _r: &str,
            head: &str,
            path: &Path,
            contents: &str,
            _m: &str,
        ) -> Result<(), SpecError> {
            self.note(
                "put_file",
                &format!("{head}:{}:{}b", path.display(), contents.len()),
            )
        }
        fn open_pr(
            &self,
            _r: &str,
            head: &str,
            base: &str,
            _t: &str,
            _b: &str,
        ) -> Result<ChangeRef, SpecError> {
            self.note("open_pr", &format!("{head}->{base}"))?;
            Ok(ChangeRef(format!("https://example.invalid/pr/{head}")))
        }
    }

    fn change(manifest: &str, scope: ManifestScope) -> DeclareChange {
        DeclareChange {
            target: DeclareTarget::FluxHelmValues {
                release_path: PathBuf::from("clusters/alpha/apps/api/release.yaml"),
            },
            scope,
            full_manifest: manifest.to_owned(),
        }
    }

    fn route() -> GitOpsRoute {
        GitOpsRoute {
            repo: "pleme-io/k8s".into(),
            base: "main".into(),
            path: PathBuf::from("clusters/alpha/apps/api/release.yaml"),
        }
    }

    #[test]
    fn a_plan_submits_branch_then_file_then_pr() {
        let forge = MockForge::default();
        let plan =
            DeclarePlan::new(route(), &change("replicas: 3\n", ManifestScope::Full)).expect("plan");
        let r = submit(&forge, &plan).expect("submitted");

        let calls = forge.calls();
        assert_eq!(calls.len(), 4, "{calls:?}");
        assert!(calls[0].starts_with("base_sha:"), "{calls:?}");
        assert!(calls[1].starts_with("ensure_branch:"), "{calls:?}");
        assert!(calls[2].starts_with("put_file:"), "{calls:?}");
        assert!(calls[3].starts_with("open_pr:"), "{calls:?}");
        assert!(r.0.contains("banken/declare/"), "{r:?}");
    }

    /// **The order is the safety property.** A submit that dies before the file
    /// is written must NOT have opened a PR — a PR describing a file that was
    /// never written is worse than no PR, because a reviewer approves it.
    #[test]
    fn a_failure_before_the_file_never_opens_a_pull_request() {
        let forge = MockForge::failing_at("put_file");
        let plan =
            DeclarePlan::new(route(), &change("replicas: 3\n", ManifestScope::Full)).expect("plan");
        submit(&forge, &plan).expect_err("must fail");
        assert!(
            !forge.calls().iter().any(|c| c.starts_with("open_pr")),
            "a PR was opened for a file that was never written: {:?}",
            forge.calls(),
        );
    }

    /// **Direct-to-main has no representation.** Not a check in review — the
    /// plan value that would express it cannot be constructed.
    #[test]
    fn a_head_equal_to_the_base_is_refused_at_plan_time() {
        let c = change("replicas: 3\n", ManifestScope::Full);
        let plan = DeclarePlan::new(route(), &c).expect("plan");
        // The derived head is never the base — that is the property.
        assert_ne!(plan.head, plan.route.base);
        assert!(plan.head.starts_with("banken/declare/"), "{}", plan.head);

        // And if a route DID name the derived branch as its base, the plan
        // refuses rather than committing onto it.
        let mut r = route();
        r.base = plan.head.clone();
        let e = DeclarePlan::new(r, &c).expect_err("must refuse");
        assert!(e.to_string().contains("reviewed PR"), "{e}");
    }

    /// **The forcing function for "full manifest, never a patch".**
    ///
    /// The rule is not enforced by a runtime check here — `ManifestScope` has
    /// exactly one variant, so a patch has no value and any such check would be
    /// vacuous. The guarantee is the type; this is what makes adding a second
    /// variant a decision rather than an accident. The exhaustive match fails
    /// to compile the moment `Partial` lands, and whoever lands it has to come
    /// here and say what a partial DECLARE should do.
    #[test]
    fn adding_a_manifest_scope_variant_must_revisit_this_module() {
        let scope = ManifestScope::Full;
        match scope {
            // Adding an arm here is the point. If this match stops being
            // exhaustive, the compiler sends the author to this comment.
            ManifestScope::Full => {}
        }
        // And the plan carries the whole document through untouched — no
        // trimming, no re-serialization, no chance to drop a sibling field.
        let manifest = "apiVersion: v1\nkind: X\nspec:\n  replicas: 3\n  keep: me\n";
        let plan = DeclarePlan::new(route(), &change(manifest, ManifestScope::Full)).expect("plan");
        assert_eq!(
            plan.manifest, manifest,
            "the manifest must pass through byte-for-byte"
        );
    }

    /// An empty manifest is refused. `null` is valid YAML, so this would
    /// otherwise open a PR that blanks the file and reads as deliberate.
    #[test]
    fn an_empty_manifest_is_refused() {
        for empty in ["", "   ", "\n\n"] {
            let e = DeclarePlan::new(route(), &change(empty, ManifestScope::Full))
                .expect_err("must refuse");
            assert!(e.to_string().contains("empty"), "{e}");
        }
    }

    /// **The retry property.** The same manifest yields the same branch, so
    /// re-declaring updates the existing PR instead of opening a second one a
    /// reviewer has to reconcile. A different manifest yields a different one.
    #[test]
    fn the_branch_is_content_addressed_so_a_retry_is_not_a_duplicate() {
        let a =
            DeclarePlan::new(route(), &change("replicas: 3\n", ManifestScope::Full)).expect("plan");
        let again =
            DeclarePlan::new(route(), &change("replicas: 3\n", ManifestScope::Full)).expect("plan");
        let other =
            DeclarePlan::new(route(), &change("replicas: 4\n", ManifestScope::Full)).expect("plan");

        assert_eq!(a.head, again.head, "a retry must reuse the branch");
        assert_ne!(a.head, other.head, "a different change must not");
    }

    /// The PR body must SAY the cluster was not touched. A reviewer landing on
    /// a machine-opened PR needs to know what it did and did not do without
    /// reading banken's source.
    #[test]
    fn the_pull_request_says_what_it_did_and_did_not_do() {
        let plan =
            DeclarePlan::new(route(), &change("replicas: 3\n", ManifestScope::Full)).expect("plan");
        assert!(
            plan.body.contains("did not touch the cluster"),
            "{}",
            plan.body
        );
        assert!(plan.body.contains("full-manifest"), "{}", plan.body);
        assert!(plan.body.contains("not a patch"), "{}", plan.body);
        assert!(plan.body.contains("release.yaml"), "{}", plan.body);
    }

    /// Only the rail that carries its own path resolves. The rest REFUSE and
    /// name what is missing — a DECLARE written to a guessed path opens a PR
    /// that looks correct and reconciles nothing, which is the worst available
    /// outcome.
    #[test]
    fn an_unroutable_rail_refuses_rather_than_guessing_a_path() {
        use banken_spec::types::{BandRef, MergeflowRef, PromessaRef, StackRef};

        let ok = route_for(
            &DeclareTarget::FluxHelmValues {
                release_path: PathBuf::from("clusters/alpha/apps/api/release.yaml"),
            },
            "pleme-io/k8s",
            "main",
        )
        .expect("the flux rail carries its own path");
        assert_eq!(
            ok.path,
            PathBuf::from("clusters/alpha/apps/api/release.yaml")
        );

        for unroutable in [
            DeclareTarget::BreatheBand {
                band: BandRef("api-mem".into()),
            },
            DeclareTarget::ViggySetpoint {
                promessa: PromessaRef("p".into()),
            },
            DeclareTarget::GalhoChange {
                stack: StackRef("s".into()),
            },
            DeclareTarget::EclusaMergeflow {
                flow: MergeflowRef("f".into()),
            },
        ] {
            let e =
                route_for(&unroutable, "pleme-io/k8s", "main").expect_err("must refuse to guess");
            assert!(
                e.to_string().contains("reconciles nothing"),
                "the refusal must say WHY: {e}",
            );
        }
    }
}
