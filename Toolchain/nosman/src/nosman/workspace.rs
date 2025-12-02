use rayon::iter::ParallelIterator;
use std::collections::{HashMap, HashSet};
use std::{fs, io};
use std::cmp::PartialEq;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration};
use bitflags::bitflags;
use colored::Colorize;
use crate::nosman::common::get_progress_bar;
use inquire::Select;
use rayon::iter::IntoParallelRefIterator;
use serde::{Deserialize, Serialize};
use crate::nosman::command::{CommandError, CommandResult};
use crate::nosman::constants;
use crate::nosman::command::CommandError::InvalidArgument;
use crate::nosman::index::{Index, PackageIndexEntry, PackageReleaseEntry, PackageReleases, PackageType, Remote, SemVer};
use crate::nosman::plugin::{NodeDefinition, PluginEntry};
use crate::nosman::package::{get_package_manifests, LocalPackageEntry};
use crate::nosman::path::get_rel_path_based_on;

#[derive(Serialize, Deserialize, Debug, PartialEq, Default)]
pub enum WorkspaceStatus {
    DoesNotExist,
    FailedToOpen,
    #[default]
    Ready
}

#[derive(Debug, PartialEq, Default)]
pub enum OutputMode {
    Silent,
    #[default]
    Default,
}

#[derive(Debug, PartialEq)]
pub enum AutoRescanResult {
    /// No action was needed - workspace was up to date
    NoActionNeeded,
    /// Full workspace rescan was performed due to missing index file
    FullRescanMissingIndex,
    /// Full workspace rescan was performed due to missing manifest files
    FullRescanMissingManifests,
    /// Partial rescan was performed on specific folders with updated manifests
    PartialRescanUpdatedManifests(Vec<PathBuf>),
}

