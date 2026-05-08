use clap::Parser;
use proptest::prelude::*;

use backhopper_cli::Cli;

proptest! {
    #[test]
    fn version_command_always_parses(formatter in "json|text") {
        let argv = vec!["backhopper", "--formatter", &formatter, "version"];
        Cli::try_parse_from(&argv).unwrap();
    }

    #[test]
    fn snapshots_list_requires_project(project in "[a-z][a-z0-9_-]{0,8}") {
        let argv = vec!["backhopper", "snapshots", "list", "--project", &project];
        Cli::try_parse_from(&argv).unwrap();
    }

    #[test]
    fn api_lookup_parses_multiple_mfas(
        m in "[a-z][a-z0-9_]{0,5}",
        f in "[a-z][a-z0-9_]{0,5}",
        a in 0u8..=5
    ) {
        let mfa = format!("{}:{}/{}", m, f, a);
        let argv = vec![
            "backhopper", "api", "lookup",
            "--project", "p",
            "--tag", "v1",
            "--mfa", &mfa,
            "--mfa", &mfa,
        ];
        Cli::try_parse_from(&argv).unwrap();
    }
}
