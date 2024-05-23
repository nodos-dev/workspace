use std::collections::{HashMap};
use std::{fs, io, path};
use std::sync::OnceLock;
use std::time::Duration;
use colored::Colorize;
use indicatif::{ProgressBar, ProgressStyle};
use serde::{Deserialize, Serialize};
use crate::nosman::command::{CommandError, CommandResult};
use crate::nosman::index::{Index, ModuleType, Remote, SemVer};
use crate::nosman::module::{InstalledModule};
use crate::nosman::path::{get_rel_path_based_on};

#[derive(Serialize, Deserialize, Debug)]
pub struct Workspace {
    #[serde(skip_serializing, skip_deserializing)]
    pub root: path::PathBuf,
    pub remotes: Vec<Remote>,
    pub installed_modules: HashMap<String, HashMap<String, InstalledModule>>,
    pub index: Index,
}

impl Workspace {
    pub fn new(path: path::PathBuf) -> Workspace {
        Workspace {
            root: path,
            remotes: Vec::new(),
            installed_modules: HashMap::new(),
            index: Index { modules: HashMap::new() },
        }
    }
    pub fn from_file(path: path::PathBuf) -> Result<Workspace, io::Error> {
        let file = std::fs::File::open(&path)?;
        let mut workspace: Workspace = serde_json::from_reader(file).unwrap();
        workspace.root = dunce::canonicalize(path.parent().unwrap()).unwrap();
        Ok(workspace)
    }
    pub fn get() -> Result<Workspace, io::Error> {
        Workspace::from_file(current_nosman_file().unwrap())
    }
    pub fn add_remote(&mut self, remote: Remote) {
        self.remotes.push(remote);
    }
    pub fn save(&self) -> Result<(), std::io::Error>{
        let file = std::fs::File::create(&self.root.join(".nosman"))?;
        serde_json::to_writer_pretty(file, self)?;
        Ok(())
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
    pub fn scan_folder(&mut self, folder: path::PathBuf, force_replace_in_registry: bool) {
        // Scan folders with .noscfg and .nossys files
        let pb = ProgressBar::new(0);
        pb.set_style(ProgressStyle::default_spinner()
            .template("{spinner} {wide_msg}").unwrap());
        pb.set_message("Scanning modules...");
        pb.enable_steady_tick(Duration::from_millis(100));

        let mut stack = vec![folder];
        while let Some(current) = stack.pop() {
            let entries = match std::fs::read_dir(&current) {
                Ok(entries) => entries,
                Err(e) => {
                    eprintln!("Error reading directory {:?}: {}", current, e);
                    continue;
                }
            };
            for entry in entries {
                let entry = match entry {
                    Ok(entry) => entry,
                    Err(e) => {
                        eprintln!("Error reading entry: {}", e);
                        continue;
                    }
                };
                let path = entry.path();
                if path.is_dir() {
                    stack.push(path);
                } else {
                    let ext = match path.extension() {
                        Some(ext) => ext,
                        None => continue,
                    };
                    if ext == "noscfg" || ext == "nossys" {
                        // Open file
                        let file = match std::fs::File::open(&path) {
                            Ok(file) => file,
                            Err(e) => {
                                eprintln!("Error reading file {:?}: {}", path, e);
                                continue;
                            }
                        };
                        // Parse file
                        let mut installed_module: InstalledModule = InstalledModule::new(get_rel_path_based_on(&path, &self.root));
                        let res: Result<serde_json::Value, serde_json::Error> = serde_json::from_reader(file);
                        if let Err(e) = res {
                            eprintln!("Error parsing file {:?}: {}", path, e);
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

                        installed_module.module_type = if ext == "noscfg" {
                            ModuleType::Plugin
                        } else {
                            ModuleType::Subsystem
                        };

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
            }
        }
        pb.finish_and_clear();
    }
    pub fn scan(&mut self, force_replace_in_registry: bool) {
       self.scan_folder(self.root.clone(), force_replace_in_registry);
    }
    pub fn rescan(directory: &path::PathBuf, fetch_index: bool) -> Result<Workspace, io::Error> {
        let mut existing_remotes = Vec::new();
        let mut existing_remote_index = Index { modules: HashMap::new() };
        let res = Workspace::from_file(directory.join(".nosman"));
        if res.is_ok() {
            let existing_workspace = res.unwrap();
            existing_remotes = existing_workspace.remotes;
            existing_remote_index = existing_workspace.index;
        }
        let mut workspace = Workspace::new(directory.clone());
        if fetch_index {
            if existing_remotes.is_empty() {
                workspace.add_remote(Remote::new("default", "https://raw.githubusercontent.com/mediaz/mediaz-directory/dev/all_modules.json"));
            } else {
                workspace.remotes = existing_remotes;
            }
            let index = Index::fetch(&workspace);
            workspace.index = index;
        } else {
            // Recover remotes
            workspace.remotes = existing_remotes;
            workspace.index = existing_remote_index;
        }

        workspace.scan(true);

        println!("Saving workspace...");
        workspace.save()?;
        Ok(workspace)
    }
}

pub fn find_root_from(path: &path::PathBuf) -> Option<path::PathBuf> {
    let mut current = path.clone();
    loop {
        if current.join(".nosman").exists() {
            return Some(current);
        }

        if !current.pop() {
            break;
        }
    }
    None
}

static WORKSPACE_ROOT: OnceLock<path::PathBuf> = OnceLock::new();

pub fn set_current(path: path::PathBuf) {
    WORKSPACE_ROOT.set(path).unwrap();
}

pub fn current<'a>() -> Option<&'a path::PathBuf> {
    WORKSPACE_ROOT.get()
}

pub fn current_nosman_file<'a>() -> Option<path::PathBuf> {
    match current() {
        Some(root) => Some(root.join(".nosman")),
        None => None,
    }
}