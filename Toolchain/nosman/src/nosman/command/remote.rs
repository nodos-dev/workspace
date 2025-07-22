use clap::{Arg, ArgMatches};
use colored::Colorize;

use crate::nosman;
use crate::nosman::command::{Command, CommandError, CommandResult};
use crate::nosman::workspace::Workspace;

pub fn get_cli() -> clap::Command {
    clap::Command::new("remote")
        .about("Manage remotes.")
        .subcommand(clap::Command::new("add")
            .about("Add a remote")
            .arg(Arg::new("url").required(true))
        )
        .subcommand(clap::Command::new("list")
            .about("List remotes")
        )
        .subcommand(clap::Command::new("remove")
            .about("Remove a remote")
            .arg(Arg::new("url").required(true))
        )
}

pub struct RemoteAddCommand {
}

impl RemoteAddCommand {
    fn run_add_remote(&self, workspace: &mut Workspace, url: &str) -> CommandResult {
        if workspace.remotes.iter().any(|r| r.url == url) {
            return Err(CommandError::InvalidArgument { message: format!("Remote {} already exists", url) });
        }

        // Add the remote
        workspace.add_remote(nosman::index::Remote::new("unnamed", url));

        // Write the workspace file
        workspace.save().map_err(|e| CommandError::IO { file: workspace.get_nosman_index_filepath().display().to_string(), message: format!("{}", e) })?;

        println!("Remote added: {}", url);
        Ok(())
    }
}

impl Command for RemoteAddCommand {
    fn matched_args<'a>(&self, _workspace: &Workspace, args : &'a ArgMatches) -> Option<&'a ArgMatches> {
        if let Some(subcommand) = args.subcommand_matches("remote") {
            return subcommand.subcommand_matches("add");
        }
        None
    }

    fn run(&self, workspace: &mut Workspace, _command_name: Option<&str>, args: &ArgMatches) -> CommandResult {
        let url = args.get_one::<String>("url").unwrap();
        if url.is_empty() {
            return Err(CommandError::InvalidArgument { message: "url is required".to_string() });
        }
        self.run_add_remote(workspace, url)
    }
}

pub struct RemoteListCommand {
}

impl RemoteListCommand {
    fn run_list_remotes(&self, workspace: &Workspace) -> CommandResult {
        if workspace.remotes.is_empty() {
            println!("No remotes found");
            return Ok(())
        }
        println!("{}", "Remotes".green());
        for remote in &workspace.remotes {
            println!("  {} - {}", remote.name, remote.url);
        }
        Ok(())
    }
}

impl Command for RemoteListCommand {
    fn matched_args<'a>(&self, _workspace: &Workspace, args : &'a ArgMatches) -> Option<&'a ArgMatches> {
        if let Some(subcommand) = args.subcommand_matches("remote") {
            return subcommand.subcommand_matches("list");
        }
        None
    }

    fn run(&self, workspace: &mut Workspace, _command_name: Option<&str>, _args: &ArgMatches) -> CommandResult {
        self.run_list_remotes(workspace)
    }
}


