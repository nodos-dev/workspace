use clap::{Arg, ArgAction, ArgMatches};
use colored::Colorize;

use crate::nosman::command::{Command, CommandResult};

use crate::nosman::command::CommandError::{InvalidArgument};
use crate::nosman::workspace::{find_root_from, Workspace};

pub struct InitCommand {
}

impl InitCommand {
    pub(crate) fn run_init(&self, workspace: &mut Workspace, allow_nested: bool, reinit: bool) -> CommandResult {
        let directory = &workspace.root;
        if let Some(ws) = find_root_from(&directory.to_path_buf()) {
            if ws == *directory {
                if !reinit {
                    return Err(InvalidArgument {
                        message: format!(
                            "Workspace already exists at {}. Use --reinit to recreate it.",
                            directory.display()
                        ),
                    });
                }
            } else if !allow_nested {
                return Err(InvalidArgument {
                    message: format!(
                        "Directory {} is already under a workspace: {}",
                        directory.display(),
                        ws.display()
                    ),
                });
            }
        }
        if reinit {
            println!("Reinitializing workspace under {:?}", directory);
        } else {
            println!("Creating a new workspace under {:?}", directory);
        }
        workspace.recreate()?;
        println!("{}", format!("Workspace initialized with {} packages", workspace.packages.len()).as_str().green());
        Ok(())
    }
}

pub fn get_cli() -> clap::Command {
    clap::Command::new("init")
        .about("Initialize a directory as a Nodos workspace.")
        .arg(Arg::new("allow_nested")
            .action(ArgAction::SetTrue)
            .long("allow-nested")
            .help("Allow creating a workspace even if the folder is already inside another workspace.")
            .num_args(0)
            .required(false)
        )
        .arg(Arg::new("reinit")
            .action(ArgAction::SetTrue)
            .long("reinit")
            .help("Reinitialize this directory if a workspace already exists here.")
            .num_args(0)
            .required(false)
        )
}

impl Command for InitCommand {
    fn matched_args<'a>(&self, _workspace: &Workspace, args : &'a ArgMatches) -> Option<&'a ArgMatches> {
        args.subcommand_matches("init")
    }

    fn run(&self, workspace: &mut Workspace, _command_name: Option<&str>, args: &ArgMatches) -> CommandResult {
        let allow_nested = args.get_flag("allow_nested");
        let reinit = args.get_flag("reinit");
        self.run_init(workspace, allow_nested, reinit)
    }

    fn needs_workspace(&self) -> bool {
        false
    }
}
