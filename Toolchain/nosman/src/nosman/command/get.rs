use std::{fs, io};
use std::fs::File;
use std::io::{Error, Read, Write};
use std::path::{PathBuf};
use std::time::Duration;
use clap::{ArgMatches};
use colored::Colorize;
use filetime::FileTime;
use indicatif::ProgressBar;
use linked_hash_set::LinkedHashSet;

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
    fn move_file_or_dir(src: &PathBuf, dst: &PathBuf) -> Result<(), io::Error> {
        if src.is_dir() {
            let opts = fs_more::directory::DirectoryMoveOptions {
                destination_directory_rule: fs_more::directory::DestinationDirectoryRule::AllowNonEmpty {
                    existing_destination_file_behaviour: fs_more::file::ExistingFileBehaviour::Overwrite,
                    existing_destination_subdirectory_behaviour: fs_more::directory::ExistingSubDirectoryBehaviour::Continue,
                },
            };
            let res = fs_more::directory::move_directory(src, dst, opts);
            if let Err(e) = res {
                return Err(Error::new(io::ErrorKind::Other, e.to_string()));
            }
        } else {
            let res = File::open(&src);
            if let Err(e) = res {
                return Err(e);
            }
            let mut source = res.unwrap();
            let res = File::create(&dst);
            if let Err(e) = res {
                return Err(e);
            }
            let mut target = res.unwrap();
            let res = std::io::copy(&mut source, &mut target);
            if let Err(e) = res {
                return Err(e);
            }
            // Copy last access and modification times
            let metadata = fs::metadata(src);
            if let Err(e) = metadata {
                return Err(e);
            }
            let metadata = metadata.unwrap();
            let atime = FileTime::from_last_access_time(&metadata);
            let mtime = FileTime::from_last_modification_time(&metadata);
            let res = filetime::set_file_times(dst, atime, mtime);
            if let Err(e) = res {
                return Err(e);
            }
            let res = rm_rf::remove(src);
            if let Err(e) = res {
                return Err(Error::new(io::ErrorKind::Other, e.to_string()));
            }
        }
        Ok(())
    }
    fn temp_remove(pb: &ProgressBar, src: &PathBuf, dst: &PathBuf, do_default: bool) -> bool {
        if let Some(parent) = dst.parent() {
            if !parent.exists() {
                fs::create_dir_all(parent).expect(format!("Failed to create directory {:?}", parent).as_str());
            }
        }
        let mut res = Self::move_file_or_dir(src, dst);
        if let Err(e) = res.as_ref() {
            pb.println(format!("Unable to remove {}: {}", src.display(), e).red().to_string());
            pb.suspend(|| {
                while common::ask("Retry removing", false, do_default) {
                    res = Self::move_file_or_dir(src, dst);
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
    fn rollback(pb: &ProgressBar, removed: &Vec<(PathBuf, PathBuf)>, new_paths: &LinkedHashSet<PathBuf>) {
        pb.println("Rolling back changes".yellow().to_string());
        pb.set_message("Rolling back changes".yellow().to_string());
        let mut remove_order = new_paths.clone();
        Self::sort_paths(&mut remove_order);
        for path in remove_order {
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
            let res = Self::move_file_or_dir(&removed_path, &original_path);
            if let Err(e) = res {
                pb.println(format!("Failed to rollback: {}", e).red().to_string());
            }
        }
        pb.println("Rollback complete".yellow().to_string());
    }
    fn sort_paths(paths: &mut LinkedHashSet<PathBuf>) {
        // Sort paths such that children come before parents
        let mut paths_vec: Vec<PathBuf> = paths.iter().cloned().collect();
        paths_vec.sort_by(|a, b| {
            let a = a.components().count();
            let b = b.components().count();
            b.cmp(&a)
        });
        paths.clear();
        for path in paths_vec {
            paths.insert(path);
        }
    }
    fn remove_or_rollback(pb: &ProgressBar, cur_dst_path: &PathBuf, removed_path: &PathBuf, removed: &Vec<(PathBuf, PathBuf)>, new_paths: &LinkedHashSet<PathBuf>, do_default: bool) -> Result<(), CommandError> {
        if !Self::temp_remove(&pb, &cur_dst_path, &removed_path, do_default) {
            pb.println(format!("Failed to remove file: {}", cur_dst_path.display()).red().to_string());
            Self::rollback(&pb, removed, new_paths);
            return Err(IOError { file: cur_dst_path.display().to_string(), message: "Failed to remove file".to_string() });
        }
        Ok(())
    }
    fn run_get(&self, path: &PathBuf, nodos_name: &String, version: Option<&String>, fetch_index: bool, do_default: bool, clean_modules: bool) -> CommandResult {
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
            return self.run_get(path, nodos_name, version, false, do_default, clean_modules)
        }

        let res;
        if let Some(version) = version {
            let version_start = SemVer::parse_from_string(version).expect(format!("Invalid semantic version: {}", version).as_str());
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
        let mut prev_paths: LinkedHashSet<PathBuf> = LinkedHashSet::new();
        let mut removed: Vec<(PathBuf, PathBuf)> = Vec::new(); // (removed, original)
        let mut new_paths: LinkedHashSet<PathBuf> = LinkedHashSet::new();
        let mut eula_confirmed_opt = None;
        for entry in glob_prev {
            let entry = entry.unwrap();
            let curr_path = entry.path().to_path_buf();
            prev_paths.insert(curr_path.clone());
            let relative_path = curr_path.strip_prefix(&dst_path).unwrap();
            // If file is Engine/*/EULA_CONFIRMED.json, save it and check if text changed.
            eula_confirmed_opt = if relative_path.starts_with("Engine/") && relative_path.ends_with("EULA_CONFIRMED.json") {
                let mut file = File::open(&curr_path)?;
                let mut contents = String::new();
                file.read_to_string(&mut contents)?;
                if eula_confirmed_opt.is_none() {
                    Some(contents)
                } else {
                    // Multiple EULA_CONFIRMED.json files found
                    None
                }
            } else {
                eula_confirmed_opt
            };
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
            if let Some(eula_confirmed_contents) = eula_confirmed_opt.as_ref() {
                if relative_path.starts_with("Engine/") && relative_path.ends_with("EULA_UNCONFIRMED.json") {
                    // If 'text' field is same as EULA_CONFIRMED.json, remove EULA_UNCONFIRMED.json
                    let mut file = File::open(&curr_file_path)?;
                    let mut contents = String::new();
                    file.read_to_string(&mut contents)?;
                    // Check "text" field in JSON
                    let res= serde_json::from_str::<serde_json::Value>(&contents);
                    if let Err(e) = res {
                        pb.println(format!("Error parsing {}: {}", curr_file_path.display(), e).yellow().to_string());
                    }
                    else
                    {
                        let json: serde_json::Value = res.unwrap();
                        if let Some(text) = json.get("license_text") {
                            let res = serde_json::from_str::<serde_json::Value>(eula_confirmed_contents);
                            if let Err(e) = res {
                                pb.println(format!("Error parsing new EULA_CONFIRMED.json: {}", e).yellow().to_string());
                            }
                            else {
                                let eula_confirmed_json: serde_json::Value = res.unwrap();
                                if let Some(eula_confirmed_text) = eula_confirmed_json.get("license_text") {
                                    if text == eula_confirmed_text {
                                        pb.println("The accepted EULA has not changed. Skipping EULA confirmation.".yellow().to_string());
                                        // Write eula_confirmed_contents to EULA_CONFIRMED.json
                                        let eula_confirmed_path = cur_dst_path.parent().unwrap().join("EULA_CONFIRMED.json");
                                        let mut file = File::create(&eula_confirmed_path)?;
                                        file.write_all(eula_confirmed_contents.as_bytes())?;
                                        continue;
                                    }
                                }
                            }
                        }
                    }
                }
            }
            leftovers.remove(&cur_dst_path);
            if curr_file_path.is_dir() {
                // Create dir if it doesn't exist
                if !cur_dst_path.exists() {
                    new_paths.insert(cur_dst_path.clone());
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
                new_paths.insert(cur_dst_path.clone());
            }
        }
        pb.println("Removing previous files");
        for path in prev_paths {
            let mut parent_opt = path.parent();
            while let Some(parent) = parent_opt {
                if leftovers.contains(parent) {
                    leftovers.remove(&path);
                }
                parent_opt = parent.parent();
            }
        }
        Self::sort_paths(&mut leftovers);
        for file in leftovers {
            pb.set_message(format!("Removing: {}", file.display()));
            if !file.exists() {
                continue;
            }
            let relative_path = file.strip_prefix(&dst_path).unwrap();
            if !clean_modules && relative_path.starts_with("Module/") {
                // TODO: Don't simply skip removing, remove the older one.
                pb.println(format!("Skip deleting: {}", file.display()).yellow().dimmed().to_string());
                continue;
            }
            {
                let removed_path = removed_dir.path().join(relative_path);
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

        if !clean_modules {
            pb.println("Rescanning...");
            drop(pb);
            let _ = Workspace::new(&dst_path)?;
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
        let clean_modules = args.get_one::<bool>("clean_modules").unwrap();
        self.run_get(&workspace::current_root().unwrap(), nodos_name, version, true, *do_default, *clean_modules)
    }

    fn needs_workspace(&self) -> bool {
        false
    }
}
