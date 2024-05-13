use clap::ArgMatches;
use colored::Colorize;
use crate::nosman;
use crate::nosman::command::{Command, CommandResult};
use crate::nosman::workspace::Workspace;

pub struct RescanCommand {
}

impl Command for RescanCommand {
    fn matched_args<'a>(&self, args : &'a ArgMatches) -> Option<&'a ArgMatches> {
        args.subcommand_matches("rescan")
    }

    fn run(&self, args: &ArgMatches) -> CommandResult {
        Workspace::rescan(&nosman::workspace::current().unwrap())?;
        println!("{}", "Rescan completed".green());
        Ok(true)
    }
}
