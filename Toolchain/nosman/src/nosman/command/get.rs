use std::collections::HashSet;
use std::path::PathBuf;
use std::time::Duration;
use clap::{ArgMatches};
use colored::Colorize;
use indicatif::ProgressBar;

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
    fn run_get(&self, path: &PathBuf, nodos_name: &String, version: Option<&String>, fetch_index_if_not_found: bool) -> CommandResult {
        // If not under a workspace, init
        if !workspace::exists_in(path) {
            println!("No workspace found, initializing one under {:?}", path);
            let res = InitCommand{}.run_init();
            if res.is_err() {
                return res;
            }
        }

        let pb: ProgressBar = ProgressBar::new_spinner();
        pb.enable_steady_tick(Duration::from_millis(100));
        pb.set_message(format!("Getting {}", nodos_name));

        let mut workspace = Workspace::get()?;
        let res;
        if let Some(version) = version {
            let version_start = SemVer::parse_from_string(version).unwrap();
            if version_start.minor.is_none() {
                return Err(InvalidArgumentError { message: "Please provide a minor version too!".to_string() });
            }
            let version_end = version_start.get_one_up();
            res = workspace.index_cache.get_latest_compatible_release_within_range(nodos_name, &version_start, &version_end);
        }
        else {
            res = workspace.index_cache.get_latest_release(nodos_name);
        }
        if res.is_none() {
            return if fetch_index_if_not_found {
                pb.println("Updating index");
                pb.finish_and_clear();
                workspace.fetch_remotes(false)?;
                self.run_get(path, nodos_name, version, false)
            } else {
                if version.is_none() {
                    Err(InvalidArgumentError { message: format!("No release found for {}", nodos_name) })
                } else {
                    Err(InvalidArgumentError { message: format!("No release found for {} version {}", nodos_name, version.unwrap()) })
                }
            }
        }
        let (package_type, release) = res.unwrap();
        if *package_type != PackageType::Nodos {
            return Err(InvalidArgumentError { message: format!("Package {} found in the index is not a Nodos package", nodos_name) });
        }
        let tmpdir = tempfile::tempdir()?;
        let downloaded_path = tmpdir.path().to_path_buf();
        pb.println(format!("Downloading and extracting {}-{}", nodos_name, release.version));
        let res = download_and_extract(&release.url, &downloaded_path);
        if let Err(e) = res {
            return Err(e);
        }
        pb.println(format!("Installing {}-{}", nodos_name, release.version));

        // Get current executable's absolute path
        let dst_path = path.canonicalize().unwrap();
        let current_exe = std::env::current_exe().unwrap().canonicalize().unwrap();

        // Remove all files
        let glob_prev = globwalk::GlobWalkerBuilder::from_patterns(&dst_path, &["**"]).min_depth(1).build().unwrap();
        for entry in glob_prev {
            let entry = entry.unwrap();
            let curr_path = entry.path().to_path_buf();
            pb.set_message(format!("Removing: {}", curr_path.display()));
            let res;
            if curr_path.is_dir() {
                res = std::fs::remove_dir_all(&curr_path);
            } else {
                res = std::fs::remove_file(&curr_path);
            }
            if curr_path == current_exe {
                continue;
            }
            if let Err(e) = res {
                pb.println(format!("Unable to remove {}: {}", curr_path.display(), e).yellow().to_string());
            }
        }

        // Get all files in the path
        let glob_new = globwalk::GlobWalkerBuilder::from_patterns(&downloaded_path, &["**"]).min_depth(1).build().unwrap();
        for entry in glob_new {
            let entry = entry.unwrap();
            let curr_file_path = entry.path();
            let relative_path = curr_file_path.strip_prefix(&downloaded_path).unwrap();
            let cur_dst_path = dst_path.join(&relative_path);
            if curr_file_path.is_dir() {
                // Create dir if it doesn't exist
                std::fs::create_dir_all(&cur_dst_path)?;
                continue;
            }
            // If file is same as current executable, use self_replace
            if cur_dst_path == current_exe {
                pb.println("Updating nosman");
                let res = self_replace::self_replace(&curr_file_path);
                if let Err(e) = res {
                    return Err(IOError { file: cur_dst_path.display().to_string(), message: format!("Error replacing executable: {}", e) });
                }
                continue;
            }
            // If destination file exists and someone is using it, kill them.
            if cur_dst_path.exists() {
                let res = std::fs::remove_file(&cur_dst_path);
                // TODO: Kill processes that uses the file.
                if let Err(e) = res {
                    return Err(IOError { file: cur_dst_path.display().to_string(), message: format!("Unable to remove file: {}", e) });
                }
            }
            pb.set_message(format!("Copying: {}", cur_dst_path.display()));
            std::fs::copy(curr_file_path, &cur_dst_path)?;
        }

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
        self.run_get(&path, nodos_name, version, true)
    }

    fn needs_workspace(&self) -> bool {
        false
    }
}
