use clap::{Arg, ArgAction, ArgMatches};
use crate::nosman;
use crate::nosman::command::{Command, CommandResult};

use nosman::workspace::Workspace;
use crate::nosman::command::CommandError::Runtime;
use crate::nosman::ui;

pub struct UnpublishCommand {
}

impl UnpublishCommand {
    pub fn run_unpublish(&self, workspace: &mut Workspace, dry_run: bool, verbose: bool, package_name: &String, version: Option<&String>) -> CommandResult {
        if version.is_none() {
            ui::step("Unpublishing", format!("every version of {}", package_name));
        }

        let client = workspace.authenticated_store_client_mut()?;

        if dry_run {
            let releases = client
                .get_my_releases(package_name)
                .map_err(|e| Runtime { message: e.to_string() })?;

            let matched: Vec<_> = if let Some(v) = version {
                releases.iter().filter(|r| r.version == *v).collect()
            } else {
                releases.iter().collect()
            };

            if matched.is_empty() {
                return if let Some(v) = version {
                    Err(Runtime { message: format!("No release found for package {} version {}", package_name, v) })
                } else {
                    Err(Runtime { message: format!("No releases found for package {}", package_name) })
                };
            }

            for release in matched {
                ui::step("Dry run", format!("would delete {} release {} (v{})", package_name, release.id, release.version));
            }
        } else {
            if verbose {
                ui::detail(format!("requesting deletion of {} {}", package_name, version.map_or("(all versions)".to_string(), |v| format!("v{}", v))));
            }
            client
                .delete_release(package_name, version.map(|v| v.as_str()))
                .map_err(|e| Runtime { message: e.to_string() })?;
        }

        if let Some(version) = version {
            ui::removed(package_name, version);
        }
        else {
            ui::step("Unpublished", format!("every release of {}", package_name));
        }
        Ok(())
    }
}

pub fn get_cli() -> clap::Command {
    clap::Command::new("unpublish")
        .alias("yank")
        .about("Unpublish a package from the Nodos Store.")
        .arg(Arg::new("package_name").required(true))
        .arg(Arg::new("version")
            .help("Version of the package to unpublish. If not provided, all versions will be unpublished."))
        .arg(Arg::new("dry_run")
            .action(ArgAction::SetTrue)
            .long("dry-run")
            .help("Do not actually unpublish the package, just show what would be done.")
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
        let verbose = ui::is_verbose();
        self.run_unpublish(workspace, *dry_run, verbose, package_name, version)
    }

    fn needs_workspace(&self) -> bool {
        false
    }
}
