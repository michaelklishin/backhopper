use std::path::PathBuf;

use clap::Args;

#[derive(Debug, Args, Clone)]
pub struct TreeSource {
    /// Directory containing the Erlang source tree to analyse.
    #[arg(long)]
    pub tree_dir_path: PathBuf,
}
