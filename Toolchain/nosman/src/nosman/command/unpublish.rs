use clap::{Arg, ArgAction, ArgMatches};
use colored::Colorize;
use crate::nosman;
use crate::nosman::command::{Command, CommandResult};

use nosman::workspace::Workspace;
use crate::nosman::command::CommandError::{Runtime, InvalidArgument};

pub struct UnpublishCommand {
}

impl UnpublishCommand {
    pub fn run_unpublish(&self, workspace: &Workspace, dry_run: bool, verbose: bool, remote_name: &String, package_name: &String, version: Option<&String>) -> CommandResult {
        let remote = workspace.find_remote(remote_name);
        if remote.is_none() {
            return Err(InvalidArgument { message: format!("Remote {} not found", remote_name) });
        }
        let remote = remote.unwrap();
        if version.is_none() {
            println!("Unpublishing all versions of package {}", package_name);
        }
        let res = remote.fetch(&workspace);
        if let Err(msg) = res {
            return Err(Runtime { message: msg });
        }
        let res = remote.remove_release(dry_run, verbose, &workspace, package_name, version);
        if let Err(msg) = res {
            return Err(Runtime { message: msg });
        }
        if let Some(version) = version {
            println!("{}", format!("Package {} version {} unpublished", package_name, version).yellow());
        }
        else {
            println!("{}", format!("All releases of package {} are unpublished", package_name).yellow());
        }
        Ok(())
    }
}

pub fn get_cli() -> clap::Command {
    clap::Command::new("unpublish")
        .alias("yank")
        .about("Unpublish a package from the index.")
        .arg(Arg::new("package_name").required(true))
        .arg(Arg::new("remote")
            .help("Name of the remote to edit.")
            .long("remote")
            .default_value("default")
        )
        .arg(Arg::new("version")
            .help("Version of the package to unpublish. If not provided, all versions will be unpublished."))
        .arg(Arg::new("dry_run")
            .action(ArgAction::SetTrue)
            .long("dry-run")
            .help("Do not actually publish the package, just show what would be done.")
            .num_args(0)
            .required(false)
        )
        .arg(Arg::new("verbose")
            .action(ArgAction::SetTrue)
            .long("verbose")
            .help("Print more information about the process.")
            .num_args(0)
            .required(false)
        )
}

impl Command for UnpublishCommand {
    fn matched_args<'a>(&self, _workspace: &Workspace, args : &'a ArgMatches) -> Option<&'a ArgMatches> {
        args.subcommand_matches("unpublish")
    }

    fn run(&self, workspace: &mut Workspace, _command_name: Option<&str>, args: &ArgMatches) -> CommandResult {
        let package_name = args.get_one::<String>("package_name").unwrap();
        let remote_name = args.get_one::<String>("remote").unwrap();
        let version = args.get_one::<String>("version");
        let dry_run = args.get_one::<bool>("dry_run").unwrap();
        let verbose = args.get_one::<bool>("verbose").unwrap();
        self.run_unpublish(workspace, *dry_run, *verbose, remote_name, package_name, version)
    }
}
