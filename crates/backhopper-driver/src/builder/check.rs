// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! `check`-family verb builders.
//!
//! Each builder threads a target slot (series or pin) and an input
//! slot through the type system; only the fully-typed shape exposes
//! `run` and `run_with_diagnostics`. Calling `run()` before both
//! slots are filled is a compile-time error.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::ffi::OsString;
use std::fmt;
use std::marker::PhantomData;
use std::path::PathBuf;

use backhopper_core::model::batch::BatchPayload;
use backhopper_core::model::evaluation::{
    AggregateVerdict, BehaviourModuleMissingFinding, HeaderFileMissingFinding,
    SeriesEvaluationView, TestModuleSymbolMissingFinding, VersionedMachineSnapshotMissingFinding,
    WireConstantBindingsMissingFinding,
};
use backhopper_core::model::names::{
    CommitSha, ModuleName, ProjectName, RelativePath, SeriesName, TagName,
};
use backhopper_core::model::pin::Pin;
use backhopper_core::model::summary::VerdictKind;
use backhopper_core::model::verdict::{Diagnostics, PinVerdict, Reason, SeriesVerdict};
use serde::Deserialize;

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
            commit: None,
            options: CheckOptions::default(),
            _state: PhantomData,
        }
    }

    /// Build a `check range` invocation.
    pub fn range(&'a self) -> CheckRangeBuilder<'a, B, NoTarget, NoInput> {
        CheckRangeBuilder {
            api: self,
            target: None,
            range: None,
            options: CheckOptions::default(),
            _state: PhantomData,
        }
    }

    /// Build a `check merge` invocation.
    pub fn merge(&'a self) -> CheckMergeBuilder<'a, B, NoTarget, NoInput> {
        CheckMergeBuilder {
            api: self,
            target: None,
            merge_commit: None,
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
    /// `--explain` format.
    pub explain: ExplainFormat,
    /// Pass `--suggest-prereqs`.
    pub suggest_prereqs: bool,
    /// Pass `--show-untracked-calls`.
    pub show_untracked_calls: bool,
    /// Pass `--auto-generate`.
    pub auto_generate_missing_snapshots: bool,
    /// Pass `--terse`.
    pub terse: bool,
}

/// `--explain` mode.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ExplainFormat {
    /// Off; no `--explain` flag.
    #[default]
    Off,
    /// `--explain` (text default).
    Text,
    /// `--explain --formatter markdown` overlay.
    Markdown,
    /// `--explain --formatter json` overlay.
    Json,
}

impl ExplainFormat {
    // The CLI's `--explain` is a boolean: the format comes from the
    // driver-forced `--formatter json`, so any non-Off value emits the
    // bare flag.
    fn is_on(self) -> bool {
        !matches!(self, Self::Off)
    }
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
    /// Set `--explain MODE`.
    pub fn explain(mut self, explain: ExplainFormat) -> Self {
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
    pub fn run(self) -> Result<SeriesEvaluation, DriverError> {
        self.run_with_diagnostics().map(|(eval, _)| eval)
    }

    /// Dispatch the verb and return the parsed payload paired with
    /// the post-execution diagnostic snapshot.
    pub fn run_with_diagnostics(
        self,
    ) -> Result<(SeriesEvaluation, ExecutedInvocation), DriverError> {
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

/// `check commit` builder.
#[must_use = "a builder has no effect until .run() is called"]
pub struct CheckCommitBuilder<'a, B: Backend, T: TargetState, I: InputState> {
    api: &'a Check<'a, B>,
    target: Option<PinSelector>,
    commit: Option<CommitSha>,
    options: CheckOptions,
    _state: PhantomData<(T, I)>,
}

impl<B: Backend, T: TargetState, I: InputState> fmt::Debug for CheckCommitBuilder<'_, B, T, I> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CheckCommitBuilder")
            .field("target", &self.target)
            .field("commit", &self.commit)
            .field("options", &self.options)
            .finish_non_exhaustive()
    }
}

impl<B: Backend, T: TargetState, I: InputState> CheckCommitBuilder<'_, B, T, I> {
    /// Set `--explain MODE`.
    pub fn explain(mut self, explain: ExplainFormat) -> Self {
        self.options.explain = explain;
        self
    }

    /// Set `--suggest-prereqs`.
    pub fn suggest_prereqs(mut self, on: bool) -> Self {
        self.options.suggest_prereqs = on;
        self
    }

    /// Set `--terse`.
    pub fn terse(mut self, on: bool) -> Self {
        self.options.terse = on;
        self
    }
}

impl<'a, B: Backend, I: InputState> CheckCommitBuilder<'a, B, NoTarget, I> {
    /// Set the target.
    pub fn target(
        self,
        selector: impl Into<PinSelector>,
    ) -> CheckCommitBuilder<'a, B, WithTarget, I> {
        CheckCommitBuilder {
            api: self.api,
            target: Some(selector.into()),
            commit: self.commit,
            options: self.options,
            _state: PhantomData,
        }
    }

    /// Convenience: set `--series NAME`.
    pub fn series(self, name: impl Into<SeriesName>) -> CheckCommitBuilder<'a, B, WithTarget, I> {
        self.target(PinSelector::series(name))
    }

    /// Convenience: set `--project NAME --tag TAG`.
    pub fn pin(
        self,
        project: impl Into<ProjectName>,
        tag: impl Into<TagName>,
    ) -> CheckCommitBuilder<'a, B, WithTarget, I> {
        self.target(PinSelector::pin(project, tag))
    }
}

