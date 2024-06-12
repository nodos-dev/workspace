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
use crate::nosman::{util, workspace};
use crate::nosman::workspace::Workspace;

pub struct GetCommand {
}

impl GetCommand {
    fn run_get(&self, path: &PathBuf, nodos_name: &String, version: Option<&String>, fetch_index: bool, do_default: bool) -> CommandResult {
        // If not under a workspace, init
        if !workspace::exists_in(path) {
            println!("No workspace found, initializing one under {:?}", path);
            let res = InitCommand{}.run_init(path);
            if res.is_err() {
                return res;
            }
        }

        let pb: ProgressBar = ProgressBar::new_spinner();
        let progress_tick_duration = Duration::from_millis(100);
        pb.enable_steady_tick(progress_tick_duration);
        pb.set_message(format!("Getting {}", nodos_name));
        let mut workspace = Workspace::get()?;

        if fetch_index {
            pb.println("Updating index");
            pb.finish_and_clear();
            workspace.fetch_package_releases(nodos_name);
            workspace.save()?;
            return self.run_get(path, nodos_name, version, false, do_default)
        }

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
            return if version.is_none() {
                Err(InvalidArgumentError { message: format!("No release found for {}", nodos_name) })
            } else {
                Err(InvalidArgumentError { message: format!("No release found for {} version {}", nodos_name, version.unwrap()) })
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
        let dst_path = dunce::canonicalize(path).unwrap();
        let current_exe = dunce::canonicalize(std::env::current_exe().unwrap()).unwrap();

        // Remove all files
        let glob_prev = globwalk::GlobWalkerBuilder::from_patterns(&dst_path, &["**"]).min_depth(1).build().unwrap();
        let mut prev_paths: HashSet<PathBuf> = HashSet::new();
        for entry in glob_prev {
            let entry = entry.unwrap();
            let curr_path = entry.path().to_path_buf();
            prev_paths.insert(curr_path.clone());
        }

        // Get all files in the path
        let glob_new = globwalk::GlobWalkerBuilder::from_patterns(&downloaded_path, &["**"]).min_depth(1).build().unwrap();
        for entry in glob_new {
            let entry = entry.unwrap();
            let curr_file_path = entry.path();
            let relative_path = curr_file_path.strip_prefix(&downloaded_path).unwrap();
            let cur_dst_path = dst_path.join(&relative_path);
            prev_paths.remove(&cur_dst_path);
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
                // Check if files are same
                if util::check_file_contents_same(&curr_file_path.to_path_buf(), &cur_dst_path) {
                    continue;
                }
            }
            pb.set_message(format!("Copying: {}", cur_dst_path.display()));
            let res = std::fs::copy(curr_file_path, &cur_dst_path);
            if let Err (e) = res {
                pb.println(format!("Error copying {}: {}", cur_dst_path.display(), e).red().to_string());
                pb.suspend(|| {
                    // Ask user if we should retry
                    while util::ask("Retry copying", false, do_default) {
                        let res = std::fs::copy(curr_file_path, &cur_dst_path);
                        if res.is_ok() {
                            break;
                        }
                        println!("{}", format!("Error copying {}: {}", cur_dst_path.display(), res.err().unwrap()).red().to_string());
                    }
                });
            }
        }

        for file in prev_paths {
            pb.set_message(format!("Removing: {}", file.display()));
            if !file.exists() {
                continue;
            }
            let res = rm_rf::remove(&file);
            if let Err(e) = res {
                pb.println(format!("Unable to remove {}: {}", file.display(), e).red().to_string());
                pb.suspend(|| {
                    // Ask user if we should retry
                    while util::ask("Retry removing", false, do_default) {
                        let res = rm_rf::remove(&file);
                        if res.is_ok() {
                            break;
                        }
                        println!("{}", format!("Unable to remove {}: {}", file.display(), res.err().unwrap()).red().to_string());
                    }
                });
            }
        }

        Ok(true)
    }
}

impl Command for GetCommand {
    fn matched_args<'a>(&self, args : &'a ArgMatches) -> Option<&'a ArgMatches> {
        return args.subcommand_matches("get");
    }

    fn run(&self, args: &ArgMatches) -> CommandResult {
        let nodos_name = args.get_one::<String>("name").unwrap();
        let version = args.get_one::<String>("version");
        let do_default = args.get_one::<bool>("yes_to_all").unwrap();
        self.run_get(&workspace::current_root().unwrap(), nodos_name, version, true, *do_default)
    }

    fn needs_workspace(&self) -> bool {
        false
    }
}
