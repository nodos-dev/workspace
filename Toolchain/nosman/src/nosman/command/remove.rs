use clap::{ArgMatches};

use crate::nosman;
use crate::nosman::command::{Command, CommandResult};

use nosman::workspace::Workspace;

pub struct RemoveCommand {
}

impl RemoveCommand {
    fn run_remove(&self, module_name: &str, version: &str) -> CommandResult {
        // Fetch remotes
        let mut workspace = Workspace::get()?;
        workspace.remove(module_name, version)
    }
}

impl Command for RemoveCommand {
    fn matched_args<'a>(&self, args : &'a ArgMatches) -> Option<&'a ArgMatches> {
        args.subcommand_matches("remove")
    }

    fn run(&self, args: &ArgMatches) -> CommandResult {
        let module_name = args.get_one::<String>("module").unwrap();
        let version = args.get_one::<String>("version").unwrap();
        self.run_remove(module_name, version)
    }
}
