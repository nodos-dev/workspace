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
    fn run_dev_pull(&self, dirs: Vec<PathBuf>) -> Result<bool, CommandError> {
        // Scan module folder for git repositories and run "git pull" on them
        let pb = ProgressBar::new_spinner();
        pb.enable_steady_tick(Duration::from_millis(100));
        pb.set_message("Scanning for git repositories...");
        let mut git_dirs = Vec::new();
        for dir in dirs {
            let mut stack = Vec::new();
            stack.push(dir);
            while let Some(dir) = stack.pop() {
                if dir.join(".git").is_dir() {
                    git_dirs.push(dir);
                    continue;
                }
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
        }
        pb.set_message("Pulling...");
        git_dirs.par_iter().for_each(|path| {
            pb.println(format!("{}{}", "Pulling: ".yellow(), path.display()));
            let output = std::process::Command::new("git")
                .arg("pull")
                .arg("--autostash")
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

impl Command for DevPullCommand {
    fn matched_args<'a>(&self, args : &'a ArgMatches) -> Option<&'a ArgMatches> {
        if let Some(subcommand) = args.subcommand_matches("dev") {
            return subcommand.subcommand_matches("pull");
        }
        None
    }

    fn run(&self, args: &ArgMatches) -> CommandResult {
        let dirs: Vec<&String> = args.get_many::<String>("dir").unwrap_or_default().collect();
        let dirs: Vec<PathBuf> = dirs.iter().map(|s| PathBuf::from(s)).collect();
        self.run_dev_pull(dirs)
    }

    fn needs_workspace(&self) -> bool {
        false
    }
}
