// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! `check`-family verb builders.
//!
//! Each builder threads a target slot (series or pin) and an input
//! slot through the type system; only the fully-typed shape exposes
//! `run` and `run_with_diagnostics`. Calling `run()` before both
//! slots are filled is a compile-time error.

use std::collections::HashSet;
use std::ffi::OsString;
use std::fmt;
use std::marker::PhantomData;
use std::path::PathBuf;

use backhopper_core::model::batch::BatchPayload;
use backhopper_core::model::check_payload::CheckPayload;
use backhopper_core::model::names::{CommitSha, ProjectName, SeriesName, TagName};

use crate::backend::Backend;
use crate::builder::list_input::ListInput;
use crate::builder::state::{InputState, NoInput, NoTarget, TargetState, WithInput, WithTarget};
use crate::driver::Backhopper;
use crate::envelope::ExecutedInvocation;
use crate::error::DriverError;
use crate::selector::PinSelector;
use crate::stdin::StdinPayload;
use crate::verb::Verb;

/// Top-level handle returned by [`Backhopper::check`].
#[derive(Debug)]
pub struct Check<'a, B: Backend> {
    pub(crate) driver: &'a Backhopper<B>,
}

impl<'a, B: Backend> Check<'a, B> {
    /// Build a `check patch` invocation.
    pub fn patch(&'a self) -> CheckPatchBuilder<'a, B, NoTarget, NoInput> {
        CheckPatchBuilder {
            api: self,
            target: None,
            input: None,
            options: CheckOptions::default(),
            _state: PhantomData,
        }
    }

    /// Build a `check commit` invocation.
    pub fn commit(&'a self) -> CheckCommitBuilder<'a, B, NoTarget, NoInput> {
        CheckCommitBuilder {
            api: self,
            target: None,
            positional: None,
            options: CheckOptions::default(),
            _state: PhantomData,
        }
    }

    /// Build a `check range` invocation.
    pub fn range(&'a self) -> CheckRangeBuilder<'a, B, NoTarget, NoInput> {
        CheckRangeBuilder {
            api: self,
            target: None,
            positional: None,
            options: CheckOptions::default(),
            _state: PhantomData,
        }
    }

    /// Build a `check merge` invocation.
    pub fn merge(&'a self) -> CheckMergeBuilder<'a, B, NoTarget, NoInput> {
        CheckMergeBuilder {
            api: self,
            target: None,
            positional: None,
            options: CheckOptions::default(),
            _state: PhantomData,
        }
    }

    /// Build a `check batch` invocation.
    ///
    /// Batch is merge-aware, so a mixed commit set needs no merge versus
    /// non-merge routing: feed every SHA through one call.
    pub fn batch(&'a self) -> CheckBatchBuilder<'a, B, NoTarget, NoInput> {
        CheckBatchBuilder {
            api: self,
            series: Vec::new(),
            commits: None,
            target_repo_dir_path: None,
            target_ref: None,
            options: CheckOptions::default(),
            _state: PhantomData,
        }
    }
}

/// Knobs shared across every `check` verb. Defaults are off.
#[non_exhaustive]
#[derive(Debug, Clone, Default)]
pub struct CheckOptions {
    /// Pass `--explain`. The format is fixed: the driver forces
    /// `--formatter json`, so there is nothing to select.
    pub explain: bool,
    /// Pass `--suggest-prereqs`.
    pub suggest_prereqs: bool,
    /// Pass `--show-untracked-calls`.
    pub show_untracked_calls: bool,
    /// Pass `--auto-generate`.
    pub auto_generate_missing_snapshots: bool,
    /// Pass `--terse`.
    pub terse: bool,
}

/// Where the patch bytes come from.
#[derive(Debug, Clone)]
pub enum PatchSource {
    /// Bytes piped via stdin.
    Bytes(Vec<u8>),
    /// File path passed as the positional argument.
    File(PathBuf),
}

/// `check patch` builder. See module docs for the type-state shape.
#[must_use = "a builder has no effect until .run() is called"]
pub struct CheckPatchBuilder<'a, B: Backend, T: TargetState, I: InputState> {
    api: &'a Check<'a, B>,
    target: Option<PinSelector>,
    input: Option<PatchSource>,
    options: CheckOptions,
    _state: PhantomData<(T, I)>,
}

impl<B: Backend, T: TargetState, I: InputState> fmt::Debug for CheckPatchBuilder<'_, B, T, I> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CheckPatchBuilder")
            .field("target", &self.target)
            .field("options", &self.options)
            .finish_non_exhaustive()
    }
}

impl<B: Backend, T: TargetState, I: InputState> CheckPatchBuilder<'_, B, T, I> {
    /// Set `--explain`.
    pub fn explain(mut self, explain: bool) -> Self {
        self.options.explain = explain;
        self
    }

    /// Set `--suggest-prereqs`.
    pub fn suggest_prereqs(mut self, on: bool) -> Self {
        self.options.suggest_prereqs = on;
        self
    }

    /// Set `--show-untracked-calls`.
    pub fn show_untracked_calls(mut self, on: bool) -> Self {
        self.options.show_untracked_calls = on;
        self
    }

    /// Set `--auto-generate`.
    pub fn auto_generate_missing_snapshots(mut self, on: bool) -> Self {
        self.options.auto_generate_missing_snapshots = on;
        self
    }

    /// Set `--terse`.
    pub fn terse(mut self, on: bool) -> Self {
        self.options.terse = on;
        self
    }

    /// Replace the entire options struct.
    pub fn with_options(mut self, options: CheckOptions) -> Self {
        self.options = options;
        self
    }
}

impl<'a, B: Backend, I: InputState> CheckPatchBuilder<'a, B, NoTarget, I> {
    /// Set the target to a series or pin.
    pub fn target(
        self,
        selector: impl Into<PinSelector>,
    ) -> CheckPatchBuilder<'a, B, WithTarget, I> {
        CheckPatchBuilder {
            api: self.api,
            target: Some(selector.into()),
            input: self.input,
            options: self.options,
            _state: PhantomData,
        }
    }

