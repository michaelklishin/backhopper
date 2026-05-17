use clap::Subcommand;

use backhopper_core::model::names::SeriesName;

#[derive(Debug, Subcommand)]
pub enum SeriesCmd {
    /// List configured series.
    List,
    /// Show the pins for a series.
    Show {
        #[arg(long)]
        series: SeriesName,
    },
}
