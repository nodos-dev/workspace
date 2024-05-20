use std::{fs, io};
use std::io::Write;
use clap::{ArgMatches};
use colored::Colorize;

use crate::nosman::command::{Command, CommandError, CommandResult};
use crate::nosman::workspace;

use crate::nosman::workspace::{Workspace};

pub struct DeinitCommand {
}

impl DeinitCommand {
    fn run_deinit(&self) -> CommandResult {
        let nosman_fpath = workspace::current_nosman_file().unwrap();
        if nosman_fpath.exists() {
            // Ask user whether to remove the installed modules
            let mut workspace = Workspace::get()?;
            io::stdout().write_all(b"Would you like to remove all installed modules? [y/N] ")?;
            io::stdout().flush()?;
            let mut input = String::new();
            io::stdin().read_line(&mut input)?;
            if input.trim().to_lowercase() == "y" {
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
