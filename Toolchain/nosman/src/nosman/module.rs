use serde::{Deserialize, Serialize};
use std::{fmt, fs, path};
use std::path::PathBuf;
use std::time::Duration;
use colored::Colorize;
use indicatif::{ProgressBar};
use crate::nosman::index::{ModuleType};
use crate::nosman::path::{get_plugin_manifest_file, get_subsystem_manifest_file};

#[derive(Serialize, Deserialize, Debug, Eq, PartialEq, Hash, Clone)]
pub struct ModuleIdentifier {
    pub name: String,
    pub version: String,
}

#[derive(Serialize, Deserialize, Debug, Eq, PartialEq, Hash, Clone)]
pub struct ModuleInfo {
    pub id: ModuleIdentifier,
    pub display_name: Option<String>,
    pub description: Option<String>,
    pub dependencies: Option<Vec<ModuleIdentifier>>,
}

#[derive(Serialize, Deserialize, Debug, Eq, PartialEq, Hash, Clone)]
pub struct InstalledModule {
    pub info: ModuleInfo,
    pub config_path: path::PathBuf,
    pub public_include_folder: Option<path::PathBuf>,
    pub type_schema_files: Vec<path::PathBuf>,
    pub module_type: ModuleType,
}

impl InstalledModule {
    pub fn new(path: path::PathBuf) -> InstalledModule {
        InstalledModule {
            info: ModuleInfo {
                id: ModuleIdentifier {
                    name: String::new(),
                    version: String::new(),
                },
                display_name: None,
                description: None,
                dependencies: None,
            },
            config_path: path,
            public_include_folder: None,
            type_schema_files: Vec::new(),
            module_type: ModuleType::Plugin,
        }
    }
    pub fn get_module_dir(&self) -> PathBuf {
        self.config_path.parent().unwrap().to_path_buf()
    }
}

impl fmt::Display for ModuleIdentifier {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}-{}", self.name, self.version)
    }
}

pub fn get_module_manifest_file_in_folder(folder: &path::PathBuf) -> Result<Option<(ModuleType, PathBuf)>, String> {
    let res = get_plugin_manifest_file(folder);
    if res.is_err() {
        return Err(res.err().unwrap());
    }
    let plugin_manifest_file = res.unwrap();
    let res = get_subsystem_manifest_file(folder);
    if res.is_err() {
        return Err(res.err().unwrap());
    }
    let subsystem_manifest_file = res.unwrap();
    if plugin_manifest_file.is_some() && subsystem_manifest_file.is_some() {
        return Err(format!("Multiple module manifest files found in {}", folder.display()));
    }
    if plugin_manifest_file.is_none() && subsystem_manifest_file.is_none() {
        return Ok(None);
    }
    if plugin_manifest_file.is_some() {
        return Ok(Some((ModuleType::Plugin, plugin_manifest_file.unwrap())));
    }
    Ok(Some((ModuleType::Subsystem, subsystem_manifest_file.unwrap())))
}

pub fn get_module_manifests(folder: &PathBuf) -> Vec<(ModuleType, PathBuf)> {
    let pb = ProgressBar::new_spinner();
    pb.enable_steady_tick(Duration::from_millis(100));

    pb.println(format!("Looking for Nodos modules in {:?}", folder).to_string());
    let res = get_module_manifest_file_in_folder(&folder);
    if res.is_ok() {
        if let Some((ty, mpath)) = res.unwrap() {
            return vec![(ty, mpath)];
        }
    }

    let mut module_manifest_files = vec![];
    let mut stack = vec![folder.clone()];
    while let Some(current) = stack.pop() {
        let entries = match fs::read_dir(&current) {
            Ok(entries) => entries,
            Err(e) => {
                pb.println(format!("Error reading directory {:?}: {}", current, e.to_string().red()));
                continue;
            }
        };
        for entry in entries {
            let entry = match entry {
                Ok(entry) => entry,
                Err(ref e) => {
                    pb.println(format!("Error reading entry {:?}: {}",  &entry, e.to_string().red()));
                    continue;
                }
            };
            let path = entry.path();
            if path.is_dir() {
                let res = get_module_manifest_file_in_folder(&path);
                if res.is_err() {
                    pb.println(format!("{}", res.err().unwrap().yellow()));
                    continue;
                }
                if let Some((ty, mpath)) = res.unwrap() {
                    pb.set_message(format!("Found module manifest file: {:?}", mpath.file_name().unwrap()).to_string());
                    module_manifest_files.push((ty, mpath));
                }
                else {
                    pb.set_message(format!("Looking for Nodos modules in {:?}", path).to_string());
                    stack.push(path);
                }
            }
        }
    }
    module_manifest_files
}
