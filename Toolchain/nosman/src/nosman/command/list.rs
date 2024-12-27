use clap::{ArgMatches};
use colored::Colorize;
use crate::nosman::command::{Command, CommandResult};

use crate::nosman::workspace::{Workspace};

pub struct ListCommand {}

impl ListCommand {
    fn run_list(&self, workspace: &mut Workspace, installed: bool, remote: bool) -> CommandResult {
        if installed {
            println!("{}", "Installed modules".green());
            for (name, ver_map) in &workspace.installed_modules {
                for (version, module) in ver_map {
                    println!("  {} ({})", format!("{}-{}", name, version).green().to_string(), module.get_module_dir().display());
                }
            }
        }
        if remote {
            workspace.set_output_mode(crate::nosman::workspace::OutputMode::Silent);
            let latest = workspace.fetch_latest_versions();
            println!("{}", "Remote packages".green());
            for (name, entry) in latest {
                println!("  {} (latest: {})", format!("{}", name).green(), entry.version);
            }
        }
        Ok(true)
    }
}

impl Command for ListCommand {
    fn matched_args<'a>(&self, _workspace: &Workspace, args: &'a ArgMatches) -> Option<&'a ArgMatches> {
        args.subcommand_matches("list")
    }

    fn needs_workspace(&self) -> bool {
        true
    }

    fn run(&self, workspace: &mut Workspace, _command_name: Option<&str>, args: &ArgMatches) -> CommandResult {
        let installed = args.get_one::<bool>("installed").unwrap();
        let remote = args.get_one::<bool>("remote").unwrap();
        self.run_list(workspace, *installed, *remote)
    }
}
