use clap::{ArgMatches};
use colored::Colorize;

use crate::nosman;
use crate::nosman::command::{Command, CommandResult};

use crate::nosman::command::CommandError::{InvalidArgumentError, IOError};
use crate::nosman::workspace::{find_root_from, Workspace};

pub struct InitCommand {
}

impl InitCommand {
    fn run_init(&self) -> CommandResult {
        let directory = nosman::workspace::current().unwrap();
        if let Some(ws) = find_root_from(&directory.to_path_buf()) {
            return Err(InvalidArgumentError { message: format!("Directory {} is already under a workspace: {}", directory.display(), ws.display())});
        }
        println!("Creating a new workspace under {:?}", directory);

        let res = Workspace::rescan(&directory, true);
        if res.is_err() {
            return Err(IOError(res.err().unwrap()));
        }
        let workspace = res.unwrap();
        println!("{}", format!("Workspace initialized with {} modules", workspace.installed_modules.len()).as_str().green());
        Ok(true)
    }
}

impl Command for InitCommand {
    fn matched_args<'a>(&self, args : &'a ArgMatches) -> Option<&'a ArgMatches> {
        args.subcommand_matches("init")
    }

    fn needs_workspace(&self) -> bool {
        false
    }

    fn run(&self, _args: &ArgMatches) -> CommandResult {
        self.run_init()
    }
}
