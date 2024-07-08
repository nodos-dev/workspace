use clap::{ArgMatches};
use colored::Colorize;
use crate::nosman;
use crate::nosman::command::{Command, CommandResult};

use nosman::workspace::Workspace;
use crate::nosman::command::CommandError::{GenericError, InvalidArgumentError};

pub struct UnpublishCommand {
}

impl UnpublishCommand {
    fn run_unpublish(&self, dry_run: bool, verbose: bool, remote_name: &String, package_name: &String, version: Option<&String>) -> CommandResult {
        let workspace = Workspace::get()?;

        let remote = workspace.find_remote(remote_name);
        if remote.is_none() {
            return Err(InvalidArgumentError { message: format!("Remote {} not found", remote_name) });
        }
        let remote = remote.unwrap();
        if version.is_none() {
            println!("Unpublishing all versions of package {}", package_name);
        }
        let res = remote.fetch(&workspace);
        if let Err(msg) = res {
            return Err(GenericError { message: msg });
        }
        let res = remote.remove_release(dry_run, verbose, &workspace, package_name, version);
        if let Err(msg) = res {
            return Err(GenericError { message: msg });
        }
        if let Some(version) = version {
            println!("{}", format!("Package {} version {} unpublished", package_name, version).yellow());
        }
        else {
            println!("{}", format!("All releases of package {} are unpublished", package_name).yellow());
        }
        Ok(true)
    }
}

impl Command for UnpublishCommand {
    fn matched_args<'a>(&self, args : &'a ArgMatches) -> Option<&'a ArgMatches> {
        args.subcommand_matches("unpublish")
    }

    fn run(&self, args: &ArgMatches) -> CommandResult {
        let package_name = args.get_one::<String>("package_name").unwrap();
        let remote_name = args.get_one::<String>("remote").unwrap();
        let version = args.get_one::<String>("version");
        let dry_run = args.get_one::<bool>("dry_run").unwrap();
        let verbose = args.get_one::<bool>("verbose").unwrap();
        self.run_unpublish(*dry_run, *verbose, remote_name, package_name, version)
    }
}
