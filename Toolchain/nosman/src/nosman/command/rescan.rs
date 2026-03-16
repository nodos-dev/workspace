use clap::{Arg, ArgAction, ArgMatches};
use colored::Colorize;
use crate::nosman::command::{Command, CommandResult};
use crate::nosman::workspace::{RescanFlags, Workspace};

pub struct RescanCommand {
}

pub fn get_cli() -> clap::Command {
    clap::Command::new("rescan")
        .about("Rescan modules and update caches")
        .arg(Arg::new("fetch_index")
            .action(ArgAction::SetTrue)
            .help("Fetch package-server metadata before scanning")
            .long("fetch-index")
            .num_args(0)
            .required(false)
        )
}

impl Command for RescanCommand {
    fn matched_args<'a>(&self, _workspace: &Workspace, args : &'a ArgMatches) -> Option<&'a ArgMatches> {
        args.subcommand_matches("rescan")
    }

    fn run(&self, workspace: &mut Workspace, _command_name: Option<&str>, args: &ArgMatches) -> CommandResult {
        let now = std::time::Instant::now();
        let fetch_index = args.get_one::<bool>("fetch_index").unwrap();
        let mut flags = RescanFlags::ScanPackages;
        if *fetch_index {
            flags |= RescanFlags::FetchPackageIndex;
        }
        workspace.rescan(flags)?;
        println!("{}", format!("Rescan completed in {:?}", std::time::Instant::now() - now).green());
        Ok(())
    }
}
