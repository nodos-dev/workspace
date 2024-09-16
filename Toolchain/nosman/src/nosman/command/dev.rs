use std::path::PathBuf;
use clap::{ArgMatches};
use colored::Colorize;

use crate::nosman::command::{Command, CommandError, CommandResult};

pub struct DevPullCommand {
}

impl DevPullCommand {
    fn run_dev_pull(&self, modules_dir: PathBuf) -> Result<bool, CommandError> {
        // Scan module folder for git repositories and run "git pull" on them
        if !modules_dir.exists() {
            Err (CommandError::InvalidArgumentError { message: format!("No such directory: {}", modules_dir.display()) })
        } else {
            let mut stack = Vec::new();
            stack.push(modules_dir);
            while let Some(dir) = stack.pop() {
                for entry in std::fs::read_dir(dir).expect("Failed to read directory") {
                    let entry = entry.expect("Failed to read entry");
                    let path = entry.path();
                    if path.is_dir() && path.join(".git").is_dir() {
                        println!("{}{}", "Pulling: ".yellow(), path.display());
                        let status = std::process::Command::new("git")
                            .arg("pull")
                            .current_dir(&path)
                            .status()
                            .expect("Failed to run git pull");
                        if !status.success() {
                            return Err(CommandError::GenericError { message: format!("Failed to pull {}", path.display()) });
                        }
                    } else if path.is_dir() {
                        stack.push(path);
                    }
                }
            }
            Ok(true)
        }
    }
}

impl Command for DevPullCommand {
    fn matched_args<'a>(&self, args : &'a ArgMatches) -> Option<&'a ArgMatches> {
        if let Some(subcommand) = args.subcommand_matches("dev") {
            return subcommand.subcommand_matches("pull");
        }
        None
    }

    fn run(&self, args: &ArgMatches) -> CommandResult {
        let modules_dir = PathBuf::from(args.get_one::<String>("modules_dir").unwrap());
        self.run_dev_pull(modules_dir)
    }

    fn needs_workspace(&self) -> bool {
        false
    }
}
