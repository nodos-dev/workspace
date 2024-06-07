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
        let fetch_index = args.get_one::<bool>("fetch_index").unwrap();
        Workspace::rescan(&nosman::workspace::current_root().unwrap(), fetch_index.clone())?;
        println!("{}", "Rescan completed".green());
        Ok(true)
    }
}
