//! Clap-based command-line parser definition.

use std::path::PathBuf;

use clap::{ArgAction, Args, Parser, Subcommand, ValueEnum};

use backhopper_core::model::names::{Mfa, ProjectName, SeriesName, TagName};

#[derive(Debug, Parser)]
#[command(
    name = "backhopper",
    version,
    about = "Record Erlang/Elixir public APIs per git tag and answer compatibility questions",
    propagate_version = true,
    arg_required_else_help = true
)]
pub struct Cli {
    #[command(flatten)]
    pub global: GlobalArgs,

    #[command(subcommand)]
    pub group: Group,
}

#[derive(Debug, Args, Clone)]
pub struct GlobalArgs {
    #[arg(
        long,
        short = 'c',
        env = "BACKHOPPER_CONFIG_FILE_PATH",
        global = true,
        help = "Path to backhopper.toml"
    )]
    pub config: Option<PathBuf>,

    #[arg(
        long,
        short = 's',
        env = "BACKHOPPER_SNAPSHOT_DIR",
        global = true,
        help = "Override snapshot_dir from config"
    )]
    pub snapshot_dir: Option<PathBuf>,

    #[arg(
        long,
        env = "BACKHOPPER_FORMATTER",
        global = true,
        default_value_t = Formatter::Json,
        value_enum,
        help = "Output formatter",
    )]
    pub formatter: Formatter,

    #[arg(long, short = 'q', global = true, conflicts_with = "verbose")]
    pub quiet: bool,

    #[arg(
        long,
        short = 'v',
        global = true,
        conflicts_with = "quiet",
        action = ArgAction::Count,
        help = "Increase log verbosity: -v=info, -vv=debug, -vvv=trace"
    )]
    pub verbose: u8,

    #[arg(
        long,
        env = "BACKHOPPER_NON_INTERACTIVE_MODE",
        global = true,
        help = "Disable prompts and TTY-only behaviors"
    )]
    pub non_interactive: bool,

    #[arg(
        long,
        global = true,
        default_value = "modern",
        help = "Style preset for the text formatter"
    )]
    pub table_style: String,

    #[arg(long, global = true, help = "Plan only; do not write to disk")]
    pub dry_run: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Formatter {
    Json,
    Text,
}

#[derive(Debug, Subcommand)]
pub enum Group {
    Projects {
        #[command(subcommand)]
        cmd: ProjectsCmd,
    },
    Series {
        #[command(subcommand)]
        cmd: SeriesCmd,
    },
    Snapshots {
        #[command(subcommand)]
        cmd: SnapshotsCmd,
    },
    Api {
        #[command(subcommand)]
        cmd: ApiCmd,
    },
    Compatibility {
        #[command(subcommand)]
        cmd: CompatibilityCmd,
    },
    Config {
        #[command(subcommand)]
        cmd: ConfigCmd,
    },
    Completions {
        #[command(subcommand)]
        cmd: CompletionsCmd,
    },
    Version,
}

#[derive(Debug, Subcommand)]
pub enum ProjectsCmd {
    List,
    Show {
        #[arg(long)]
        project: ProjectName,
    },
}

#[derive(Debug, Subcommand)]
pub enum SeriesCmd {
    List,
    Show {
        #[arg(long)]
        series: SeriesName,
    },
    InferFromRabbitmq {
        #[arg(long)]
        repo: PathBuf,
        #[arg(long, conflicts_with = "all_branches")]
        branch: Option<String>,
        #[arg(long, conflicts_with = "branch")]
        all_branches: bool,
        #[arg(
            long,
            value_delimiter = ',',
            default_values_t = default_branches(),
            help = "Branches to walk when --all-branches (comma-separated)",
        )]
        branches: Vec<String>,
        #[arg(long)]
        name: Option<String>,
        #[arg(long, help = "Print warnings for skipped or commit-pinned deps")]
        show_skipped: bool,
    },
}

fn default_branches() -> Vec<String> {
    vec![
        "main".into(),
        "v4.3.x".into(),
        "v4.2.x".into(),
        "v4.1.x".into(),
        "v4.0.x".into(),
    ]
}