impl<'a, B: Backend, T: TargetState> CheckCommitBuilder<'a, B, T, NoInput> {
    /// Set the positional `COMMIT` argument.
    pub fn commit(self, commit: impl Into<CommitSha>) -> CheckCommitBuilder<'a, B, T, WithInput> {
        CheckCommitBuilder {
            api: self.api,
            target: self.target,
            commit: Some(commit.into()),
            options: self.options,
            _state: PhantomData,
        }
    }
}

impl<B: Backend> CheckCommitBuilder<'_, B, WithTarget, WithInput> {
    /// Dispatch and return the parsed payload.
    pub fn run(self) -> Result<SeriesEvaluation, DriverError> {
        self.run_with_diagnostics().map(|(eval, _)| eval)
    }

    /// Dispatch and return the parsed payload plus the diagnostic snapshot.
    pub fn run_with_diagnostics(
        self,
    ) -> Result<(SeriesEvaluation, ExecutedInvocation), DriverError> {
        run_target_positional(
            self.api,
            self.target.as_ref().expect("target set"),
            &self.options,
            Verb::CheckCommit,
            OsString::from(self.commit.as_ref().expect("commit set").to_string()),
        )
    }
}

/// `check range` builder.
#[must_use = "a builder has no effect until .run() is called"]
pub struct CheckRangeBuilder<'a, B: Backend, T: TargetState, I: InputState> {
    api: &'a Check<'a, B>,
    target: Option<PinSelector>,
    range: Option<String>,
    options: CheckOptions,
    _state: PhantomData<(T, I)>,
}

impl<B: Backend, T: TargetState, I: InputState> fmt::Debug for CheckRangeBuilder<'_, B, T, I> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CheckRangeBuilder")
            .field("target", &self.target)
            .field("range", &self.range)
            .field("options", &self.options)
            .finish_non_exhaustive()
    }
}

impl<B: Backend, T: TargetState, I: InputState> CheckRangeBuilder<'_, B, T, I> {
    /// Set `--explain MODE`.
    pub fn explain(mut self, explain: ExplainFormat) -> Self {
        self.options.explain = explain;
        self
    }

    /// Set `--terse`.
    pub fn terse(mut self, on: bool) -> Self {
        self.options.terse = on;
        self
    }
}

impl<'a, B: Backend, I: InputState> CheckRangeBuilder<'a, B, NoTarget, I> {
    /// Set the target.
    pub fn target(
        self,
        selector: impl Into<PinSelector>,
    ) -> CheckRangeBuilder<'a, B, WithTarget, I> {
        CheckRangeBuilder {
            api: self.api,
            target: Some(selector.into()),
            range: self.range,
            options: self.options,
            _state: PhantomData,
        }
    }

    /// Convenience: set `--series NAME`.
    pub fn series(self, name: impl Into<SeriesName>) -> CheckRangeBuilder<'a, B, WithTarget, I> {
        self.target(PinSelector::series(name))
    }
}

impl<'a, B: Backend, T: TargetState> CheckRangeBuilder<'a, B, T, NoInput> {
    /// Set the positional `RANGE` argument.
    pub fn range(self, range: impl Into<String>) -> CheckRangeBuilder<'a, B, T, WithInput> {
        CheckRangeBuilder {
            api: self.api,
            target: self.target,
            range: Some(range.into()),
            options: self.options,
            _state: PhantomData,
        }
    }
}

