use clap::{Arg, ArgMatches};

use crate::nosman;
use crate::nosman::command::{Command, CommandResult};

use nosman::workspace::Workspace;

pub struct RemoveCommand {}

pub fn get_cli() -> clap::Command {
    clap::Command::new("remove")
        .about("Remove a package")
        .arg(Arg::new("package").required(true))
        .arg(Arg::new("version").required(true))
}

impl Command for RemoveCommand {
    fn matched_args<'a>(
        &self,
        _workspace: &Workspace,
        args: &'a ArgMatches,
    ) -> Option<&'a ArgMatches> {
        args.subcommand_matches("remove")
    }

    fn run(
        &self,
        workspace: &mut Workspace,
        _command_name: Option<&str>,
        args: &ArgMatches,
    ) -> CommandResult {
        let package_name = args.get_one::<String>("package").unwrap();
        let version = args.get_one::<String>("version").unwrap();
        workspace.remove(package_name, version)
    }
}
