use std::path::PathBuf;
use std::time::Duration;
use clap::{ArgMatches};
use colored::Colorize;
use indicatif::ProgressBar;
use rayon::prelude::*;
use crate::nosman::command::{Command, CommandError, CommandResult};

pub struct DevPullCommand {
}

impl DevPullCommand {
    fn run_dev_pull(&self, modules_dir: PathBuf) -> Result<bool, CommandError> {
        // Scan module folder for git repositories and run "git pull" on them
        if !modules_dir.exists() {
            Err (CommandError::InvalidArgumentError { message: format!("No such directory: {}", modules_dir.display()) })
        } else {
            let pb = ProgressBar::new_spinner();
            pb.enable_steady_tick(Duration::from_millis(100));
            pb.set_message("Scanning for git repositories...");
            let mut stack = Vec::new();
            stack.push(modules_dir);
            let mut git_dirs = Vec::new();
            while let Some(dir) = stack.pop() {
                for entry in std::fs::read_dir(dir).expect("Failed to read directory") {
                    let entry = entry.expect("Failed to read entry");
                    let path = entry.path();
                    if path.is_dir() && path.join(".git").is_dir() {
                        git_dirs.push(path);
                    } else if path.is_dir() {
                        stack.push(path);
                    }
                }
            }
            pb.set_message("Pulling...");
            git_dirs.par_iter().for_each(|path| {
                pb.println(format!("{}{}", "Pulling: ".yellow(), path.display()));
                let output = std::process::Command::new("git")
                    .arg("pull")
                    .current_dir(&path)
                    .output()
                    .expect("Failed to run git pull");
                if !output.status.success() {
                    pb.println(format!("{}{}{}", "Failed to pull: ".red(), path.display(), String::from_utf8_lossy(&output.stderr)));
                }
                // Submodule update recursive
                let status = std::process::Command::new("git")
                    .arg("submodule")
                    .arg("update")
                    .arg("--init")
                    .arg("--recursive")
                    .current_dir(&path)
                    .status()
                    .expect("Failed to run git submodule update");
                if !status.success() {
                    pb.println(format!("{}{}{}", "Failed to update submodules: ".red(), path.display(), String::from_utf8_lossy(&output.stderr)));
                }
                pb.println(format!("{}: {}", path.display().to_string().green(), String::from_utf8_lossy(&output.stdout)));
            });
            pb.finish_and_clear();
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