    /// Convenience: set `--series NAME`.
    pub fn series(self, name: impl Into<SeriesName>) -> CheckPatchBuilder<'a, B, WithTarget, I> {
        self.target(PinSelector::series(name))
    }

    /// Convenience: set `--project NAME --tag TAG`.
    pub fn pin(
        self,
        project: impl Into<ProjectName>,
        tag: impl Into<TagName>,
    ) -> CheckPatchBuilder<'a, B, WithTarget, I> {
        self.target(PinSelector::pin(project, tag))
    }
}

impl<'a, B: Backend, T: TargetState> CheckPatchBuilder<'a, B, T, NoInput> {
    /// Pipe `bytes` to the CLI's stdin.
    pub fn patch_bytes(self, bytes: impl Into<Vec<u8>>) -> CheckPatchBuilder<'a, B, T, WithInput> {
        CheckPatchBuilder {
            api: self.api,
            target: self.target,
            input: Some(PatchSource::Bytes(bytes.into())),
            options: self.options,
            _state: PhantomData,
        }
    }

    /// Pass `path` as the positional `[PATCH_FILE]` argument.
    pub fn patch_file(self, path: impl Into<PathBuf>) -> CheckPatchBuilder<'a, B, T, WithInput> {
        CheckPatchBuilder {
            api: self.api,
            target: self.target,
            input: Some(PatchSource::File(path.into())),
            options: self.options,
            _state: PhantomData,
        }
    }
}

