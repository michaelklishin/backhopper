use std::path::PathBuf;

use serde::Serialize;

use crate::cli::{GlobalArgs, ProjectsCmd};
use crate::commands::context::{load_config, open_store_read};
use crate::errors::{CliError, CliResult};
use crate::output::{OutputContext, render};

#[derive(Debug, Serialize)]
struct ProjectListEntry {
    name: String,
    git_url: PathBuf,
    language: String,
}

#[derive(Debug, Serialize)]
struct ProjectShow {
    name: String,
    git_url: PathBuf,
    language: String,
    tag_prefix: String,
    public_modules: Vec<String>,
    captured_tags: Vec<String>,
}

pub fn handle(args: &GlobalArgs, cmd: ProjectsCmd) -> CliResult<i32> {
    let cfg = load_config(args)?;
    match cmd {
        ProjectsCmd::List => {
            let entries: Vec<_> = cfg
                .projects
                .iter()
                .map(|p| ProjectListEntry {
                    name: p.name.to_string(),
                    git_url: p.git_url.clone(),
                    language: format!("{:?}", p.language).to_lowercase(),
                })
                .collect();
            let ctx = OutputContext::new(args.formatter, "projects list");
            render(&ctx, &entries, |w| {
                for e in &entries {
                    writeln!(w, "{}\t{}\t{}", e.name, e.language, e.git_url.display())?;
                }
                Ok(())
            })?;
            Ok(0)
        }
        ProjectsCmd::Show { project } => {
            let p = cfg
                .project(&project)
                .map_err(|e| CliError::Core(e.into()))?;
            let store = open_store_read(args, &cfg)?;
            let tags = store
                .list_tags(&project)
                .map_err(|e| CliError::Core(e.into()))?;
            let payload = ProjectShow {
                name: p.name.to_string(),
                git_url: p.git_url.clone(),
                language: format!("{:?}", p.language).to_lowercase(),
                tag_prefix: p.tag_prefix.clone(),
                public_modules: p.public_modules.clone(),
                captured_tags: tags.iter().map(|t| t.to_string()).collect(),
            };
            let ctx = OutputContext::new(args.formatter, "projects show");
            render(&ctx, &payload, |w| {
                writeln!(w, "{}", payload.name)?;
                writeln!(w, "  git_url       {}", payload.git_url.display())?;
                writeln!(w, "  language      {}", payload.language)?;
                writeln!(w, "  tag_prefix    {:?}", payload.tag_prefix)?;
                writeln!(w, "  captured_tags {}", payload.captured_tags.len())?;
                Ok(())
            })?;
            Ok(0)
        }
    }
}
