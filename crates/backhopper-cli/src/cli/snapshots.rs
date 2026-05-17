use clap::Subcommand;

use backhopper_core::model::names::{Mfa, ProjectName, TagName};

#[derive(Debug, Subcommand)]
pub enum SnapshotsCmd {
    /// List tags that have no snapshot on disk yet (read-only).
    #[command(name = "list_tags")]
    ListTags {
        #[arg(long)]
        project: Option<ProjectName>,
    },
    /// Generate snapshots for tags that don't have one yet.
    Generate {
        #[arg(long)]
        project: Option<ProjectName>,
        #[arg(long, help = "Skip ls-remote freshness check")]
        no_remote_check: bool,
        #[arg(long, help = "Plan only; do not write to disk")]
        dry_run: bool,
    },
    /// List existing snapshots for a project.
    List {
        #[arg(long)]
        project: ProjectName,
    },
    /// Print a snapshot's canonical text.
    Show {
        #[arg(long)]
        project: ProjectName,
        #[arg(long)]
        tag: TagName,
    },
    /// Verify a snapshot's canonical-form invariants.
    Verify {
        #[arg(long)]
        project: ProjectName,
        #[arg(long)]
        tag: TagName,
    },
    /// Rebuild a snapshot from source (replace the on-disk copy).
    Rebuild {
        #[arg(long)]
        project: ProjectName,
        #[arg(long)]
        tag: TagName,
        #[arg(long, help = "Plan only; do not write to disk")]
        dry_run: bool,
    },
    /// Look up one or more MFAs against a snapshot.
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
    /// List modules a snapshot covers.
    Modules {
        #[arg(long)]
        project: ProjectName,
        #[arg(long)]
        tag: TagName,
        #[arg(long)]
        include_hidden: bool,
    },
    /// List a module's exports in a snapshot.
    Exports {
        #[arg(long)]
        project: ProjectName,
        #[arg(long)]
        tag: TagName,
        #[arg(long)]
        module: String,
    },
    /// Diff two snapshots of the same project.
    Diff {
        #[arg(long)]
        project: ProjectName,
        #[arg(long)]
        from: TagName,
        #[arg(long)]
        to: TagName,
    },
}
