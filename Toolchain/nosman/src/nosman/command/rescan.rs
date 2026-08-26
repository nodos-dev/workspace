use clap::{Arg, ArgAction, ArgMatches};
use crate::nosman::command::{Command, CommandResult};
use crate::nosman::workspace::{RescanFlags, Workspace};
use crate::nosman::ui;

pub struct RescanCommand {
}

pub fn get_cli() -> clap::Command {
    clap::Command::new("rescan")
        .about("Rescan packages and update caches")
        .arg(Arg::new("fetch_index")
            .action(ArgAction::SetTrue)
            .help("Fetch Nodos Store metadata before scanning")
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
        ui::summary_line("Rescanned the workspace", now);
        Ok(())
    }
}