impl<B: Backend> CheckPatchBuilder<'_, B, WithTarget, WithInput> {
    /// Dispatch the verb and return the parsed payload.
    pub fn run(self) -> Result<CheckPayload, DriverError> {
        self.run_with_diagnostics().map(|(eval, _)| eval)
    }

    /// Dispatch the verb and return the parsed payload paired with
    /// the post-execution diagnostic snapshot.
    pub fn run_with_diagnostics(self) -> Result<(CheckPayload, ExecutedInvocation), DriverError> {
        let args = patch_args(
            &self.options,
            self.target.as_ref().expect("target set"),
            self.input.as_ref().expect("input set"),
        );
        let stdin = match self.input.as_ref().expect("input set") {
            PatchSource::Bytes(b) => StdinPayload::Bytes(b.as_slice()),
            PatchSource::File(_) => StdinPayload::None,
        };
        self.api
            .driver
            .dispatch_check(Verb::CheckPatch, args, stdin)
    }
}

fn patch_args(options: &CheckOptions, target: &PinSelector, source: &PatchSource) -> Vec<OsString> {
    let mut args = Vec::new();
    push_target(&mut args, target);
    push_options(&mut args, options);
    if let PatchSource::File(p) = source {
        args.push(p.as_os_str().to_owned());
    }
    args
}

/// A `check` verb that takes a target plus one positional argument.
/// `commit`, `range`, and `merge` accept the same flags, so they share
/// [`CheckPositionalBuilder`] and differ only through this kind.
pub trait PositionalKind {
    /// The positional value's type.
    type Positional;
    /// The verb dispatched to the CLI.
    const VERB: Verb;
    /// The builder's name, for `Debug`.
    const NAME: &'static str;
    /// Render the positional as one argv element.
    fn to_arg(positional: &Self::Positional) -> OsString;
}

/// `check commit`: the diff of a single commit against its parent.
#[derive(Debug, Clone, Copy)]
pub struct CommitKind;

impl PositionalKind for CommitKind {
    type Positional = CommitSha;
    const VERB: Verb = Verb::CheckCommit;
    const NAME: &'static str = "CheckCommitBuilder";
    fn to_arg(positional: &CommitSha) -> OsString {
        OsString::from(positional.to_string())
    }
}

/// `check range`: a commit range or a merge commit's diff.
#[derive(Debug, Clone, Copy)]
pub struct RangeKind;

impl PositionalKind for RangeKind {
    type Positional = String;
    const VERB: Verb = Verb::CheckRange;
    const NAME: &'static str = "CheckRangeBuilder";
    fn to_arg(positional: &String) -> OsString {
        OsString::from(positional)
    }
}

/// `check merge`: the two-parent diff of a merge commit.
#[derive(Debug, Clone, Copy)]
pub struct MergeKind;

impl PositionalKind for MergeKind {
    type Positional = CommitSha;
    const VERB: Verb = Verb::CheckMerge;
    const NAME: &'static str = "CheckMergeBuilder";
    fn to_arg(positional: &CommitSha) -> OsString {
        OsString::from(positional.to_string())
    }
}

/// Builder for a `check` verb that takes a target and one positional.
/// Reached through the [`CheckCommitBuilder`], [`CheckRangeBuilder`],
/// and [`CheckMergeBuilder`] aliases.
#[must_use = "a builder has no effect until .run() is called"]
pub struct CheckPositionalBuilder<'a, B: Backend, K: PositionalKind, T: TargetState, I: InputState>
{
    api: &'a Check<'a, B>,
    target: Option<PinSelector>,
    positional: Option<K::Positional>,
    options: CheckOptions,
    _state: PhantomData<(K, T, I)>,
}

impl<B: Backend, K: PositionalKind, T: TargetState, I: InputState> fmt::Debug
    for CheckPositionalBuilder<'_, B, K, T, I>
where
    K::Positional: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct(K::NAME)
            .field("target", &self.target)
            .field("positional", &self.positional)
            .field("options", &self.options)
            .finish_non_exhaustive()
    }
}

