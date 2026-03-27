use clap::ArgMatches;

use crate::nosman::command::{Command, CommandError, CommandResult};
use crate::nosman::workspace::Workspace;

pub struct AuthCommand {}

impl AuthCommand {
    fn run_login(&self, workspace: &mut Workspace) -> CommandResult {
        workspace.authenticated_store_client_mut()
            .login()
            .map_err(|e| CommandError::Runtime { message: e.to_string() })?;
        println!("Logged in successfully.");
        Ok(())
    }

    fn run_logout(&self, workspace: &mut Workspace) -> CommandResult {
        workspace.authenticated_store_client_mut()
            .logout()
            .map_err(|e| CommandError::Runtime { message: e.to_string() })?;
        println!("Logged out.");
        Ok(())
    }
}

pub fn get_cli() -> clap::Command {
    clap::Command::new("auth")
        .about("Manage Nodos Store authentication")
        .subcommand_required(true)
        .subcommand(
            clap::Command::new("login")
                .about("Log in to the Nodos Store via device flow"),
        )
        .subcommand(
            clap::Command::new("logout")
                .about("Remove the stored Nodos Store access token"),
        )
}

impl Command for AuthCommand {
    fn matched_args<'a, 'b>(
        &self,
        _workspace: &'a Workspace,
        args: &'b ArgMatches,
    ) -> Option<&'b ArgMatches> {
        args.subcommand_matches("auth")
    }

    fn needs_workspace(&self) -> bool {
        false
    }

    fn run(
        &self,
        workspace: &mut Workspace,
        _command_name: Option<&str>,
        args: &ArgMatches,
    ) -> CommandResult {
        match args.subcommand_name() {
            Some("login") => self.run_login(workspace),
            Some("logout") => self.run_logout(workspace),
            _ => unreachable!("subcommand_required ensures a subcommand is always present"),
        }
    }
}
