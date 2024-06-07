use std::path::PathBuf;
use std::time::Duration;
use clap::{ArgMatches};
use indicatif::ProgressBar;
use sysinfo::System;

use crate::nosman::command::{Command, CommandResult};
use crate::nosman::command::CommandError::{InvalidArgumentError, IOError};
use crate::nosman::command::init::InitCommand;
use crate::nosman::index::{PackageType, SemVer};
use crate::nosman::util::download_and_extract;
use crate::nosman::workspace;
use crate::nosman::workspace::Workspace;

pub struct GetCommand {
}

impl GetCommand {
    fn run_get(&self, path: &PathBuf, nodos_name: &String, version: Option<&String>) -> CommandResult {
        // If not under a workspace, init
        if !workspace::exists() {
            println!("No workspace found, initializing one under {:?}", path);
            let res = InitCommand{}.run_init();
            if res.is_err() {
                return res;
            }
        }

        let pb: ProgressBar = ProgressBar::new_spinner();
        pb.enable_steady_tick(Duration::from_millis(100));
        pb.set_message(format!("Getting {}", nodos_name));

        let workspace = Workspace::get()?;
        let res;
        if let Some(version) = version {
            let version_start = SemVer::parse_from_string(version).unwrap();
            if version_start.minor.is_none() {
                return Err(InvalidArgumentError { message: "Please provide a minor version too!".to_string() });
            }
            let version_end = version_start.get_one_up();
            res = workspace.index_cache.get_latest_compatible_release_within_range(nodos_name, &version_start, &version_end);
            if res.is_none() {
                return Err(InvalidArgumentError { message: format!("No release found for {} version {}", nodos_name, version) });
            }
        }
        else {
            res = workspace.index_cache.get_latest_release(nodos_name);
            if res.is_none() {
                return Err(InvalidArgumentError { message: format!("No release found for {}", nodos_name) });
            }
        }
        let (package_type, release) = res.unwrap();
        if *package_type != PackageType::Nodos {
            return Err(InvalidArgumentError { message: format!("Package {} found in the index is not a Nodos package", nodos_name) });
        }
        let tmpdir = tempfile::tempdir()?;
        let tmpdir_path = tmpdir.path().to_path_buf();
        pb.println(format!("Downloading and extracting {}-{}", nodos_name, release.version));
        let res = download_and_extract(&release.url, &tmpdir_path);
        if res.is_err() {
            return Err(res.err().unwrap());
        }
        pb.println(format!("Installing {}-{}", nodos_name, release.version));

        // Get current executable's absolute path
        let current_exe = std::env::current_exe()?;

        // Get all files in the path
        let all = globwalk::GlobWalkerBuilder::from_patterns(&tmpdir_path, &["**"]).build().unwrap();
        for entry in all {
            let entry = entry.unwrap();
            let path = entry.path();
            let relative_path = path.strip_prefix(&tmpdir_path).unwrap();
            let dest_path = path.join(&relative_path);
            if path.is_dir() {
                // Create dir if it doesn't exist
                std::fs::create_dir_all(&dest_path)?;
                continue;
            }
            // If file is same as current executable, use self_replace
            if dest_path == current_exe {
                let res = self_replace::self_replace(&path);
                if res.is_err() {
                    return Err(InvalidArgumentError { message: format!("Error replacing executable: {}", res.err().unwrap()) });
                }
                continue;
            }
            // If destination file exists and someone is using it, kill them.
            if dest_path.exists() {
                let res = std::fs::remove_file(&dest_path);
                if res.is_err() {
                    // Initialize the system struct
                    let mut system = System::new_all();
                    system.refresh_all();

                    // Iterate over all processes
                    for (pid, process) in system.processes() {
                        // Check if the process has the file open
                        if process.cmd().iter().any(|arg| arg == dest_path.to_str().unwrap()) {
                            // Kill the process
                            let res = process.kill();
                            if !res {
                                return Err(IOError { file: dest_path.display().to_string(), message: format!("Unable to kill process: {}", pid.to_string()) });
                            }
                        }
                    }
                    // Try removing the file again
                    let res = std::fs::remove_file(&dest_path);
                    if res.is_err() {
                        return Err(IOError { file: dest_path.display().to_string(), message: format!("Unable to remove: {}", res.err().unwrap()) });
                    }
                }
            }
            std::fs::copy(&path, &dest_path)?;
        }

        //
        // // Remove all files in the path
        // if path.exists() {
        //     std::fs::remove_dir_all(&path)?;
        // }
        // std::fs::create_dir_all(&path)?;
        // std::fs::rename(tmpdir_path, path)?;

        Ok(true)
    }
}

impl Command for GetCommand {
    fn matched_args<'a>(&self, args : &'a ArgMatches) -> Option<&'a ArgMatches> {
        return args.subcommand_matches("get");
    }

    fn run(&self, args: &ArgMatches) -> CommandResult {
        let path = PathBuf::from(args.get_one::<String>("path").unwrap());
        let nodos_name = args.get_one::<String>("name").unwrap();
        let version = args.get_one::<String>("version");
        self.run_get(&path, nodos_name, version)
    }

    fn needs_workspace(&self) -> bool {
        false
    }
}
