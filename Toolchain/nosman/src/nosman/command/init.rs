use clap::{ArgMatches};
use colored::Colorize;

use crate::nosman::command::{Command, CommandResult};

use crate::nosman::command::CommandError::{InvalidArgument};
use crate::nosman::workspace::{find_root_from, Workspace};

pub struct InitCommand {
}

impl InitCommand {
    pub(crate) fn run_init(&self, workspace: &mut Workspace) -> CommandResult {
        let directory = &workspace.root;
        if let Some(ws) = find_root_from(&directory.to_path_buf()) {
            return Err(InvalidArgument { message: format!("Directory {} is already under a workspace: {}", directory.display(), ws.display())});
        }
        println!("Creating a new workspace under {:?}", directory);
        workspace.recreate()?;
        println!("{}", format!("Workspace initialized with {} modules", workspace.installed_modules.len()).as_str().green());
        Ok(true)
    }
}

impl Command for InitCommand {
    fn matched_args<'a>(&self, _workspace: &Workspace, args : &'a ArgMatches) -> Option<&'a ArgMatches> {
        args.subcommand_matches("init")
    }

    fn run(&self, workspace: &mut Workspace, _command_name: Option<&str>, _args: &ArgMatches) -> CommandResult {
        self.run_init(workspace)
    }

    fn needs_workspace(&self) -> bool {
        false
    }
}
