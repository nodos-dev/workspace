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
    fn run_deinit(&self, workspace: &mut Workspace) -> CommandResult {
        let nosman_fpath = workspace.get_nosman_index_filepath();
        if nosman_fpath.exists() {
            // Ask user whether to remove the installed modules
            let erase_modules = Confirm::new("Would you like to erase all installed modules?")
                .with_default(false)
                .prompt();
            if erase_modules.map_err(|e| CommandError::RuntimeError { message: format!("Failed to prompt user: {}", e) })? {
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
    fn matched_args<'a>(&self, _workspace: &mut Workspace, args : &'a ArgMatches) -> Option<&'a ArgMatches> {
        args.subcommand_matches("deinit")
    }

    fn run(&self, workspace: &mut Workspace, _command_name: Option<&str>, _args: &ArgMatches) -> CommandResult {
        self.run_deinit(workspace)
    }

    fn needs_workspace(&self) -> bool {
        true
    }
}
