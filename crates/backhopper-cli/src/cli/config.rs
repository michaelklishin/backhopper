use clap::Subcommand;

#[derive(Debug, Subcommand)]
pub enum ConfigCmd {
    /// Print the resolved config file path.
    Path,
    /// Print the loaded config as canonical TOML.
    Show,
    /// Parse and validate the config; exit non-zero if invalid.
    Validate,
}
