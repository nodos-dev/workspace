use std::{fs, path};

use clap::{ArgMatches};
use colored::Colorize;

use crate::nosman;
use crate::nosman::command::{Command, CommandError, CommandResult};

use serde::{Deserialize, Serialize};
use serde_json::Result;
use crate::nosman::command::CommandError::{InvalidArgumentError, IOError};
use crate::nosman::index::{Index, Remote};
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

        let res = Workspace::rescan(&directory);
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

    fn run(&self, args: &ArgMatches) -> CommandResult {
        self.run_init()
    }
}