#[derive(Debug, Default)]
struct WorkspaceRuntimeParams {
    status: WorkspaceStatus,
    output_mode_stack: Vec<OutputMode>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Workspace {
    #[serde(skip_serializing, skip_deserializing)]
    pub root: PathBuf,
    pub remotes: Vec<Remote>,
    #[serde(alias = "installed_modules")]
    pub packages: HashMap<String, HashMap<String, LocalPackageEntry>>,
    #[serde(skip_serializing, skip_deserializing)]
    pub index_cache: Index,
    #[serde(skip_serializing, skip_deserializing)]
    runtime: WorkspaceRuntimeParams,
}

#[derive(Clone, Copy)]
pub struct RescanFlags(u8);

bitflags! {
    impl RescanFlags: u8 {
        const ScanPackages = 0b1;
        const FetchPackageIndex = 0b10;
        const AddDefaultPackageIndexIfNoRemoteExists = 0b100;
    }
}

#[derive(Clone, Copy)]
pub struct ScanPackagesFlags(u8);

bitflags! {
    impl ScanPackagesFlags: u8 {
        const ForceReplaceInRegistry = 0b1;
        const RegisterCommands = 0b10;
    }
}

impl PartialEq<u8> for RescanFlags {
    fn eq(&self, other: &u8) -> bool {
        self.bits() == *other
    }
}

fn fetch_releases_mt(workspace: &Workspace, package_names: Option<HashSet<String>>) -> HashMap<String, (PackageType, Vec<PackageReleaseEntry>)> {
    let releases = Mutex::new(HashMap::new());
    let pb = get_progress_bar(workspace.is_silent());
    pb.enable_steady_tick(Duration::from_millis(100));
    pb.set_message("Fetching package index...");
    workspace.remotes.par_iter().for_each(|remote| {
        pb.set_message(format!("Fetching remote {}", remote.name));
        let res = remote.fetch(&workspace);
        if let Err(e) = res {
            pb.println(format!("Failed to fetch remote: {}", e));
            return;
        }
        let package_list: Vec<PackageIndexEntry> = res.unwrap();
        pb.println(format!("Fetched {} packages from remote {}", package_list.len(), remote.name));
        package_list.par_iter().for_each(|package| {
            if let Some(ref package_names) = package_names {
                if !package_names.contains(&package.name) {
                    return;
                }
            }
            let res = reqwest::blocking::get(&package.releases_url);
            if let Err(e) = res {
                pb.println(format!("Failed to fetch package releases for {}: {}", package.name, e));
                return;
            }
            let res = res.unwrap().json();
            if let Err(e) = res {
                pb.println(format!("Failed to parse package releases for {}: {}", package.name, e));
                return;
            }
            let versions: PackageReleases = res.unwrap();
            pb.set_message(format!("Remote {}: Found {} releases for package {}", remote.name, versions.releases.len(), versions.name));
            // For each version in list
            for release in versions.releases {
                let mut map = releases.lock().unwrap();
                let entry = map.entry(versions.name.clone()).or_insert((package.package_type.clone(), Vec::new()));
                entry.1.push(release);
            }
        });
    });
    releases.into_inner().unwrap()
}

impl Workspace {
    fn new_empty(path: PathBuf) -> Workspace {
        Workspace {
            root: path,
            remotes: Vec::new(),
            packages: HashMap::new(),
            index_cache: Index { packages: HashMap::new() },
            runtime: WorkspaceRuntimeParams { status: WorkspaceStatus::DoesNotExist, output_mode_stack: vec![OutputMode::Default] },
        }
    }
    pub fn from_root(path: &PathBuf) -> Workspace {
        let index_filepath = get_nosman_index_filepath_for(&path);
        let exists = index_filepath.exists();
        let mut workspace = Workspace::new_empty(std::path::absolute(path).unwrap_or_else(|e| panic!("Failed to get absolute path from {:?}: {}", path, e)));
        if !exists {
            workspace.runtime.status = WorkspaceStatus::DoesNotExist;
            return workspace;
        }
        let res = fs::File::open(&index_filepath);
        if let Ok(file) = res {
            let mut parsed_ws = match serde_json::from_reader(file) {
                Ok(workspace) => workspace,
                Err(e) => {
                    println!("{}", format!("Failed to parse workspace file: {}. Rescanning...", e).red());
                    workspace.rescan(RescanFlags::all()).unwrap_or_else(|e| panic!("Failed to rescan workspace: {}", e));
                    workspace.save().unwrap_or_else(|e| panic!("Failed to save workspace: {}", e));
                    workspace
                }
            };
            parsed_ws.root = dunce::canonicalize(path).unwrap_or_else(|e| panic!("Failed to canonicalize path {:?}: {}", path, e));
            parsed_ws.runtime.status = WorkspaceStatus::Ready;
            workspace = parsed_ws;
        } else {
            println!("{}", format!("Failed to open workspace file: {}", index_filepath.display()).red());
            workspace.runtime.status = WorkspaceStatus::FailedToOpen;
        }
        workspace
    }
    pub fn ready(&self) -> bool {
        self.runtime.status == WorkspaceStatus::Ready
    }
    pub fn get_remote_repo_dir(&self, remote: &Remote) -> PathBuf {
        get_nosman_dir_for(&self.root).join("remote").join(remote.name.clone())
    }
    pub fn add_remote(&mut self, remote: Remote) {
        self.remotes.push(remote);
    }
    pub fn find_remote(&self, name: &str) -> Option<&Remote> {
        self.remotes.iter().find(|r| r.name == name)
    }
    pub fn save(&self) -> Result<(), std::io::Error>{
        if !get_nosman_dir_for(&self.root).exists() {
            fs::create_dir(get_nosman_dir_for(&self.root))?;
        }
        let file = fs::File::create(self.get_nosman_index_filepath())?;
        serde_json::to_writer_pretty(file, self)?;
        Ok(())
    }
    pub fn get_nosman_index_filepath(&self) -> PathBuf {
        get_nosman_index_filepath_for(&self.root)
    }
    pub fn get_package(&self, name: &str, version: &str) -> Option<&LocalPackageEntry> {
        match self.packages.get(name) {
            Some(versions) => versions.get(version),
            None => None,
        }
    }
    pub fn get_packages(&self, name: &str) -> Vec<&LocalPackageEntry> {
        let mut res = Vec::new();
        if let Some(versions) = self.packages.get(name) {
            for (_version, package) in versions {
                res.push(package);
            }
        }
        res
    }
    pub fn get_or_select_package(&self, package_name: &String) -> Result<LocalPackageEntry, CommandError> {
        let packages = self.get_packages(package_name);
        let package;
        if packages.len() == 0 {
            return Err(InvalidArgument { message: format!("Package {} not found", package_name) });
        } else if packages.len() > 1 {
            let selection = Select::new(format!("Multiple packages found with name {}. Please select one:", package_name).as_str(), packages)
                .prompt();
            if let Err(e) = selection {
                return Err(InvalidArgument { message: format!("Failed to select module: {}", e) });
            } else {
                package = selection.unwrap();
            }
        } else {
            package = packages[0];
        }
        Ok(self.absolutize_paths(package))
    }
    pub fn absolutize_paths(&self, package: &LocalPackageEntry) -> LocalPackageEntry {
        let mut new_package = package.clone();
        if new_package.manifest_path.is_relative() {
            new_package.manifest_path = self.root.join(&new_package.manifest_path);
        }
        if let Some(ref path) = new_package.public_include_folder {
            if path.is_relative() {
                new_package.public_include_folder = Some(self.root.join(path));
            }
        }
        for path in &mut new_package.type_schema_files {
            if path.is_relative() {
                *path = self.root.join(&*path);
            }
        }
        new_package
    }
    pub fn get_latest_local_package_within_range(&self, name: &str, version_start: &SemVer, version_end: &SemVer) -> Option<&LocalPackageEntry> {
        let version_list = self.packages.get(name);
        let version_list = version_list?;
        let mut versions: Vec<(&String, &LocalPackageEntry)> = version_list.iter().collect();
        versions.sort_by(|a, b| a.0.cmp(b.0));
        versions.reverse();
        for (version, package) in versions {
            let semver = SemVer::parse_from_str(version);
            if semver.is_none() {
                continue;
            }
            let semver = semver?;
            if semver >= *version_start && semver < *version_end {
                return Some(package);
            }
        }
        None
    }
    pub fn get_latest_local_package_for_version(&self, module_name: &str, requested_version: &str) -> Result<&LocalPackageEntry, String> {
        let semver_res = SemVer::parse_from_str(requested_version);
        if semver_res.is_none() {
            return Err(format!("Invalid semantic version: {}.", requested_version));
        }
        let version_start = semver_res.unwrap();
        if version_start.minor.is_none() {
            return Err("Please provide a minor version too!".to_string());
        }
        let version_end = version_start.get_one_up();
        let res = self.get_latest_local_package_within_range(module_name, &version_start, &version_end);
        if res.is_none() {
            return Err(format!("No installed version in range [{}, {}) for package {}", version_start.to_string(), version_end.to_string(), module_name));
        }
        Ok(res.unwrap())
    }
    pub fn get_latest_absent_release_for(&self, name: &str, requested_version: &str) -> Result<Option<(&PackageType, &PackageReleaseEntry)>, CommandError> {
        // If the version is not a valid semantic version, return Error
        let semver = SemVer::parse_from_str(requested_version);
        if semver.is_none() {
            return Err(InvalidArgument { message: format!("{} is not a valid semantic version", requested_version) });
        }
        let semver = semver.unwrap();
        let version_end = semver.get_one_up();
        let installed = self.get_latest_local_package_within_range(name, &semver, &version_end);
        if installed.is_some() {
            return Ok(None);
        }
        let res = self.index_cache.get_latest_compatible_release_within_range(name, &semver, &version_end);
        if res.is_none() {
            return Err(InvalidArgument { message: format!("No releases found for package {} in range [{}, {})", name, semver.to_string(), version_end.to_string()) });
        }
        Ok(Some(res.unwrap()))
    }
    pub fn add(&mut self, package: LocalPackageEntry) {
        let versions = self.packages.entry(package.info.id.name.clone()).or_insert(HashMap::new());
        versions.insert(package.info.id.version.clone(), package);
    }
    pub fn remove(&mut self, name: &str, version: &str) -> CommandResult {
        let res = self.get_package(name, version);
        if res.is_none() {
            return Err(CommandError::InvalidArgument { message: format!("Package {} version {} is not installed", name, version) });
        }
        println!("Removing package {} version {}", name, version);
        let package = res.unwrap();
        fs::remove_dir_all(package.get_package_root())?;
        if let Some(versions) = self.packages.get_mut(name) {
            versions.remove(version);
        }
        self.save()?;
        println!("{}", format!("Module {} version {} removed successfully", name, version).as_str().green());
        Ok(())
    }
    pub fn remove_all(&mut self) -> CommandResult {
        for (_name, versions) in self.packages.iter() {
            for (_version, module) in versions.iter() {
                println!("Removing module {}", module.info.id);
                fs::remove_dir_all(module.get_package_root())?;
            }
        }
        self.packages.clear();
        self.save()?;
        println!("{}", "All modules removed successfully".green());
        Ok(())
    }
    pub fn is_silent(&self) -> bool {
        self.runtime.output_mode_stack.last().unwrap_or(&OutputMode::Default) == &OutputMode::Silent
    }
    pub fn push_output_mode(&mut self, mode: OutputMode) {
        self.runtime.output_mode_stack.push(mode);
    }
    pub fn pop_output_mode(&mut self) {
        if self.runtime.output_mode_stack.len() > 1 {
            self.runtime.output_mode_stack.pop();
        }
    }
    pub fn with_output_mode_scoped<T, F>(&mut self, mode: OutputMode, f: F) -> T 
    where 
        F: FnOnce(&mut Self) -> T 
    {
        self.push_output_mode(mode);
        let result = f(self);
        self.pop_output_mode();
        result
    }
    pub fn scan_packages_in_folder(&mut self, folder: PathBuf, flags: ScanPackagesFlags) {
        // Scan folders with .noscfg and .nossys files
        let folder = dunce::canonicalize(&folder).unwrap_or_else(|e| panic!("Failed to canonicalize path {}: {}", folder.display(), e));
        let package_manifests = get_package_manifests(&folder, self.is_silent());

        let pb = get_progress_bar(self.is_silent());
        pb.enable_steady_tick(Duration::from_millis(100));

        pb.println(format!("Found {} packages in {}", package_manifests.len(), folder.display()).as_str().green().to_string());

        for (ty, path) in package_manifests {
            pb.set_message(format!("Scanning: {}", path.display()));
            let res = LocalPackageEntry::new(&self, get_rel_path_based_on(&path, &self.root), ty, flags.contains(ScanPackagesFlags::RegisterCommands));
            if let Err(msg) = res {
                pb.println(format!("Error while scanning {}: {}", path.display(), msg).red().to_string());
                continue;
            }
            let package = res.unwrap();
            let opt_found = self.get_package(&package.info.id.name, &package.info.id.version);
            if opt_found.is_some() {
                let found = opt_found.unwrap();
                if flags.contains(ScanPackagesFlags::ForceReplaceInRegistry) {
                    pb.println(format!("Updating package entry in registry: {}. {} <=> {}", package.info.id, path.display(), found.manifest_path.display()));
                } else {
                    pb.println(format!("Duplicate module found: {}. {} <=> {}, skipping.", package.info.id, path.display(), found.manifest_path.display()));
                    continue;
                }
            }
            self.add(package);
        }
    }
    pub fn scan_packages(&mut self, flags: ScanPackagesFlags) {
       self.scan_packages_in_folder(self.root.clone(), flags);
    }
    pub fn recreate(&mut self) -> Result<(), CommandError> {
        self.rescan(RescanFlags::all())?;
        self.save()?;
        Ok(())
    }
    pub fn rescan(&mut self, flags: RescanFlags) -> CommandResult {
        if flags.contains(RescanFlags::FetchPackageIndex) {
            self.index_cache.packages.clear();
            self.fetch_remotes(flags.contains(RescanFlags::AddDefaultPackageIndexIfNoRemoteExists))?;
        }
        if flags.contains(RescanFlags::ScanPackages) {
            self.packages.clear();
            self.scan_packages(ScanPackagesFlags::ForceReplaceInRegistry | ScanPackagesFlags::RegisterCommands);
        }
        self.save()?;
        self.runtime.status = WorkspaceStatus::Ready;
        Ok(())
    }
    pub fn fetch_remotes(&mut self, add_default_remote: bool) -> Result<(), io::Error>{
        if self.remotes.is_empty() {
            if add_default_remote {
                self.add_remote(Remote::new("default", constants::DEFAULT_PACKAGE_INDEX_REPO));
            } else {
                return Ok(());
            }
        }
        self.index_cache = Index::fetch(self, self.is_silent());
        self.save()
    }
    pub fn fetch_package_releases(&mut self, package_name: &str) {
        let mut package_names = HashSet::new();
        package_names.insert(package_name.to_string());
        self.fetch_releases(Some(package_names));
    }
    pub fn fetch_releases(&mut self, package_names: Option<HashSet<String>>) {
        let res = fetch_releases_mt(self, package_names);
        for (name, (package_type, releases)) in res {
            for release in releases {
                self.index_cache.add_package(&name, package_type.clone(), release);
            }
        }
    }
    pub fn fetch_latest_versions(&mut self) -> Vec<(&String, &PackageReleaseEntry)> {
        println!("Fetching latest versions...");
        self.fetch_releases(None);
        let mut res = Vec::new();
        for name in self.index_cache.packages.keys() {
            if let Some(entry) = self.index_cache.get_latest_release(name) {
                res.push((name, entry.1));
            }
        }
        res
    }
    pub fn get_node_definitions(&self, node_class_name: &String, nodos_version: &Option<SemVer>) -> Vec<NodeDefinition> {
        let mut res = Vec::new();
        for versions in self.packages.values() {
            for package in versions.values() {
                let package_abs = self.absolutize_paths(package);
                if package_abs.package_type == PackageType::Plugin {
                    if let Ok(plugin) = PluginEntry::new(package_abs) {
                        if let Some(found) = plugin.get_node_definition(node_class_name, nodos_version) {
                            res.push(found);
                        }
                    }
                }
            }
        }
        res
    }
    pub fn get_latest_local_packages(&self) -> Vec<&LocalPackageEntry> {
        let mut versions_map = HashMap::new();
        for (package_name, versions) in &self.packages {
            for (version, module) in versions {
                if !versions_map.contains_key(package_name) {
                    versions_map.insert(package_name.clone(), module);
                } else {
                    let existing = versions_map.get(package_name).unwrap();
                    let existing_semver = SemVer::parse_from_str(existing.info.id.version.as_str());
                    let new_semver = SemVer::parse_from_str(version.as_str());
                    if existing_semver.is_none() || new_semver.is_none() {
                        continue;
                    }
                    let existing_semver = existing_semver.unwrap();
                    let new_semver = new_semver.unwrap();
                    if new_semver > existing_semver {
                        versions_map.insert(package_name.clone(), module);
                    }
                }
            }
        }
        let mut ret = vec![];
        for (_name, module) in versions_map {
            ret.push(module);
        }
        ret
    }
    pub fn exit_if_required_but_not_found(&self, required: bool) {
        if required && !self.ready() {
            eprintln!("Workspace required but not found in {}", self.root.display());
            std::process::exit(1);
        }
    }
    #[allow(dead_code)]
    pub fn get_local_package_count(&self) -> usize {
        let mut count = 0;
        for (_name, versions) in self.packages.iter() {
            count += versions.len();
        }
        count
    }

