use clap::Subcommand;

use backhopper_core::model::names::ProjectName;

#[derive(Debug, Subcommand)]
pub enum ProjectsCmd {
    /// List configured projects.
    List,
    /// Show a single project's configuration.
    Show {
        #[arg(long)]
        project: ProjectName,
    },
}
