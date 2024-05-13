use std::collections::HashMap;
use std::time::Duration;
use colored::Colorize;
use indicatif::{ProgressBar, ProgressStyle};
use serde::{Deserialize, Serialize};
use crate::nosman::workspace::Workspace;

#[derive(Serialize, Deserialize, Debug)]
pub struct ModuleIndexEntry {
    pub(crate) name: String,
    url: String,
    vendor: String,
}

#[derive(Serialize, Deserialize, Debug, Eq, PartialEq, Hash, Clone)]
pub enum ModuleType {
    Plugin,
    Subsystem,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct SemVer {
    #[serde(alias = "major", alias = "MAJOR", alias = "Major")]
    major: u32,
    #[serde(alias = "minor", alias = "MINOR", alias = "Minor")]
    minor: u32,
    #[serde(alias = "patch", alias = "PATCH", alias = "Patch")]
    patch: u32,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ModuleReleaseEntry {
    version: String,
    pub(crate) url: String,
    plugin_api_version: Option<SemVer>,
    subsystem_api_version: Option<SemVer>
    // TODO: Replace with these
    // module_type: String,
    // api_version: SemVer,
    // release_date: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ModuleReleases {
    pub(crate) name: String,
    pub(crate) releases: Vec<ModuleReleaseEntry>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Remote {
    pub name: String,
    pub url: String,
}

impl Remote {
    pub fn new(name: &str, url: &str) -> Remote {
        Remote {
            name: name.to_string(),
            url: url.to_string(),
        }
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Index {
    pub modules: HashMap<String, HashMap<String, ModuleReleaseEntry>>, // name -> version -> ModuleReleaseEntry
}

impl Index {
    pub fn fetch(workspace: &Workspace) -> Index {
        println!("Fetching module index...");
        let pb = ProgressBar::new(0);
        pb.set_style(ProgressStyle::default_spinner()
            .template("{spinner} {wide_msg}").unwrap());
        pb.enable_steady_tick(Duration::from_millis(100));
        let mut index = Index {
            modules: HashMap::new(),
        };
        for remote in &workspace.remotes {
            // Fetch json file
            let res = reqwest::blocking::get(&remote.url);
            if let Err(e) = res {
                pb.println(format!("Failed to fetch remote: {}", e));
                continue;
            }
            let res = res.unwrap().json();
            if let Err(e) = res {
                pb.println(format!("Failed to parse remote: {}", e));
                continue;
            }
            let module_list: Vec<ModuleIndexEntry> = res.unwrap();
            pb.println(format!("Fetched {} modules from remote {}", module_list.len(), remote.name));
            // For each module in list
            for module in module_list {
                let res = reqwest::blocking::get(&module.url);
                if let Err(e) = res {
                    pb.println(format!("Failed to fetch module releases: {}", e));
                    continue;
                }
                let res = res.unwrap().json();
                if let Err(e) = res {
                    pb.println(format!("Failed to parse module releases: {}", e));
                    continue;
                }
                let versions: ModuleReleases = res.unwrap();
                pb.set_message(format!("Remote {}: Found {} releases for module {}", remote.name, versions.releases.len(), versions.name));
                // For each version in list
                for release in versions.releases {
                    index.add_module(&versions.name, release);
                }
            }
        }
        index
    }
    pub fn add_module(&mut self, name: &String, module: ModuleReleaseEntry) {
        let module_map = self.modules.entry(name.clone()).or_insert(HashMap::new());
        module_map.insert(module.version.clone(), module);
    }
    pub fn get_module(&self, name: &str, version: &str) -> Option<&ModuleReleaseEntry> {
        self.modules.get(name).and_then(|m| m.get(version))
    }
}