    /// Automatically rescans the workspace if needed based on manifest file status.
    /// - If any manifest files are missing, performs a full rescan
    /// - If manifest files are only updated, rescans only the folders containing those files
    /// Returns an AutoRescanResult indicating what action was taken

    pub fn auto_rescan_if_needed(&mut self) -> Result<AutoRescanResult, CommandError> {
        // If workspace is not ready, return early - no auto-rescan needed
        if !self.ready() {
            return Ok(AutoRescanResult::NoActionNeeded);
        }

        // Get the modification time of the workspace index file
        let index_path = self.get_nosman_index_filepath();
        let index_modified_time = match fs::metadata(&index_path).and_then(|m| m.modified()) {
            Ok(time) => time,
            Err(_) => {
                self.rescan(RescanFlags::ScanPackages)?;
                return Ok(AutoRescanResult::FullRescanMissingIndex);
            }
        };

        // Collect all packages for parallel processing
        let all_packages: Vec<&LocalPackageEntry> = self.packages
            .values()
            .flat_map(|versions| versions.values())
            .collect();

        // Use parallel iterator to check manifest files
        use rayon::prelude::*;
        use std::sync::atomic::{AtomicBool, Ordering};
        
        let missing_manifests = AtomicBool::new(false);
        let updated_folders = Mutex::new(Vec::new());
        
        all_packages.par_iter().for_each(|package| {
            // If we already found missing manifests, skip further processing
            if missing_manifests.load(Ordering::Relaxed) {
                return;
            }
            
            let manifest_path = self.root.join(&package.manifest_path);
            
            // Check if manifest file still exists
            if !manifest_path.exists() {
                missing_manifests.store(true, Ordering::Relaxed);
                return;
            }
            
            // Check if manifest file has been modified since the index was last saved
            if let Ok(metadata) = fs::metadata(&manifest_path) {
                if let Ok(modified_time) = metadata.modified() {
                    if modified_time > index_modified_time {
                        // Get the folder containing this manifest
                        if let Some(folder) = manifest_path.parent() {
                            let folder = folder.to_path_buf();
                            let mut folders = updated_folders.lock().unwrap();
                            if !folders.contains(&folder) {
                                folders.push(folder);
                            }
                        }
                    }
                }
            }
        });

        let missing_manifests = missing_manifests.load(Ordering::Relaxed);
        let updated_folders = updated_folders.into_inner().unwrap();

        // If any manifests are missing, do a full rescan
        if missing_manifests {
            self.rescan(RescanFlags::ScanPackages)?;
            return Ok(AutoRescanResult::FullRescanMissingManifests);
        }

        // If no updates, nothing to do
        if updated_folders.is_empty() {
            return Ok(AutoRescanResult::NoActionNeeded);
        }

        // Store the folders for the result
        let updated_folders_result = updated_folders.clone();

        // Rescan only the updated folders
        self.with_output_mode_scoped(OutputMode::Silent, |ws| {
            for folder in &updated_folders {
                // Remove packages from this folder first
                let folder_relative = get_rel_path_based_on(folder, &ws.root);
                ws.packages.retain(|_name, versions| {
                    versions.retain(|_version, package| {
                        let package_folder = package.manifest_path.parent()
                            .map(|p| p.to_path_buf())
                            .unwrap_or_else(|| PathBuf::new());
                        package_folder != folder_relative
                    });
                    !versions.is_empty()
                });

                // Rescan this specific folder
                ws.scan_packages_in_folder(folder.clone(), ScanPackagesFlags::ForceReplaceInRegistry | ScanPackagesFlags::RegisterCommands);
            }
        });

        // Save the updated workspace
        self.save()?;
        self.runtime.status = WorkspaceStatus::Ready;
        
        Ok(AutoRescanResult::PartialRescanUpdatedManifests(updated_folders_result))
    }
}

pub fn find_root_from(path: &PathBuf) -> Option<PathBuf> {
    let mut current = path.clone();
    loop {
        if get_nosman_index_filepath_for(&current).exists() {
            return Some(current);
        }
        if !current.pop() {
            break;
        }
    }
    None
}

pub fn get_nosman_dir_for(path: &PathBuf) -> PathBuf {
    path.join(".nosman")
}

pub fn get_nosman_index_filepath_for(path: &PathBuf) -> PathBuf {
    get_nosman_dir_for(path).join("index")
}
