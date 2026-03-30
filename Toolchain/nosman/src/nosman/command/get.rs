use std::{fs, io};
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;
use clap::{Arg, ArgAction, ArgMatches};
use colored::Colorize;
use filetime::FileTime;
use indicatif::ProgressBar;
use linked_hash_set::LinkedHashSet;

use crate::nosman::command::{Command, CommandError, CommandResult};
use crate::nosman::command::CommandError::{InvalidArgument, IO};
use crate::nosman::command::init::InitCommand;
use crate::nosman::index::{PackageType, SemVer};
use crate::nosman::common::{download_and_extract};
use crate::nosman::{common, constants};
use crate::nosman::workspace::{Workspace};

pub struct GetCommand {
}

enum EulaFileType {
    Confirmed,
    Unconfirmed
}

fn is_eula_file(relpath: &Path, f_type: EulaFileType) -> bool {
    relpath.starts_with("Engine/") && relpath.ends_with(match f_type {
        EulaFileType::Confirmed => {"EULA_CONFIRMED.json"}
        EulaFileType::Unconfirmed => {"EULA_UNCONFIRMED.json"}
    })
}

impl GetCommand {
    fn move_file_or_dir(src: &PathBuf, dst: &PathBuf) -> Result<(), io::Error> {
        if src.is_dir() {
            let opts = fs_more::directory::DirectoryMoveOptions {
                destination_directory_rule: fs_more::directory::DestinationDirectoryRule::AllowNonEmpty {
                    colliding_file_behaviour: fs_more::file::CollidingFileBehaviour::Overwrite,
                    colliding_subdirectory_behaviour: fs_more::directory::CollidingSubDirectoryBehaviour::Continue,
                },
                allowed_strategies: fs_more::directory::DirectoryMoveAllowedStrategies::Either {
                    copy_and_delete_options: fs_more::directory::DirectoryMoveByCopyOptions::default(),
                }
            };
            let res = fs_more::directory::move_directory(src, dst, opts);
            if let Err(e) = res {
                return Err(std::io::Error::other(e.to_string()));
            }
        } else {
            let res = File::open(src);
            let mut source = res?;
            let res = File::create(dst);
            let mut target = res?;
            std::io::copy(&mut source, &mut target)?;
            // Copy last access and modification times
            let metadata = fs::metadata(src);
            let metadata = metadata?;
            let atime = FileTime::from_last_access_time(&metadata);
            let mtime = FileTime::from_last_modification_time(&metadata);
            filetime::set_file_times(dst, atime, mtime)?;
            let res = rm_rf::remove(src);
            if let Err(e) = res {
                return Err(std::io::Error::other(e.to_string()));
            }
        }
        Ok(())
    }
    fn temp_remove(pb: &ProgressBar, src: &PathBuf, dst: &PathBuf, dont_ask: bool) -> bool {
        if let Some(parent) = dst.parent() {
            if !parent.exists() {
                fs::create_dir_all(parent).unwrap_or_else(|e| panic!("Failed to create directory {}: {}", parent.display(), e));
            }
        }
        let mut res = Self::move_file_or_dir(src, dst);
        if let Err(e) = res.as_ref() {
            pb.println(format!("Unable to remove {}: {}", src.display(), e).red().to_string());
            pb.suspend(|| {
                while common::ask("Retry removing", false, dont_ask) {
                    res = Self::move_file_or_dir(src, dst);
                    if let Err(e) = res.as_ref() {
                        println!("{}", format!("Unable to remove {}: {}", src.display(), e).red());
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
            let res = Self::move_file_or_dir(removed_path, original_path);
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
    fn remove_or_rollback(pb: &ProgressBar, cur_dst_path: &PathBuf, removed_path: &PathBuf, removed: &Vec<(PathBuf, PathBuf)>, new_paths: &LinkedHashSet<PathBuf>, dont_ask: bool) -> Result<(), CommandError> {
        if !Self::temp_remove(pb, cur_dst_path, removed_path, dont_ask) {
            pb.println(format!("Failed to remove file: {}", cur_dst_path.display()).red().to_string());
            Self::rollback(pb, removed, new_paths);
            return Err(IO { file: cur_dst_path.display().to_string(), message: "Failed to remove file".to_string() });
        }
        Ok(())
    }
    pub fn run_get(&self, workspace: &mut Workspace, nodos_name: &String, version: &String, fetch_index: bool, dont_ask: bool, clean_modules: bool) -> CommandResult {
        // If not under a workspace, init
        let path = workspace.root.clone();
        if !workspace.ready() {
            println!("No workspace found, initializing one under {:?}", path);
            InitCommand{}.run_init(workspace, false, false)?;
        }

        let pb: ProgressBar = ProgressBar::new_spinner();
        let progress_tick_duration = Duration::from_millis(100);
        pb.enable_steady_tick(progress_tick_duration);
        pb.set_message(format!("Bringing {}", nodos_name));
        
        if fetch_index {
            pb.println("Updating index");
            pb.finish_and_clear();
            workspace.fetch_package_releases(nodos_name);
            workspace.save()?;
            return self.run_get(workspace, nodos_name, version, false, dont_ask, clean_modules)
        }

        let (artifact_id, release_url, release_version) = {
            let res = if version == "latest" {
                workspace.index_cache.get_latest_release(nodos_name)
            } else {
                let version_prefix = SemVer::parse_from_str(version)
                    .unwrap_or_else(|| panic!("Invalid semantic version: {}", version));
                workspace.index_cache.get_latest_compatible_release(nodos_name, &version_prefix)
            };
            let (package_type, release) = res.ok_or_else(|| InvalidArgument {
                message: format!("No release found for {} version {}", nodos_name, version),
            })?;
            if *package_type != PackageType::Nodos {
                return Err(InvalidArgument { message: format!("Package {} found in the index is not a Nodos package", nodos_name) });
            }
            (release.artifact_id, release.url.clone(), release.version.clone())
        };
        let tmpdir = tempfile::tempdir()?;
        let downloaded_path = tmpdir.path().to_path_buf();
        let download_url = match artifact_id {
            Some(id) => workspace.store_client()?
                .get_artifact_download_url(id)
                .map_err(|e| CommandError::Runtime { message: e.to_string() })?,
            None => release_url,
        };
        pb.println(format!("Downloading and extracting {}-{}", nodos_name, release_version));
        download_and_extract(&download_url, &downloaded_path)?;
        pb.println(format!("Installing {}-{}", nodos_name, release_version));

        // Get current executable's absolute path
        let dst_path = dunce::canonicalize(path)?;
        let current_exe = dunce::canonicalize(std::env::current_exe().unwrap())?;
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
            eula_confirmed_opt = if is_eula_file(relative_path, EulaFileType::Confirmed) {
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
            let cur_dst_path = dst_path.join(relative_path);
            if let Some(eula_confirmed_contents) = eula_confirmed_opt.as_ref() {
                if is_eula_file(relative_path, EulaFileType::Unconfirmed) {
                    // If 'text' field is same as EULA_CONFIRMED.json, remove EULA_UNCONFIRMED.json
                    let mut file = File::open(curr_file_path)?;
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
                                        leftovers.remove(&eula_confirmed_path);
                                        prev_paths.remove(&eula_confirmed_path);
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
                let removed_path = removed_dir.path().join(relative_path);
                pb.set_message(format!("Removing: {}", cur_dst_path.display()));
                Self::remove_or_rollback(&pb, &cur_dst_path, &removed_path, &removed, &new_paths, dont_ask)?;
                removed.push((removed_path, cur_dst_path.clone()));
            }
            pb.set_message(format!("Copying: {}", cur_dst_path.display()));
            {
                let mut res = fs::copy(curr_file_path, &cur_dst_path);
                if let Err(e) = res.as_ref() {
                    pb.println(format!("Error copying {}: {}", cur_dst_path.display(), e).red().to_string());
                    pb.suspend(|| {
                        while common::ask("Retry copying", false, dont_ask) {
                            res = fs::copy(curr_file_path, &cur_dst_path);
                            if let Err(e) = res.as_ref() {
                                println!("{}", format!("Error copying {}: {}",  cur_dst_path.display(), e).red());
                                continue;
                            }
                            break;
                        }
                    });
                }
                if res.is_err() {
                    Self::rollback(&pb, &removed, &new_paths);
                    return Err(IO { file: cur_dst_path.display().to_string(), message: "Failed to copy file".to_string() });
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
        let mut delete_count = 0usize;
        for file in &leftovers {
            if !file.exists() {
                continue;
            }
            let relative_path = file.strip_prefix(&dst_path).unwrap();
            if !clean_modules && relative_path.starts_with("Module/") {
                continue;
            }
            delete_count += 1;
        }
        if !dont_ask && delete_count > 0 {
            pb.suspend(||{
                let prompt = format!(
                    "This update will delete {} existing item{}. Continue?",
                    delete_count,
                    if delete_count == 1 { "" } else { "s" }
                );
                if !common::ask(&prompt, false, dont_ask) {
                    return Err(CommandError::Runtime { message: "Aborted by user".to_string() });
                }
                Ok(())
            })?;
        }
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
                Self::remove_or_rollback(&pb, &file, &removed_path, &removed, &new_paths, dont_ask)?;
                removed.push((removed_path, file));
            }
        }

        if let Some(file_path) = replace_nosman_with {
            pb.println("Updating nosman");
            let res = self_replace::self_replace(&file_path);
            if let Err(e) = res {
                return Err(IO { file: current_exe.display().to_string(), message: format!("Error replacing executable: {}", e) });
            }
        }

        if !clean_modules {
            pb.println("Rescanning...");
            drop(pb);
            workspace.recreate()?;
        }

        Ok(())
    }
}

pub fn get_cli() -> clap::Command {
    clap::Command::new("get").visible_alias("update")
        .about("Brings a Nodos release under workspace (with --workspace option).\n\
    If there is an existing Nodos release, updates it (note that this will remove all installed Nodos engines!)")
        .arg(Arg::new("name")
            .help("Name of the Nodos release to bring. Can be 'nodos' or some bundled version.")
            .long("name")
            .default_value(constants::GET_CMD_DEFAULT_NAME)
        )
        .arg(Arg::new("version")
            .help("Version of the Nodos release to bring. If not provided, the preferred version will be installed.")
            .long("version")
            .short('v')
            .default_value(constants::GET_CMD_DEFAULT_VERSION)
            .required(false)
        )
        .arg(Arg::new("yes_to_all")
            .help("Do not ask for confirmation. Execute default behaviour.")
            .short('y')
            .action(ArgAction::SetTrue)
            .num_args(0)
            .required(false)
        )
        .arg(Arg::new("clean_modules")
            .help("Remove Nodos modules before installing the new release.")
            .action(ArgAction::SetTrue)
            .num_args(0)
            .required(false)
            .long("clean-modules")
        )
}

impl Command for GetCommand {
    fn matched_args<'a>(&self, _workspace: &Workspace, args : &'a ArgMatches) -> Option<&'a ArgMatches> {
        args.subcommand_matches("get")
    }

    fn run(&self, workspace: &mut Workspace, _command_name: Option<&str>, args: &ArgMatches) -> CommandResult {
        let nodos_name = args.get_one::<String>("name").unwrap();
        let version = args.get_one::<String>("version").unwrap();
        let dont_ask = args.get_one::<bool>("yes_to_all").unwrap();
        let clean_modules = args.get_one::<bool>("clean_modules").unwrap();
        self.run_get(workspace, nodos_name, version, true, *dont_ask, *clean_modules)
    }

    fn needs_workspace(&self) -> bool {
        false
    }
}