#[derive(Debug, Subcommand)]
pub enum SnapshotsCmd {
    Discover {
        #[arg(long)]
        project: Option<ProjectName>,
    },
    Update {
        #[arg(long)]
        project: Option<ProjectName>,
        #[arg(long, help = "Skip ls-remote freshness check")]
        no_remote_check: bool,
    },
    List {
        #[arg(long)]
        project: ProjectName,
    },
    Show {
        #[arg(long)]
        project: ProjectName,
        #[arg(long)]
        tag: TagName,
    },
    Verify {
        #[arg(long)]
        project: ProjectName,
        #[arg(long)]
        tag: TagName,
    },
    Rebuild {
        #[arg(long)]
        project: ProjectName,
        #[arg(long)]
        tag: TagName,
    },
}

#[derive(Debug, Subcommand)]
pub enum ApiCmd {
    Lookup {
        #[arg(long)]
        project: ProjectName,
        #[arg(long)]
        tag: TagName,
        #[arg(long, action = clap::ArgAction::Append, required = true)]
        mfa: Vec<Mfa>,
        #[arg(long)]
        include_hidden: bool,
    },
    Modules {
        #[arg(long)]
        project: ProjectName,
        #[arg(long)]
        tag: TagName,
        #[arg(long)]
        include_hidden: bool,
    },
    Exports {
        #[arg(long)]
        project: ProjectName,
        #[arg(long)]
        tag: TagName,
        #[arg(long)]
        module: String,
    },
    Diff {
        #[arg(long)]
        project: ProjectName,
        #[arg(long)]
        from: TagName,
        #[arg(long)]
        to: TagName,
    },
}

#[derive(Debug, Args, Clone, Copy, Default)]
pub struct DiagnosticsFlags {
    #[arg(
        long,
        help = "Print untracked module calls in the text-mode footer (informational, not a verdict input)"
    )]
    pub show_untracked_calls: bool,
    #[arg(
        long,
        help = "Include OTP stdlib calls in the untracked-calls footer (implies --show-untracked-calls)"
    )]
    pub show_otp_calls: bool,
}

#[derive(Debug, Subcommand)]
pub enum CompatibilityCmd {
    Patch {
        #[arg(long, conflicts_with = "series")]
        project: Option<ProjectName>,
        #[arg(long, requires = "project")]
        tag: Option<TagName>,
        #[arg(long)]
        series: Option<SeriesName>,
        #[arg(long)]
        explain: bool,
        #[command(flatten)]
        diagnostics: DiagnosticsFlags,
        #[arg(value_name = "PATCH_FILE")]
        patch_file: Option<PathBuf>,
    },
    Commit {
        #[arg(long, conflicts_with = "series")]
        project: Option<ProjectName>,
        #[arg(long, requires = "project")]
        tag: Option<TagName>,
        #[arg(long)]
        series: Option<SeriesName>,
        #[arg(long, default_value = ".")]
        repo: PathBuf,
        #[command(flatten)]
        diagnostics: DiagnosticsFlags,
        #[arg(value_name = "COMMIT_SHA")]
        commit: String,
    },
    Range {
        #[arg(long, conflicts_with = "series")]
        project: Option<ProjectName>,
        #[arg(long, requires = "project")]
        tag: Option<TagName>,
        #[arg(long)]
        series: Option<SeriesName>,
        #[arg(long, default_value = ".")]
        repo: PathBuf,
        #[arg(long, conflicts_with = "merge_commit")]
        range: Option<String>,
        #[arg(long)]
        merge_commit: Option<String>,
        #[command(flatten)]
        diagnostics: DiagnosticsFlags,
    },
}

#[derive(Debug, Subcommand)]
pub enum ConfigCmd {
    Path,
    Show,
    Validate,
}

#[derive(Debug, Subcommand)]
pub enum CompletionsCmd {
    Bash,
    Zsh,
    Fish,
    Nushell,
    Pwsh,
}
