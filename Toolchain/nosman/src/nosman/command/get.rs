use std::collections::HashSet;
use std::fs::{File};
use std::{fs};
use std::path::{PathBuf};
use std::time::Duration;
use clap::{ArgMatches};
use colored::Colorize;
use indicatif::ProgressBar;

use crate::nosman::command::{Command, CommandError, CommandResult};
use crate::nosman::command::CommandError::{InvalidArgumentError, IOError};
use crate::nosman::command::init::InitCommand;
use crate::nosman::index::{PackageType, SemVer};
use crate::nosman::common::{download_and_extract};
use crate::nosman::{common, workspace};
use crate::nosman::workspace::Workspace;

pub struct GetCommand {
}

impl GetCommand {
    fn force_move(src: &PathBuf, dst: &PathBuf) -> Result<(), rm_rf::Error> {
        if src.is_dir() {
            let res = fs::rename(src, dst);
            if let Err(e) = res {
                return Err(rm_rf::Error::IoError(e));
            }
            return Ok(());
        }
        let res = File::open(&src);
        if let Err(e) = res {
            return Err(rm_rf::Error::IoError(e));
        }
        let mut source = res.unwrap();
        let res = File::create(&dst);
        if let Err(e) = res {
            return Err(rm_rf::Error::IoError(e));
        }
        let mut target = res.unwrap();
        let res = std::io::copy(&mut source, &mut target);
        if let Err(e) = res {
            return Err(rm_rf::Error::IoError(e));
        }
        let res = rm_rf::remove(src);
        if let Err(e) = res {
            return Err(e);
        }
        Ok(())
    }
    fn temp_remove(pb: &ProgressBar, src: &PathBuf, dst: &PathBuf, do_default: bool) -> bool {
        if let Some(parent) = dst.parent() {
            if !parent.exists() {
                fs::create_dir_all(parent).expect(format!("Failed to create directory {:?}", parent).as_str());
            }
        }
        let mut res = Self::force_move(src, dst);
        if let Err(e) = res.as_ref() {
            pb.println(format!("Unable to remove {}: {}", src.display(), e).red().to_string());
            pb.suspend(|| {
                while common::ask("Retry removing", false, do_default) {
                    res = Self::force_move(src, dst);
                    if let Err(e) = res.as_ref() {
                        println!("{}", format!("Unable to remove {}: {}", src.display(), e).red().to_string());
                        continue;
                    }
                    break;
                }
            });
        }
        res.is_ok()
    }
    fn rollback(pb: &ProgressBar, removed: &Vec<(PathBuf, PathBuf)>, new_paths: &Vec<PathBuf>) {
        pb.println("Rolling back changes".yellow().to_string());
        pb.set_message("Rolling back changes".yellow().to_string());
        // Reorder new_paths to remove children first
        let mut new_paths = new_paths.clone();
        new_paths.sort_by(|a, b| b.cmp(a));
        for path in new_paths {
            pb.println(format!("Rolling back: Remove {}", path.display()).yellow().dimmed().to_string());
            if path.is_dir() {
                let _ = fs::remove_dir_all(path);
            }
            else {
                let _ = fs::remove_file(&path);
            }
        }
        for (removed_path, original_path) in removed {
            pb.println(format!("Rolling back: Restore {}", original_path.display()).yellow().dimmed().to_string());
            let err = fs::rename(&removed_path, &original_path);
            if let Err(e) = err {
                pb.println(format!("Failed to rollback: {}", e).red().to_string());
            }
        }
        pb.println("Rollback complete".yellow().to_string());
    }
    fn remove_or_rollback(pb: &ProgressBar, cur_dst_path: &PathBuf, removed_path: &PathBuf, removed: &Vec<(PathBuf, PathBuf)>, new_paths: & Vec<PathBuf>, do_default: bool) -> Result<(), CommandError> {
        if !Self::temp_remove(&pb, &cur_dst_path, &removed_path, do_default) {
            pb.println(format!("Failed to remove file: {}", cur_dst_path.display()).red().to_string());
            Self::rollback(&pb, removed, new_paths);
            return Err(IOError { file: cur_dst_path.display().to_string(), message: "Failed to remove file".to_string() });
        }
        Ok(())
    }
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
        pb.set_message(format!("Bringing {}", nodos_name));
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
        let removed_dir = tempfile::tempdir()?;