impl<B: Backend> CheckRangeBuilder<'_, B, WithTarget, WithInput> {
    /// Dispatch and return the parsed payload.
    pub fn run(self) -> Result<SeriesEvaluation, DriverError> {
        self.run_with_diagnostics().map(|(eval, _)| eval)
    }

    /// Dispatch and return the parsed payload plus the diagnostic snapshot.
    pub fn run_with_diagnostics(
        self,
    ) -> Result<(SeriesEvaluation, ExecutedInvocation), DriverError> {
        run_target_positional(
            self.api,
            self.target.as_ref().expect("target set"),
            &self.options,
            Verb::CheckRange,
            OsString::from(self.range.as_ref().expect("range set")),
        )
    }
}

/// `check merge` builder.
#[must_use = "a builder has no effect until .run() is called"]
pub struct CheckMergeBuilder<'a, B: Backend, T: TargetState, I: InputState> {
    api: &'a Check<'a, B>,
    target: Option<PinSelector>,
    merge_commit: Option<CommitSha>,
    options: CheckOptions,
    _state: PhantomData<(T, I)>,
}

impl<B: Backend, T: TargetState, I: InputState> fmt::Debug for CheckMergeBuilder<'_, B, T, I> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CheckMergeBuilder")
            .field("target", &self.target)
            .field("merge_commit", &self.merge_commit)
            .field("options", &self.options)
            .finish_non_exhaustive()
    }
}

impl<B: Backend, T: TargetState, I: InputState> CheckMergeBuilder<'_, B, T, I> {
    /// Set `--terse`.
    pub fn terse(mut self, on: bool) -> Self {
        self.options.terse = on;
        self
    }
}

impl<'a, B: Backend, I: InputState> CheckMergeBuilder<'a, B, NoTarget, I> {
    /// Set the target.
    pub fn target(
        self,
        selector: impl Into<PinSelector>,
    ) -> CheckMergeBuilder<'a, B, WithTarget, I> {
        CheckMergeBuilder {
            api: self.api,
            target: Some(selector.into()),
            merge_commit: self.merge_commit,
            options: self.options,
            _state: PhantomData,
        }
    }

    /// Convenience: set `--series NAME`.
    pub fn series(self, name: impl Into<SeriesName>) -> CheckMergeBuilder<'a, B, WithTarget, I> {
        self.target(PinSelector::series(name))
    }
}

impl<'a, B: Backend, T: TargetState> CheckMergeBuilder<'a, B, T, NoInput> {
    /// Set the positional `MERGE-SHA` argument.
    pub fn merge_commit(
        self,
        commit: impl Into<CommitSha>,
    ) -> CheckMergeBuilder<'a, B, T, WithInput> {
        CheckMergeBuilder {
            api: self.api,
            target: self.target,
            merge_commit: Some(commit.into()),
            options: self.options,
            _state: PhantomData,
        }
    }
}

