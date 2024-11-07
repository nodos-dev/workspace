use clap::{ArgMatches};
use colored::Colorize;
use crate::nosman::command::{Command, CommandResult};

use crate::nosman::workspace::{Workspace};

pub struct ListCommand {}

impl ListCommand {
    fn run_list(&self, workspace: &Workspace) -> CommandResult {
        for (name, ver_map) in &workspace.installed_modules {
            for (version, module) in ver_map {
                println!("{} ({})", format!("{}-{}", name, version).green().to_string(), module.get_module_dir().display());
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

    fn run(&self, workspace: &mut Workspace, _command_name: Option<&str>, _args: &ArgMatches) -> CommandResult {
        self.run_list(workspace)
    }
}
