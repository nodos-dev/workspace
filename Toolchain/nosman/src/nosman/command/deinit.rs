use std::{fs};
use clap::{ArgMatches};
use colored::Colorize;
use inquire::Confirm;
use crate::nosman::command::{Command, CommandError, CommandResult};
use crate::nosman::workspace;

use crate::nosman::workspace::{Workspace};

pub struct DeinitCommand {
}

impl DeinitCommand {
    fn run_deinit(&self) -> CommandResult {
        let nosman_fpath = workspace::get_nosman_index_filepath().unwrap();
        if nosman_fpath.exists() {
            // Ask user whether to remove the installed modules
            let mut workspace = Workspace::get()?;
            let erase_modules = Confirm::new("Would you like to erase all installed modules?")
                .with_default(false)
                .prompt();
            if erase_modules.map_err(|e| CommandError::GenericError { message: format!("Failed to prompt user: {}", e) })? {
                workspace.remove_all()?;
            }
            fs::remove_file(nosman_fpath)?;
            println!("{}", "Workspace removed".green());
            Ok(true)
        } else {
            Err(CommandError::InvalidArgumentError { message: format!("No workspace found at {:?}", nosman_fpath) })
        }
    }
}

impl Command for DeinitCommand {
    fn matched_args<'a>(&self, args : &'a ArgMatches) -> Option<&'a ArgMatches> {
        return args.subcommand_matches("deinit");
    }

    fn needs_workspace(&self) -> bool {
        true
    }

    fn run(&self, _args: &ArgMatches) -> CommandResult {
        self.run_deinit()
    }
}
