use clap::ArgMatches;
use colored::Colorize;
use crate::nosman::command::{Command, CommandResult};
use crate::nosman::workspace::{RescanFlags, Workspace};

pub struct RescanCommand {
}

impl Command for RescanCommand {
    fn matched_args<'a>(&self, _workspace: &mut Workspace, args : &'a ArgMatches) -> Option<&'a ArgMatches> {
        args.subcommand_matches("rescan")
    }

    fn run(&self, workspace: &mut Workspace, _command_name: Option<&str>, args: &ArgMatches) -> CommandResult {
        let now = std::time::Instant::now();
        let fetch_index = args.get_one::<bool>("fetch_index").unwrap();
        let mut flags = RescanFlags::ScanModules;
        if *fetch_index {
            flags |= RescanFlags::FetchPackageIndex;
        }
        workspace.rescan(flags)?;
        println!("{}", format!("Rescan completed in {:?}", std::time::Instant::now() - now).green());
        Ok(true)
    }
}
