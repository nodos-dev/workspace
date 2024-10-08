use std::path::PathBuf;
use clap::{ArgMatches};
use colored::Colorize;

use crate::nosman;
use crate::nosman::command::{Command, CommandResult};

use crate::nosman::command::CommandError::{InvalidArgumentError};
use crate::nosman::workspace::{find_root_from, Workspace};

pub struct InitCommand {
}

impl InitCommand {
    pub(crate) fn run_init(&self, directory: &PathBuf) -> CommandResult {
        if let Some(ws) = find_root_from(&directory.to_path_buf()) {
            return Err(InvalidArgumentError { message: format!("Directory {} is already under a workspace: {}", directory.display(), ws.display())});
        }
        println!("Creating a new workspace under {:?}", directory);

        let workspace = Workspace::new(&directory)?;

        println!("{}", format!("Workspace initialized with {} modules", workspace.installed_modules.len()).as_str().green());
        Ok(true)
    }
}

impl Command for InitCommand {
    fn matched_args<'a>(&self, args : &'a ArgMatches) -> Option<&'a ArgMatches> {
        args.subcommand_matches("init")
    }

    fn run(&self, _args: &ArgMatches) -> CommandResult {
        let directory = nosman::workspace::current_root().unwrap();
        self.run_init(directory)
    }

    fn needs_workspace(&self) -> bool {
        false
    }
}