impl<B: Backend> CheckMergeBuilder<'_, B, WithTarget, WithInput> {
    /// Dispatch and return the parsed payload.
    pub fn run(self) -> Result<SeriesEvaluation, DriverError> {
        self.run_with_diagnostics().map(|(eval, _)| eval)
    }

    /// Dispatch and return the parsed payload plus the diagnostic snapshot.
    pub fn run_with_diagnostics(
        self,
    ) -> Result<(SeriesEvaluation, ExecutedInvocation), DriverError> {
        run_target_positional(
            self.api,
            self.target.as_ref().expect("target set"),
            &self.options,
            Verb::CheckMerge,
            OsString::from(
                self.merge_commit
                    .as_ref()
                    .expect("merge_commit set")
                    .to_string(),
            ),
        )
    }
}

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
    pub fn explain(mut self, explain: ExplainFormat) -> Self {
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
) -> Result<(SeriesEvaluation, ExecutedInvocation), DriverError> {
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
    if options.explain.is_on() {
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

/// Parsed payload of every `check`-family verb.
#[non_exhaustive]
#[derive(Debug, Clone, Deserialize)]
#[must_use = "a SeriesEvaluation describes what backhopper found; \
              act on it or assert it"]
pub struct SeriesEvaluation {
    /// What the verb was queried against (a series or a pin).
    pub queried_against: QueriedAgainst,
    /// Per-pin verdicts plus summary.
    pub results: SeriesVerdict,
    /// Series-wide diagnostics (untracked calls, suggested suites).
    #[serde(default)]
    pub diagnostics: Diagnostics,
    /// Projects excluded from the tracked-dependency tally. `None` when
    /// the producing binary predates the field.
    #[serde(default)]
    pub self_projects: Option<BTreeSet<ProjectName>>,
}

impl SeriesEvaluation {
    /// A borrowed view exposing every query and finding accessor. The same
    /// view is reachable from a batch row via `BatchResult::evaluation`, so
    /// both check paths read alike.
    #[must_use]
    pub fn view(&self) -> SeriesEvaluationView<'_> {
        SeriesEvaluationView::new(&self.results, &self.diagnostics)
    }

    /// Self-excluded tracked-dependency count from the wire-recorded
    /// self-projects. `None` when the producer predates the field.
    #[must_use]
    pub fn tracked_refs(&self) -> Option<u32> {
        self.self_projects
            .as_ref()
            .map(|projects| self.view().tracked_refs(projects))
    }

    /// See [`SeriesEvaluationView::worst_verdict`].
    #[must_use]
    pub fn worst_verdict(&self) -> AggregateVerdict {
        self.view().worst_verdict()
    }

    /// See [`SeriesEvaluationView::has_blocking_reason`].
    #[must_use]
    pub fn has_blocking_reason(&self) -> bool {
        self.view().has_blocking_reason()
    }

    /// See [`SeriesEvaluationView::pins_in`].
    pub fn pins_in(&self, verdict: VerdictKind) -> impl Iterator<Item = &PinVerdict> {
        self.view().pins_in(verdict)
    }

    /// See [`SeriesEvaluationView::reasons_for`].
    pub fn reasons_for(&self, pin: &Pin) -> impl Iterator<Item = &Reason> {
        self.view().reasons_for(pin)
    }

    /// See [`SeriesEvaluationView::pin_by_project`].
    #[must_use]
    pub fn pin_by_project(&self, project: &ProjectName) -> Option<&PinVerdict> {
        self.view().pin_by_project(project)
    }

    /// See [`SeriesEvaluationView::missing_test_modules`].
    #[must_use]
    pub fn missing_test_modules(&self) -> &BTreeMap<RelativePath, BTreeMap<ModuleName, usize>> {
        self.view().missing_test_modules()
    }

    /// See [`SeriesEvaluationView::test_module_symbol_missing`].
    pub fn test_module_symbol_missing(
        &self,
    ) -> impl Iterator<Item = TestModuleSymbolMissingFinding<'_>> {
        self.view().test_module_symbol_missing()
    }

    /// See [`SeriesEvaluationView::header_file_missing`].
    pub fn header_file_missing(&self) -> impl Iterator<Item = HeaderFileMissingFinding<'_>> {
        self.view().header_file_missing()
    }

    /// See [`SeriesEvaluationView::behaviour_module_missing`].
    pub fn behaviour_module_missing(
        &self,
    ) -> impl Iterator<Item = BehaviourModuleMissingFinding<'_>> {
        self.view().behaviour_module_missing()
    }

    /// See [`SeriesEvaluationView::versioned_machine_snapshot_missing`].
    pub fn versioned_machine_snapshot_missing(
        &self,
    ) -> impl Iterator<Item = VersionedMachineSnapshotMissingFinding<'_>> {
        self.view().versioned_machine_snapshot_missing()
    }

    /// See [`SeriesEvaluationView::wire_constant_bindings_missing`].
    pub fn wire_constant_bindings_missing(
        &self,
    ) -> impl Iterator<Item = WireConstantBindingsMissingFinding<'_>> {
        self.view().wire_constant_bindings_missing()
    }
}

/// What [`SeriesEvaluation::queried_against`] carries.
#[non_exhaustive]
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum QueriedAgainst {
    /// `check ... --project NAME --tag TAG`.
    Pin {
        /// Pinned project.
        project: ProjectName,
        /// Pinned tag.
        tag: TagName,
    },
    /// `check ... --series NAME`.
    Series {
        /// Series name.
        name: SeriesName,
        /// Pins resolved from the series.
        pins: Vec<PinDescriptor>,
    },
}

/// One entry in [`QueriedAgainst::Series::pins`].
#[non_exhaustive]
#[derive(Debug, Clone, Deserialize)]
pub struct PinDescriptor {
    /// Project name.
    pub project: ProjectName,
    /// Pinned tag.
    pub tag: TagName,
}
