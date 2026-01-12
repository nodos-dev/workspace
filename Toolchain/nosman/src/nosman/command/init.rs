use clap::{Arg, ArgAction, ArgMatches};
use colored::Colorize;

use crate::nosman::command::{Command, CommandResult};

use crate::nosman::command::CommandError::{InvalidArgument};
use crate::nosman::workspace::{find_root_from, Workspace};

pub struct InitCommand {
}

impl InitCommand {
    pub(crate) fn run_init(&self, workspace: &mut Workspace, allow_nested: bool) -> CommandResult {
        let directory = &workspace.root;
        if !allow_nested {
            if let Some(ws) = find_root_from(&directory.to_path_buf()) {
                return Err(InvalidArgument { message: format!("Directory {} is already under a workspace: {}", directory.display(), ws.display())});
            }   
        }
        println!("Creating a new workspace under {:?}", directory);
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
            .help("Allow creating a workspace even if the folder is already inside another workspace. This also allows to recreate an existing workspace.")
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
        self.run_init(workspace, allow_nested)
    }

    fn needs_workspace(&self) -> bool {
        false
    }
}
