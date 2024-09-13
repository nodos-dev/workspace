use clap::{ArgMatches};
use colored::Colorize;
use crate::nosman::command::{Command, CommandResult};

use crate::nosman::workspace::{Workspace};

pub struct ListCommand {}

impl ListCommand {
    fn run_list(&self) -> CommandResult {
        let workspace = Workspace::get()?;
        for (name, ver_map) in &workspace.installed_modules {
            for (version, module) in ver_map {
                println!("{} ({})", format!("{}-{}", name, version).green().to_string(), module.get_module_dir().display());
            }
        }
        Ok(true)
    }
}

impl Command for ListCommand {
    fn matched_args<'a>(&self, args: &'a ArgMatches) -> Option<&'a ArgMatches> {
        return args.subcommand_matches("list");
    }

    fn needs_workspace(&self) -> bool {
        true
    }

    fn run(&self, _args: &ArgMatches) -> CommandResult {
        self.run_list()
    }
}
