use clap::{Arg, ArgAction, ArgMatches};
use colored::Colorize;
use crate::nosman;
use crate::nosman::command::{Command, CommandResult};

use nosman::workspace::Workspace;
use crate::nosman::command::CommandError::Runtime;

pub struct UnpublishCommand {
}

impl UnpublishCommand {
    pub fn run_unpublish(&self, workspace: &mut Workspace, dry_run: bool, package_name: &String, version: Option<&String>) -> CommandResult {
        if version.is_none() {
            println!("Unpublishing all versions of package {}", package_name);
        }

        let client = workspace.authenticated_store_client_mut();

        (|| -> std::result::Result<(), String> {
            if dry_run {
                let releases = client
                    .get_my_releases(package_name)
                    .map_err(|e| e.to_string())?;

                let matched: Vec<_> = if let Some(v) = version {
                    releases.iter().filter(|r| r.version == *v).collect()
                } else {
                    releases.iter().collect()
                };

                if matched.is_empty() {
                    return if let Some(v) = version {
                        Err(format!("No release found for package {} version {}", package_name, v))
                    } else {
                        Err(format!("No releases found for package {}", package_name))
                    };
                }

                for release in matched {
                    println!(
                        "Would delete package {} release {} (v{})",
                        package_name, release.id, release.version
                    );
                }
                return Ok(());
            }

            client
                .delete_release(package_name, version.map(|v| v.as_str()))
                .map_err(|e| e.to_string())
        })()
        .map_err(|message| Runtime { message })?;

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

    fn run(&self, workspace: &mut Workspace, _command_name: Option<&str>, args: &ArgMatches) -> CommandResult {
        let package_name = args.get_one::<String>("package_name").unwrap();
        let version = args.get_one::<String>("version");
        let dry_run = args.get_one::<bool>("dry_run").unwrap();
        self.run_unpublish(workspace, *dry_run, package_name, version)
    }

    fn needs_workspace(&self) -> bool {
        false
    }
}