impl<B: Backend, K: PositionalKind, T: TargetState, I: InputState>
    CheckPositionalBuilder<'_, B, K, T, I>
{
    /// Set `--explain`.
    pub fn explain(mut self, explain: bool) -> Self {
        self.options.explain = explain;
        self
    }

    /// Set `--suggest-prereqs`.
    pub fn suggest_prereqs(mut self, on: bool) -> Self {
        self.options.suggest_prereqs = on;
        self
    }

    /// Set `--show-untracked-calls`.
    pub fn show_untracked_calls(mut self, on: bool) -> Self {
        self.options.show_untracked_calls = on;
        self
    }

    /// Set `--auto-generate`.
    pub fn auto_generate_missing_snapshots(mut self, on: bool) -> Self {
        self.options.auto_generate_missing_snapshots = on;
        self
    }

    /// Set `--terse`.
    pub fn terse(mut self, on: bool) -> Self {
        self.options.terse = on;
        self
    }

    /// Replace the entire options struct.
    pub fn with_options(mut self, options: CheckOptions) -> Self {
        self.options = options;
        self
    }
}

impl<'a, B: Backend, K: PositionalKind, I: InputState>
    CheckPositionalBuilder<'a, B, K, NoTarget, I>
{
    /// Set the target to a series or pin.
    pub fn target(
        self,
        selector: impl Into<PinSelector>,
    ) -> CheckPositionalBuilder<'a, B, K, WithTarget, I> {
        CheckPositionalBuilder {
            api: self.api,
            target: Some(selector.into()),
            positional: self.positional,
            options: self.options,
            _state: PhantomData,
        }
    }

    /// Convenience: set `--series NAME`.
    pub fn series(
        self,
        name: impl Into<SeriesName>,
    ) -> CheckPositionalBuilder<'a, B, K, WithTarget, I> {
        self.target(PinSelector::series(name))
    }

    /// Convenience: set `--project NAME --tag TAG`.
    pub fn pin(
        self,
        project: impl Into<ProjectName>,
        tag: impl Into<TagName>,
    ) -> CheckPositionalBuilder<'a, B, K, WithTarget, I> {
        self.target(PinSelector::pin(project, tag))
    }
}

impl<'a, B: Backend, K: PositionalKind, T: TargetState>
    CheckPositionalBuilder<'a, B, K, T, NoInput>
{
    /// Set the positional argument (the commit, range, or merge SHA).
    pub fn positional(
        self,
        positional: impl Into<K::Positional>,
    ) -> CheckPositionalBuilder<'a, B, K, T, WithInput> {
        CheckPositionalBuilder {
            api: self.api,
            target: self.target,
            positional: Some(positional.into()),
            options: self.options,
            _state: PhantomData,
        }
    }
}

impl<B: Backend, K: PositionalKind> CheckPositionalBuilder<'_, B, K, WithTarget, WithInput> {
    /// Dispatch and return the parsed payload.
    pub fn run(self) -> Result<CheckPayload, DriverError> {
        self.run_with_diagnostics().map(|(eval, _)| eval)
    }

    /// Dispatch and return the parsed payload plus the diagnostic snapshot.
    pub fn run_with_diagnostics(self) -> Result<(CheckPayload, ExecutedInvocation), DriverError> {
        run_target_positional(
            self.api,
            self.target.as_ref().expect("target set"),
            &self.options,
            K::VERB,
            K::to_arg(self.positional.as_ref().expect("positional set")),
        )
    }
}

/// `check commit` builder.
pub type CheckCommitBuilder<'a, B, T, I> = CheckPositionalBuilder<'a, B, CommitKind, T, I>;
/// `check range` builder.
pub type CheckRangeBuilder<'a, B, T, I> = CheckPositionalBuilder<'a, B, RangeKind, T, I>;
/// `check merge` builder.
pub type CheckMergeBuilder<'a, B, T, I> = CheckPositionalBuilder<'a, B, MergeKind, T, I>;

