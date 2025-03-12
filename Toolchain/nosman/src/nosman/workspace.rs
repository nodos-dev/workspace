use rayon::iter::ParallelIterator;
use std::collections::{HashMap, HashSet};
use std::{fs, io};
use std::cmp::PartialEq;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;
use bitflags::bitflags;
use colored::Colorize;
use crate::nosman::common::get_progress_bar;
use inquire::Select;
use rayon::iter::IntoParallelRefIterator;
use serde::{Deserialize, Serialize};
use crate::nosman::command::{CommandError, CommandResult};
use crate::nosman::{constants};
use crate::nosman::command::CommandError::InvalidArgument;
use crate::nosman::index::{Index, PackageIndexEntry, PackageReleaseEntry, PackageReleases, PackageType, Remote, SemVer};
use crate::nosman::module::{InstalledModule, get_module_manifests, NodeDefinition};
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

#[derive(Debug, Default)]
struct WorkspaceRuntimeParams {
    status: WorkspaceStatus,
    output_mode: OutputMode,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Workspace {
    #[serde(skip_serializing, skip_deserializing)]
    pub root: PathBuf,
    pub remotes: Vec<Remote>,
    pub installed_modules: HashMap<String, HashMap<String, InstalledModule>>,
    pub index_cache: Index,
    #[serde(skip_serializing, skip_deserializing)]
    runtime: WorkspaceRuntimeParams,
}

#[derive(Clone, Copy)]
pub struct RescanFlags(u8);

bitflags! {
    impl RescanFlags: u8 {
        const ScanModules = 0b1;
        const FetchPackageIndex = 0b10;
        const AddDefaultPackageIndexIfNoRemoteExists = 0b100;
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
            installed_modules: HashMap::new(),
            index_cache: Index { packages: HashMap::new() },
            runtime: WorkspaceRuntimeParams { status: WorkspaceStatus::DoesNotExist, output_mode: OutputMode::Default },
        }
    }
    pub fn from_root(path: &PathBuf) -> Workspace {
        let index_filepath = get_nosman_index_filepath_for(&path);
        let exists = index_filepath.exists();
        let mut workspace = Workspace::new_empty(dunce::canonicalize(path).unwrap_or_else(|e| panic!("Failed to canonicalize path {:?}: {}", path, e)));
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
    pub fn get_installed_module(&self, name: &str, version: &str) -> Option<&InstalledModule> {
        match self.installed_modules.get(name) {
            Some(versions) => versions.get(version),
            None => None,
        }
    }
    pub fn get_installed_modules(&self, name: &str) -> Vec<&InstalledModule> {
        let mut res = Vec::new();
        if let Some(versions) = self.installed_modules.get(name) {
            for (_version, module) in versions {
                res.push(module);
            }
        }
        res
    }
    pub fn select_installed_module(&self, module_name: &String) -> Result<&InstalledModule, CommandError> {
        let modules = self.get_installed_modules(module_name);
        let module;
        if modules.len() == 0 {
            return Err(InvalidArgument { message: format!("Module {} not found", module_name) });
        } else if modules.len() > 1 {
            let selection = Select::new(format!("Multiple modules found with name {}. Please select one:", module_name).as_str(), modules)
                .prompt();
            if let Err(e) = selection {
                return Err(InvalidArgument { message: format!("Failed to select module: {}", e) });
            } else {
                module = selection.unwrap();
            }
        } else {
            module = modules[0];
        }
        Ok(module)
    }
    pub fn get_latest_installed_module_within_range(&self, name: &str, version_start: &SemVer, version_end: &SemVer) -> Option<&InstalledModule> {
        let version_list = self.installed_modules.get(name);
        let version_list = version_list?;
        let mut versions: Vec<(&String, &InstalledModule)> = version_list.iter().collect();
        versions.sort_by(|a, b| a.0.cmp(b.0));
        versions.reverse();
        for (version, module) in versions {
            let semver = SemVer::parse_from_string(version);
            if semver.is_none() {
                continue;
            }
            let semver = semver?;
            if semver >= *version_start && semver < *version_end {
                
                return Some(module);
            }
        }
        None
    }
    pub fn get_latest_installed_module_for_version(&self, module_name: &str, requested_version: &str) -> Result<&InstalledModule, String> {
        let semver_res = SemVer::parse_from_string(requested_version);
        if semver_res.is_none() {
            return Err(format!("Invalid semantic version: {}.", requested_version));
        }
        let version_start = semver_res.unwrap();
        if version_start.minor.is_none() {
            return Err("Please provide a minor version too!".to_string());
        }
        let version_end = version_start.get_one_up();
        let res = self.get_latest_installed_module_within_range(module_name, &version_start, &version_end);
        if res.is_none() {
            return Err(format!("No installed version in range [{}, {}) for module {}", version_start.to_string(), version_end.to_string(), module_name));
        }
        Ok(res.unwrap())
    }
    pub fn add(&mut self, module: InstalledModule) {
        let versions = self.installed_modules.entry(module.info.id.name.clone()).or_insert(HashMap::new());
        versions.insert(module.info.id.version.clone(), module);
    }
    pub fn remove(&mut self, name: &str, version: &str) -> CommandResult {
        let res = self.get_installed_module(name, version);
        if res.is_none() {
            return Err(CommandError::InvalidArgument { message: format!("Module {} version {} is not installed", name, version) });
        }
        println!("Removing module {} version {}", name, version);
        let module = res.unwrap();
        fs::remove_dir_all(module.get_module_dir())?;
        if let Some(versions) = self.installed_modules.get_mut(name) {
            versions.remove(version);
        }
        self.save()?;
        println!("{}", format!("Module {} version {} removed successfully", name, version).as_str().green());
        Ok(())
    }
    pub fn remove_all(&mut self) -> CommandResult {
        for (_name, versions) in self.installed_modules.iter() {
            for (_version, module) in versions.iter() {
                println!("Removing module {}", module.info.id);
                fs::remove_dir_all(module.get_module_dir())?;
            }
        }
        self.installed_modules.clear();
        self.save()?;
        println!("{}", "All modules removed successfully".green());
        Ok(())
    }
    fn is_silent(&self) -> bool {
        self.runtime.output_mode == OutputMode::Silent
    }
    pub fn set_output_mode(&mut self, mode: OutputMode) {
        self.runtime.output_mode = mode;
    }
    pub fn scan_modules_in_folder(&mut self, folder: PathBuf, force_replace_in_registry: bool) {
        // Scan folders with .noscfg and .nossys files
        let folder = dunce::canonicalize(&folder).unwrap_or_else(|e| panic!("Failed to canonicalize path {}: {}", folder.display(), e));
        let module_manifests = get_module_manifests(&folder, self.is_silent());

        let pb = get_progress_bar(self.is_silent());
        pb.enable_steady_tick(Duration::from_millis(100));

        pb.println(format!("Found {} modules in {}", module_manifests.len(), folder.display()).as_str().green().to_string());

        for (_ty, path) in module_manifests {
            pb.set_message(format!("Scanning module: {}", path.display()));
            let res = InstalledModule::new(&self, get_rel_path_based_on(&path, &self.root));
            if let Err(msg) = res {
                pb.println(format!("Error while scanning {}: {}", path.display(), msg).red().to_string());
                continue;
            }
            let installed_module = res.unwrap();
            let opt_found = self.get_installed_module(&installed_module.info.id.name, &installed_module.info.id.version);
            if opt_found.is_some() {
                let found = opt_found.unwrap();
                if force_replace_in_registry {
                    pb.println(format!("Updating module entry in registry: {}. {} <=> {}", installed_module.info.id, path.display(), found.manifest_path.display()));
                } else {
                    pb.println(format!("Duplicate module found: {}. {} <=> {}, skipping.", installed_module.info.id, path.display(), found.manifest_path.display()));
                    continue;
                }
            }
            self.add(installed_module);
        }
    }
    pub fn scan_modules(&mut self, force_replace_in_registry: bool) {
       self.scan_modules_in_folder(self.root.clone(), force_replace_in_registry);
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
        if flags.contains(RescanFlags::ScanModules) {
            self.installed_modules.clear();
            self.scan_modules(true);
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
    pub fn get_node_definitions(&self, node_class_name: &String) -> Vec<NodeDefinition> {
        let mut res = Vec::new();
        for versions in self.installed_modules.values() {
            for module in versions.values() {
                if let Some(found) = module.get_node_definition(node_class_name) {
                    res.push(found);
                }
            }
        }
        res
    }
    pub fn get_latest_installed_modules(&self) -> Vec<&InstalledModule> {
        let mut versions_map = HashMap::new();
        for (module_name, versions) in &self.installed_modules {
            for (version, module) in versions {
                if !versions_map.contains_key(module_name) {
                    versions_map.insert(module_name.clone(), module);
                } else {
                    let existing = versions_map.get(module_name).unwrap();
                    let existing_semver = SemVer::parse_from_string(existing.info.id.version.as_str());
                    let new_semver = SemVer::parse_from_string(version.as_str());
                    if existing_semver.is_none() || new_semver.is_none() {
                        continue;
                    }
                    let existing_semver = existing_semver.unwrap();
                    let new_semver = new_semver.unwrap();
                    if new_semver > existing_semver {
                        versions_map.insert(module_name.clone(), module);
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
