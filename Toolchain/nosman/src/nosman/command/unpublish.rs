use clap::{Arg, ArgAction, ArgMatches};
use colored::Colorize;
use crate::nosman;
use crate::nosman::command::{Command, CommandResult};
use crate::nosman::constants;

use nosman::workspace::Workspace;
use crate::nosman::command::CommandError::Runtime;

pub struct UnpublishCommand {
}

impl UnpublishCommand {
    pub fn run_unpublish(&self, dry_run: bool, package_name: &String, version: Option<&String>) -> CommandResult {
        if version.is_none() {
            println!("Unpublishing all versions of package {}", package_name);
        }
        let res = crate::nosman::package_server::delete_release(
            constants::NODOS_STORE_API_URL,
            package_name,
            version,
            dry_run,
        );
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
        .about("Unpublish a package from the package server.")
        .arg(Arg::new("package_name").required(true))
        .arg(Arg::new("version")
            .help("Version of the package to unpublish. If not provided, all versions will be unpublished."))
        .arg(Arg::new("dry_run")
            .action(ArgAction::SetTrue)
            .long("dry-run")
            .help("Do not actually publish the package, just show what would be done.")
            .num_args(0)
            .required(false)
        )
}

impl Command for UnpublishCommand {
    fn matched_args<'a>(&self, _workspace: &Workspace, args : &'a ArgMatches) -> Option<&'a ArgMatches> {
        args.subcommand_matches("unpublish")
    }

    fn run(&self, _workspace: &mut Workspace, _command_name: Option<&str>, args: &ArgMatches) -> CommandResult {
        let package_name = args.get_one::<String>("package_name").unwrap();
        let version = args.get_one::<String>("version");
        let dry_run = args.get_one::<bool>("dry_run").unwrap();
        self.run_unpublish(*dry_run, package_name, version)
    }

    fn needs_workspace(&self) -> bool {
        false
    }
}