/// `check batch` builder. Series-only (the verb takes no pin target)
/// and merge-aware, so a mixed commit set needs no merge versus
/// non-merge routing. See module docs for the type-state shape.
#[must_use = "a builder has no effect until .run() is called"]
pub struct CheckBatchBuilder<'a, B: Backend, T: TargetState, I: InputState> {
    api: &'a Check<'a, B>,
    series: Vec<SeriesName>,
    commits: Option<ListInput>,
    target_repo_dir_path: Option<PathBuf>,
    target_ref: Option<String>,
    options: CheckOptions,
    _state: PhantomData<(T, I)>,
}

impl<B: Backend, T: TargetState, I: InputState> fmt::Debug for CheckBatchBuilder<'_, B, T, I> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CheckBatchBuilder")
            .field("series", &self.series)
            .field("target_repo_dir_path", &self.target_repo_dir_path)
            .field("target_ref", &self.target_ref)
            .field("options", &self.options)
            .finish_non_exhaustive()
    }
}

impl<B: Backend, T: TargetState, I: InputState> CheckBatchBuilder<'_, B, T, I> {
    /// Set `--explain`.
    pub fn explain(mut self, explain: bool) -> Self {
        self.options.explain = explain;
        self
    }

    /// Set `--terse`.
    pub fn terse(mut self, on: bool) -> Self {
        self.options.terse = on;
        self
    }

    /// Set `--auto-generate`.
    pub fn auto_generate_missing_snapshots(mut self, on: bool) -> Self {
        self.options.auto_generate_missing_snapshots = on;
        self
    }

    /// Replace the entire options struct.
    pub fn with_options(mut self, options: CheckOptions) -> Self {
        self.options = options;
        self
    }

    /// Set `--target-repo-dir-path`: the working clone of the branch
    /// the commits are being backported to. Enables the cross-branch
    /// analyser.
    pub fn target_repo_dir_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.target_repo_dir_path = Some(path.into());
        self
    }

    /// Set `--target-ref` (only meaningful with a target repo dir).
    pub fn target_ref(mut self, git_ref: impl Into<String>) -> Self {
        self.target_ref = Some(git_ref.into());
        self
    }
}

impl<'a, B: Backend, I: InputState> CheckBatchBuilder<'a, B, NoTarget, I> {
    /// Evaluate against a single series.
    pub fn series(self, name: impl Into<SeriesName>) -> CheckBatchBuilder<'a, B, WithTarget, I> {
        self.series_scope([name.into()])
    }

    /// Evaluate against several series in one spawn. The payload's
    /// per-row `series` distinguishes the results.
    pub fn series_scope(
        self,
        names: impl IntoIterator<Item = impl Into<SeriesName>>,
    ) -> CheckBatchBuilder<'a, B, WithTarget, I> {
        CheckBatchBuilder {
            api: self.api,
            series: names.into_iter().map(Into::into).collect(),
            commits: self.commits,
            target_repo_dir_path: self.target_repo_dir_path,
            target_ref: self.target_ref,
            options: self.options,
            _state: PhantomData,
        }
    }
}

