use std::collections::{HashMap};
use std::{fs, io, path};
use std::sync::OnceLock;
use std::time::Duration;
use colored::Colorize;
use indicatif::{ProgressBar};
use serde::{Deserialize, Serialize};
use crate::nosman::command::{CommandError, CommandResult};
use crate::nosman::constants;
use crate::nosman::index::{Index, Remote, SemVer};
use crate::nosman::module::{InstalledModule, get_module_manifests};
use crate::nosman::path::{get_rel_path_based_on};

#[derive(Serialize, Deserialize, Debug)]
pub struct Workspace {
    #[serde(skip_serializing, skip_deserializing)]
    pub root: path::PathBuf,
    pub remotes: Vec<Remote>,
    pub installed_modules: HashMap<String, HashMap<String, InstalledModule>>,
    pub index_cache: Index,
}

impl Workspace {
    pub fn new(path: path::PathBuf) -> Workspace {
        Workspace {
            root: path,
            remotes: Vec::new(),
            installed_modules: HashMap::new(),
            index_cache: Index { packages: HashMap::new() },
        }
    }
    pub fn from_root(path: &path::PathBuf) -> Result<Workspace, io::Error> {
        let index_filepath = get_nosman_index_filepath_for(&path);
        let file = std::fs::File::open(&index_filepath)?;
        let mut workspace: Workspace = serde_json::from_reader(file).unwrap();
        workspace.root = dunce::canonicalize(path).unwrap();
        Ok(workspace)
    }
    pub fn get_remote_repo_dir(&self, remote: &Remote) -> path::PathBuf {
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
    pub fn get_nosman_index_filepath(&self) -> path::PathBuf {
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
        for (name, versions) in self.installed_modules.iter() {
            for (version, module) in versions.iter() {
                println!("Removing module {} version {}", name, version);
                fs::remove_dir_all(module.get_module_dir())?;
            }
        }
        self.installed_modules.clear();
        self.save()?;
        println!("{}", "All modules removed successfully".green());
        Ok(true)
    }
    pub fn scan_modules_in_folder(&mut self, folder: path::PathBuf, force_replace_in_registry: bool) {
        // Scan folders with .noscfg and .nossys files
        let pb = ProgressBar::new_spinner();
        pb.enable_steady_tick(Duration::from_millis(100));

        let module_manifests = get_module_manifests(&folder, &pb);
        for (ty, path) in module_manifests {
            pb.set_message(format!("Scanning module: {}", path.display()));
            let file = match fs::File::open(&path) {
                Ok(file) => file,
                Err(ref e) => {
                    pb.set_message(format!("Error reading file {:?}: {}", path, e).as_str().red().to_string());
                    continue;
                }
            };
            // Parse file
            let mut installed_module: InstalledModule = InstalledModule::new(get_rel_path_based_on(&path, &self.root));
            let res: Result<serde_json::Value, serde_json::Error> = serde_json::from_reader(file);
            if let Err(ref e) = res {
                pb.println(format!("Error parsing file {:?}: {}", path, e).as_str().red().to_string());
                continue;
            }
            let module = res.unwrap();
            installed_module.info = serde_json::from_value(module["info"].clone()).unwrap();

            // Check custom_types field
            if let Some(custom_types) = module["custom_types"].as_array() {
                for custom_type_file in custom_types {
                    installed_module.type_schema_files.push(get_rel_path_based_on(&path.parent().unwrap().join(custom_type_file.as_str().unwrap()).canonicalize().unwrap(), &self.root));
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
        pb.finish_and_clear();
    }
    pub fn scan_modules(&mut self, force_replace_in_registry: bool) {
       self.scan_modules_in_folder(self.root.clone(), force_replace_in_registry);
    }
    pub fn rescan(directory: &path::PathBuf, fetch_index: bool) -> Result<Workspace, io::Error> {
        let mut existing_remotes = Vec::new();
        let mut existing_remote_index = Index { packages: HashMap::new() };
        let res = Workspace::from_root(directory);
        if res.is_ok() {
            let existing_workspace = res.unwrap();
            existing_remotes = existing_workspace.remotes;
            existing_remote_index = existing_workspace.index_cache;
        }
        let mut workspace = Workspace::new(directory.clone());
        if fetch_index {
            if existing_remotes.is_empty() {
                workspace.add_remote(Remote::new("default", constants::DEFAULT_PACKAGE_INDEX_REPO));
            } else {
                workspace.remotes = existing_remotes;
            }
            let index = Index::fetch(&workspace);
            workspace.index_cache = index;
        } else {
            // Recover remotes
            workspace.remotes = existing_remotes;
            workspace.index_cache = existing_remote_index;
        }

        workspace.scan_modules(true);

        println!("Saving workspace...");
        workspace.save()?;
        Ok(workspace)
    }
}

pub fn find_root_from(path: &path::PathBuf) -> Option<path::PathBuf> {
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

static WORKSPACE_ROOT: OnceLock<path::PathBuf> = OnceLock::new();

pub fn set_current_root(path: path::PathBuf) {
    WORKSPACE_ROOT.set(path).unwrap();
}

pub fn current_root<'a>() -> Option<&'a path::PathBuf> {
    WORKSPACE_ROOT.get()
}

pub fn get_nosman_dir_for(path: &path::PathBuf) -> path::PathBuf {
    path.join(".nosman")
}

pub fn get_nosman_index_filepath_for(path: &path::PathBuf) -> path::PathBuf {
    get_nosman_dir_for(path).join("index")
}

pub fn get_nosman_index_filepath<'a>() -> Option<path::PathBuf> {
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