        let glob_prev = globwalk::GlobWalkerBuilder::from_patterns(&dst_path, &["**"]).min_depth(1).build().unwrap();
        let mut prev_paths: HashSet<PathBuf> = HashSet::new();
        let mut removed: Vec<(PathBuf, PathBuf)> = Vec::new(); // (removed, original)
        let mut new_paths: Vec<PathBuf> = Vec::new();
        for entry in glob_prev {
            let entry = entry.unwrap();
            let curr_path = entry.path().to_path_buf();
            prev_paths.insert(curr_path.clone());
        }

        // Get all files in the path
        let mut replace_nosman_with = None;
        let mut leftovers = prev_paths.clone();
        let glob_new = globwalk::GlobWalkerBuilder::from_patterns(&downloaded_path, &["**"]).min_depth(1).build().unwrap();
        for entry in glob_new {
            let entry = entry.unwrap();
            let curr_file_path = entry.path();
            let relative_path = curr_file_path.strip_prefix(&downloaded_path).unwrap();
            let cur_dst_path = dst_path.join(&relative_path);
            leftovers.remove(&cur_dst_path);
            if curr_file_path.is_dir() {
                // Create dir if it doesn't exist
                if !cur_dst_path.exists() {
                    new_paths.push(cur_dst_path.clone());
                }
                fs::create_dir_all(&cur_dst_path)?;
                continue;
            }
            // If file is same as current executable, use self_replace
            if cur_dst_path == current_exe {
                replace_nosman_with = Some(curr_file_path.to_path_buf());
                continue;
            }
            // If destination file exists and someone is using it, kill them.
            if cur_dst_path.exists() {
                // Check if files are same
                if common::check_file_contents_same(&curr_file_path.to_path_buf(), &cur_dst_path) {
                    continue;
                }
                let removed_path = removed_dir.path().join(&relative_path);
                pb.set_message(format!("Removing: {}", cur_dst_path.display()));
                Self::remove_or_rollback(&pb, &cur_dst_path, &removed_path, &removed, &new_paths, do_default)?;
                removed.push((removed_path, cur_dst_path.clone()));
            }
            pb.set_message(format!("Copying: {}", cur_dst_path.display()));
            {
                let mut res = fs::copy(curr_file_path, &cur_dst_path);
                if let Err(e) = res.as_ref() {
                    pb.println(format!("Error copying {}: {}", cur_dst_path.display(), e).red().to_string());
                    pb.suspend(|| {
                        while common::ask("Retry copying", false, do_default) {
                            res = fs::copy(curr_file_path, &cur_dst_path);
                            if let Err(e) = res.as_ref() {
                                println!("{}", format!("Error copying {}: {}",  cur_dst_path.display(), e).red().to_string());
                                continue;
                            }
                            break;
                        }
                    });
                }
                if res.is_err() {
                    Self::rollback(&pb, &removed, &new_paths);
                    return Err(IOError { file: cur_dst_path.display().to_string(), message: "Failed to copy file".to_string() });
                }
                new_paths.insert(0, cur_dst_path.clone());
            }
        }
        for path in prev_paths {
            let mut parent_opt = path.parent();
            while let Some(parent) = parent_opt {
                if leftovers.contains(parent) {
                    leftovers.remove(&parent.to_path_buf());
                }
                parent_opt = parent.parent();
            }
        }
        for file in leftovers {
            pb.set_message(format!("Removing: {}", file.display()));
            if !file.exists() {
                continue;
            }
            {
                let removed_path = removed_dir.path().join(file.strip_prefix(&dst_path).unwrap());
                Self::remove_or_rollback(&pb, &file, &removed_path, &removed, &new_paths, do_default)?;
                removed.push((removed_path, file));
            }
        }

        if let Some(file_path) = replace_nosman_with {
            pb.println("Updating nosman");
            let res = self_replace::self_replace(&file_path);
            if let Err(e) = res {
                return Err(IOError { file: current_exe.display().to_string(), message: format!("Error replacing executable: {}", e) });
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