impl<'a, B: Backend, T: TargetState> CheckBatchBuilder<'a, B, T, NoInput> {
    /// Frame an iterator of commits as the stdin commit list. Distinct
    /// commits only: duplicates collapse so the row-per-commit contract
    /// holds.
    pub fn commits(
        self,
        commits: impl IntoIterator<Item = impl Into<CommitSha>>,
    ) -> CheckBatchBuilder<'a, B, T, WithInput> {
        let mut seen: HashSet<CommitSha> = HashSet::new();
        let mut buf = Vec::new();
        for c in commits {
            let sha = c.into();
            if seen.insert(sha.clone()) {
                buf.extend_from_slice(sha.to_string().as_bytes());
                buf.push(b'\n');
            }
        }
        self.with_commits(ListInput::Bytes(buf))
    }

    /// Pass a file of one SHA per line as `--commits-file-path`.
    pub fn commits_file_path(
        self,
        path: impl Into<PathBuf>,
    ) -> CheckBatchBuilder<'a, B, T, WithInput> {
        self.with_commits(ListInput::File(path.into()))
    }

    /// Escape hatch: send pre-framed stdin bytes verbatim. The
    /// row-per-commit contract is the caller's to keep here.
    pub fn commits_bytes(
        self,
        bytes: impl Into<Vec<u8>>,
    ) -> CheckBatchBuilder<'a, B, T, WithInput> {
        self.with_commits(ListInput::Bytes(bytes.into()))
    }

    fn with_commits(self, commits: ListInput) -> CheckBatchBuilder<'a, B, T, WithInput> {
        CheckBatchBuilder {
            api: self.api,
            series: self.series,
            commits: Some(commits),
            target_repo_dir_path: self.target_repo_dir_path,
            target_ref: self.target_ref,
            options: self.options,
            _state: PhantomData,
        }
    }
}

impl<B: Backend> CheckBatchBuilder<'_, B, WithTarget, WithInput> {
    /// Dispatch and return the parsed payload.
    pub fn run(self) -> Result<BatchPayload, DriverError> {
        self.run_with_diagnostics().map(|(payload, _)| payload)
    }

    /// Dispatch and return the parsed payload plus the diagnostic snapshot.
    pub fn run_with_diagnostics(self) -> Result<(BatchPayload, ExecutedInvocation), DriverError> {
        let commits = self.commits.as_ref().expect("input set");
        let mut args = Vec::new();
        for s in &self.series {
            args.push(OsString::from("--series"));
            args.push(OsString::from(s.to_string()));
        }
        push_options(&mut args, &self.options);
        // The CLI's `--target-ref` requires `--target-repo-dir-path`, so
        // a ref without a target dir is dropped rather than sent alone.
        if let Some(dir) = &self.target_repo_dir_path {
            args.push(OsString::from("--target-repo-dir-path"));
            args.push(dir.as_os_str().to_owned());
            if let Some(git_ref) = &self.target_ref {
                args.push(OsString::from("--target-ref"));
                args.push(OsString::from(git_ref));
            }
        }
        let stdin = commits.apply("--commits-file-path", &mut args);
        self.api
            .driver
            .dispatch_typed::<BatchPayload>(Verb::CheckBatch, args, stdin)
    }
}

// The commit, range, and merge builders share one dispatch contract:
// target flags, then options, then a single positional, no stdin.
fn run_target_positional<B: Backend>(
    api: &Check<'_, B>,
    target: &PinSelector,
    options: &CheckOptions,
    verb: Verb,
    positional: OsString,
) -> Result<(CheckPayload, ExecutedInvocation), DriverError> {
    let mut args = Vec::new();
    push_target(&mut args, target);
    push_options(&mut args, options);
    args.push(positional);
    api.driver.dispatch_check(verb, args, StdinPayload::None)
}

fn push_target(args: &mut Vec<OsString>, target: &PinSelector) {
    match target {
        PinSelector::Series(s) => {
            args.push(OsString::from("--series"));
            args.push(OsString::from(s.to_string()));
        }
        PinSelector::Pin { project, tag } => {
            args.push(OsString::from("--project"));
            args.push(OsString::from(project.to_string()));
            args.push(OsString::from("--tag"));
            args.push(OsString::from(tag.to_string()));
        }
    }
}

fn push_options(args: &mut Vec<OsString>, options: &CheckOptions) {
    if options.explain {
        args.push(OsString::from("--explain"));
    }
    if options.suggest_prereqs {
        args.push(OsString::from("--suggest-prereqs"));
    }
    if options.show_untracked_calls {
        args.push(OsString::from("--show-untracked-calls"));
    }
    if options.auto_generate_missing_snapshots {
        args.push(OsString::from("--auto-generate"));
    }
    if options.terse {
        args.push(OsString::from("--terse"));
    }
}
