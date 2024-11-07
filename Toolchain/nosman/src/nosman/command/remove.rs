use clap::{ArgMatches};

use crate::nosman;
use crate::nosman::command::{Command, CommandResult};

use nosman::workspace::Workspace;

pub struct RemoveCommand {
}

impl RemoveCommand {
}

impl Command for RemoveCommand {
    fn matched_args<'a>(&self, _workspace: &Workspace, args : &'a ArgMatches) -> Option<&'a ArgMatches> {
        args.subcommand_matches("remove")
    }

    fn run(&self, workspace: &mut Workspace, _command_name: Option<&str>, args: &ArgMatches) -> CommandResult {
        let module_name = args.get_one::<String>("module").unwrap();
        let version = args.get_one::<String>("version").unwrap();
        workspace.remove(module_name, version)
    }
}
