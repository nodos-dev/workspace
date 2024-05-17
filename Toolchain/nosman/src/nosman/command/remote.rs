use clap::{ArgMatches};
use colored::Colorize;

use crate::nosman;
use crate::nosman::command::{Command, CommandError, CommandResult};
use crate::nosman::workspace::Workspace;

pub struct RemoteAddCommand {
}

impl RemoteAddCommand {
    fn run_add_remote(&self, url: &str) -> Result<bool, CommandError> {
        let mut workspace = Workspace::get()?;
        if workspace.remotes.iter().any(|r| r.url == url) {
            return Err(CommandError::InvalidArgumentError { message: format!("Remote {} already exists", url) });
        }

        // Add the remote
        workspace.add_remote(nosman::index::Remote::new("unnamed", url));

        // Write the workspace file
        workspace.save().map_err(CommandError::IOError)?;

        println!("Remote added: {}", url);
        Ok(true)
    }
}

impl Command for RemoteAddCommand {
    fn matched_args<'a>(&self, args : &'a ArgMatches) -> Option<&'a ArgMatches> {
        if let Some(subcommand) = args.subcommand_matches("remote") {
            return subcommand.subcommand_matches("add");
        }
        None
    }

    fn run(&self, args: &ArgMatches) -> CommandResult {
        let url = args.get_one::<String>("url").unwrap();
        if url.is_empty() {
            return Err(CommandError::InvalidArgumentError { message: "url is required".to_string() });
        }
        self.run_add_remote(url)
    }
}

pub struct RemoteListCommand {
}

impl RemoteListCommand {
    fn run_list_remotes(&self) -> Result<bool, CommandError> {
        let workspace = Workspace::get()?;
        if workspace.remotes.is_empty() {
            println!("No remotes found");
            return Ok(true);
        }

        println!("{}", "Remotes".green());
        for remote in &workspace.remotes {
            println!("  {} - {}", remote.name, remote.url);
        }

        Ok(true)
    }
}

impl Command for RemoteListCommand {
    fn matched_args<'a>(&self, args : &'a ArgMatches) -> Option<&'a ArgMatches> {
        if let Some(subcommand) = args.subcommand_matches("remote") {
            return subcommand.subcommand_matches("list");
        }
        None
    }

    fn run(&self, _args: &ArgMatches) -> CommandResult {
        self.run_list_remotes()
    }
}


