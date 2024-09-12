use std::collections::{HashMap};
use std::{fs, io};
use std::cmp::PartialEq;
use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::Duration;
use bitflags::bitflags;
use colored::Colorize;
use indicatif::{ProgressBar};
use serde::{Deserialize, Serialize};
use crate::nosman::command::{CommandError, CommandResult};
use crate::nosman::constants;
use crate::nosman::index::{Index, PackageIndexEntry, PackageReleases, Remote, SemVer};
use crate::nosman::module::{InstalledModule, get_module_manifests, NodeDefinition};
use crate::nosman::path::get_rel_path_based_on;

#[derive(Serialize, Deserialize, Debug)]
pub struct Workspace {
    #[serde(skip_serializing, skip_deserializing)]
    pub root: PathBuf,
    pub remotes: Vec<Remote>,
    pub installed_modules: HashMap<String, HashMap<String, InstalledModule>>,
    pub index_cache: Index,
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
        return self.bits() == *other;
    }
}

impl Workspace {
    pub fn new_empty(path: PathBuf) -> Workspace {
        Workspace {
            root: path,
            remotes: Vec::new(),
            installed_modules: HashMap::new(),
            index_cache: Index { packages: HashMap::new() },
        }
    }
    pub fn from_root(path: &PathBuf) -> Result<Workspace, io::Error> {
        let index_filepath = get_nosman_index_filepath_for(&path);
        let file = std::fs::File::open(&index_filepath)?;
        let mut workspace: Workspace = serde_json::from_reader(file).unwrap();
        workspace.root = dunce::canonicalize(path).unwrap();
        Ok(workspace)
    }
    pub fn get_remote_repo_dir(&self, remote: &Remote) -> PathBuf {
        get_nosman_dir_for(&self.root).join("remote").join(remote.name.clone())
    }
    pub fn get() -> Result<Workspace, io::Error> {
        Workspace::from_root(current_root().unwrap())
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
    pub fn get_latest_installed_module_within_range(&self, name: &str, version_start: &SemVer, version_end: &SemVer) -> Option<&InstalledModule> {
        let version_list = self.installed_modules.get(name);
        if version_list.is_none() {
            return None;
        }
        let version_list = version_list.unwrap();
        let mut versions: Vec<(&String, &InstalledModule)> = version_list.iter().collect();
        versions.sort_by(|a, b| a.0.cmp(b.0));
        versions.reverse();
        for (version, module) in versions {
            let semver = SemVer::parse_from_string(version);
            if semver.is_none() {
                return None;
            }
            let semver = semver.unwrap();
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
            return Err(CommandError::InvalidArgumentError { message: format!("Module {} version {} is not installed", name, version) });
        }
        println!("Removing module {} version {}", name, version);
        let module = res.unwrap();
        fs::remove_dir_all(module.get_module_dir())?;
        if let Some(versions) = self.installed_modules.get_mut(name) {
            versions.remove(version);
        }
        self.save()?;
        println!("{}", format!("Module {} version {} removed successfully", name, version).as_str().green());
        Ok(true)
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
        Ok(true)
    }
    pub fn scan_modules_in_folder(&mut self, folder: PathBuf, force_replace_in_registry: bool) {
        // Scan folders with .noscfg and .nossys files
        let folder = dunce::canonicalize(folder).expect("Failed to canonicalize path");
        let module_manifests = get_module_manifests(&folder);

        let pb = ProgressBar::new_spinner();
        pb.enable_steady_tick(Duration::from_millis(100));

        pb.println(format!("Found {} modules in {}", module_manifests.len(), folder.display()).as_str().green().to_string());

        for (ty, path) in module_manifests {
            pb.set_message(format!("Scanning module: {}", path.display()));
            let file = match fs::File::open(&path) {
                Ok(file) => file,
                Err(ref e) => {
                    pb.println(format!("Error reading file {}: {}", path.display(), e).as_str().red().to_string());
                    continue;
                }
            };
            // Parse file
            let mut installed_module: InstalledModule = InstalledModule::new(get_rel_path_based_on(&path, &self.root));
            let res: Result<serde_json::Value, serde_json::Error> = serde_json::from_reader(file);
            if let Err(ref e) = res {
                pb.println(format!("Error parsing file {}: {}", path.display(), e).as_str().red().to_string());
                continue;
            }
            let module = res.unwrap();
            installed_module.info = serde_json::from_value(module["info"].clone()).expect(format!("Failed to parse module info from {}", path.display()).as_str());

            // Check custom_types field
            if let Some(custom_types) = module["custom_types"].as_array() {
                for custom_type_file in custom_types {
                    let type_file = path.parent().unwrap().join(custom_type_file.as_str().unwrap());
                    if !type_file.exists() {
                        pb.println(format!("Module {} ({}) references a non-existent data schema file: {}", installed_module.info.id.name, path.display(), type_file.display()).as_str().red().to_string());
                        continue;
                    }
                    installed_module.type_schema_files.push(get_rel_path_based_on(&type_file.canonicalize().unwrap(), &self.root));
                }
            }

            // Check include folder
            if path.parent().unwrap().join("Include").exists() {
                installed_module.public_include_folder = Some(get_rel_path_based_on(&path.parent().unwrap().join("Include").canonicalize().unwrap(), &self.root));
            }

            pb.set_message(format!("Scanning modules: {}", installed_module.info.id));
            installed_module.module_type = ty;

            let opt_found = self.get_installed_module(&installed_module.info.id.name, &installed_module.info.id.version);
            if opt_found.is_some() {
                let found = opt_found.unwrap();
                if force_replace_in_registry {
                    pb.println(format!("Updating module entry in registry: {}. {} <=> {}", installed_module.info.id, path.display(), found.config_path.display()));
                } else {
                    pb.println(format!("Duplicate module found: {}. {} <=> {}, skipping.", installed_module.info.id, path.display(), found.config_path.display()));
                    continue;
                }
            }
            self.add(installed_module);
        }
    }
    pub fn scan_modules(&mut self, force_replace_in_registry: bool) {
       self.scan_modules_in_folder(self.root.clone(), force_replace_in_registry);
    }
    pub fn new(directory: &PathBuf) -> Result<Workspace, CommandError> {
        let mut workspace = Workspace::new_empty(directory.clone());
        workspace.rescan(RescanFlags::all())?;
        workspace.save()?;
        Ok(workspace)
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
        Ok(true)
    }
    pub fn fetch_remotes(&mut self, add_default_remote: bool) -> Result<(), io::Error>{
        if self.remotes.is_empty() {
            if add_default_remote {
                self.add_remote(Remote::new("default", constants::DEFAULT_PACKAGE_INDEX_REPO));
            } else {
                return Ok(());
            }
        }
        self.index_cache = Index::fetch(self);
        self.save()
    }
    pub fn fetch_package_releases(&mut self, package_name: &str) {
        let pb = ProgressBar::new_spinner();
        pb.enable_steady_tick(Duration::from_millis(100));
        pb.set_message(format!("Fetching package index for {}", package_name));
        for remote in &self.remotes {
            pb.set_message(format!("Fetching remote {}", remote.name));
            let res = remote.fetch(&self);
            if let Err(e) = res {
                pb.println(format!("Failed to fetch remote: {}", e));
                continue;
            }
            let package_list: Vec<PackageIndexEntry> = res.unwrap();
            pb.println(format!("Fetched {} packages from remote {}", package_list.len(), remote.name));
            // For each module in list
            for package in package_list {
                if package.name != *package_name {
                    continue;
                }
                let res = reqwest::blocking::get(&package.releases_url);
                if let Err(e) = res {
                    pb.println(format!("Failed to fetch package releases for {}: {}", package_name, e));
                    continue;
                }
                let res = res.unwrap().json();
                if let Err(e) = res {
                    pb.println(format!("Failed to parse package releases for {}: {}", package_name, e));
                    continue;
                }
                let versions: PackageReleases = res.unwrap();
                pb.set_message(format!("Remote {}: Found {} releases for package {}", remote.name, versions.releases.len(), versions.name));
                // For each version in list
                for release in versions.releases {
                    self.index_cache.add_package(&versions.name, package.package_type.clone(), release);
                }
            }
        }
    }
    pub fn get_node_definitions(&self, node_class_name: &String) -> Vec<NodeDefinition> {
        let mut res = Vec::new();
        for (_name, versions) in &self.installed_modules {
            for (_version, module) in versions {
                // Read config file as JSON, and read node definition files
                let config_file = fs::File::open(&module.config_path);
                if config_file.is_err() {
                    continue;
                }
                let config_file = config_file.unwrap();
                let config: serde_json::Value = serde_json::from_reader(config_file).expect("Failed to parse config file");
                let node_defs_rel_paths = config["node_definitions"].as_array();
                if node_defs_rel_paths.is_none() {
                    continue;
                }
                for node_defs_rel_path in node_defs_rel_paths.unwrap() {
                    let node_defs_path = module.get_module_dir().join(node_defs_rel_path.as_str().unwrap());
                    let node_defs_file_content = fs::read_to_string(&node_defs_path);
                    if let Err(e) = node_defs_file_content {
                        eprintln!("{}", format!("Failed to read node definitions file ({}): {}", node_defs_path.display(), e).red());
                        continue;
                    }
                    let node_defs_file_content = node_defs_file_content.unwrap();
                    // Remove BOM
                    let node_defs_file_content = node_defs_file_content.trim_start_matches('\u{FEFF}');
                    let node_defs: serde_json::Value = serde_json::from_str(&node_defs_file_content).expect(format!("Failed to parse node definitions file: {}", node_defs_path.display()).as_str());
                    let nodes_json_array = node_defs.get("nodes").expect("Missing 'nodes' field in node definitions file").as_array().expect("'nodes' field is not an array");
                    let mut index = 0;
                    for node_json in nodes_json_array {
                        let mut curr_class_name = node_json["class_name"].as_str().expect(format!("Missing 'class_name' field in node definition in {}", node_defs_path.display()).as_str()).to_string();
                        // If class name is not prefixed with module name, prefix it
                        if !curr_class_name.starts_with(module.info.id.name.as_str()) {
                            curr_class_name = format!("{}.{}", module.info.id.name, curr_class_name);
                        }
                        if curr_class_name == *node_class_name {
                            res.push(NodeDefinition {
                                class_name: curr_class_name.to_string(),
                                defined_in: node_defs_path.clone(),
                                index,
                                node_defs_json: node_defs.clone(),
                                owner: module.clone(),
                            });
                        }
                        index += 1;
                    }
                }
            }
        }
        res
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

static WORKSPACE_ROOT: OnceLock<PathBuf> = OnceLock::new();

pub fn set_current_root(path: PathBuf) {
    WORKSPACE_ROOT.set(path).unwrap();
}

pub fn current_root<'a>() -> Option<&'a PathBuf> {
    WORKSPACE_ROOT.get()
}

pub fn get_nosman_dir_for(path: &PathBuf) -> PathBuf {
    path.join(".nosman")
}

pub fn get_nosman_index_filepath_for(path: &PathBuf) -> PathBuf {
    get_nosman_dir_for(path).join("index")
}

pub fn get_nosman_index_filepath<'a>() -> Option<PathBuf> {
    match current_root() {
        Some(root) => Some(get_nosman_index_filepath_for(root)),
        None => None,
    }
}

pub fn exists() -> bool {
    let res = get_nosman_index_filepath();
    if res.is_none() {
        return false;
    }
    res.unwrap().exists()
}

pub fn exists_in(path: &PathBuf) -> bool {
    get_nosman_index_filepath_for(path).exists()
}