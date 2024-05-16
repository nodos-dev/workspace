use std::collections::HashMap;
use std::time::Duration;
use colored::Colorize;
use indicatif::{ProgressBar, ProgressStyle};
use serde::{Deserialize, Serialize};
use crate::nosman::command::CommandError;
use crate::nosman::module::InstalledModule;
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

#[derive(Serialize, Deserialize, Debug, Eq, PartialEq, Hash, Clone)]
pub struct SemVer {
    #[serde(alias = "major", alias = "MAJOR", alias = "Major")]
    pub major: u32,
    #[serde(alias = "minor", alias = "MINOR", alias = "Minor")]
    pub minor: Option<u32>,
    #[serde(alias = "patch", alias = "PATCH", alias = "Patch")]
    pub patch: Option<u32>,
    #[serde(alias = "build", alias = "BUILD", alias = "Build")]
    pub build_number: Option<u32>,
}

impl SemVer {
    pub fn parse_from_string(s: &str) -> Option<SemVer> {
        // Parse 1.2.3.4 -> (1, 2, 3, Some(4))
        // Parse 1.2.3 -> (1, 2, 3, None)
        // Parse 1.2 -> (1, 2, 0, None)
        // Parse 1 -> (1, 0, 0, None)
        let parts: Vec<&str> = s.split('.').collect();
        let major = parts.get(0).and_then(|s| s.parse::<u32>().ok());
        let minor = parts.get(1).and_then(|s| s.parse::<u32>().ok());
        let patch = parts.get(2).and_then(|s| s.parse::<u32>().ok());
        let build_number = parts.get(3).and_then(|s| s.parse::<u32>().ok());
        if major.is_none() {
            return None;
        }
        let major = major.unwrap();
        Some(SemVer {
            major,
            minor,
            patch,
            build_number,
        })
    }
    pub fn to_string(&self) -> String {
        let mut s = self.major.to_string();
        if let Some(minor) = self.minor {
            s.push_str(&format!(".{}", minor));
        }
        if let Some(patch) = self.patch {
            s.push_str(&format!(".{}", patch));
        }
        if let Some(build_number) = self.build_number {
            s.push_str(&format!(".b{}", build_number));
        }
        s
    }
    pub fn is_compatible_with(&self, user: &SemVer) -> bool {
        if self.major != user.major {
            return false;
        }
        if self.minor < user.minor {
            return false;
        }
        return true;
    }
    pub fn upper_minor(&self) -> SemVer {
        SemVer {
            major: self.major,
            minor: self.minor.map(|m| m + 1),
            patch: None,
            build_number: None,
        }
    }
    pub fn upper_patch(&self) -> SemVer {
        SemVer {
            major: self.major,
            minor: self.minor,
            patch: self.patch.map(|p| p + 1),
            build_number: None,
        }
    }
    pub fn get_one_up(&self) -> SemVer {
        let version_start = self.clone();
        return if version_start.patch.is_none() {
            version_start.upper_minor()
        } else {
            version_start.upper_patch()
        }
    }
}

// Implement ordering for SemVer
impl PartialOrd for SemVer {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        if self.major < other.major {
            return Some(std::cmp::Ordering::Less);
        }
        if self.major > other.major {
            return Some(std::cmp::Ordering::Greater);
        }
        if self.minor < other.minor {
            return Some(std::cmp::Ordering::Less);
        }
        if self.minor > other.minor {
            return Some(std::cmp::Ordering::Greater);
        }
        if self.patch < other.patch {
            return Some(std::cmp::Ordering::Less);
        }
        if self.patch > other.patch {
            return Some(std::cmp::Ordering::Greater);
        }
        if let Some(build_number) = self.build_number {
            if let Some(other_build_number) = other.build_number {
                if build_number < other_build_number {
                    return Some(std::cmp::Ordering::Less);
                }
                if build_number > other_build_number {
                    return Some(std::cmp::Ordering::Greater);
                }
            }
        }
        Some(std::cmp::Ordering::Equal)
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ModuleReleaseEntry {
    pub(crate) version: String,
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

#[derive(Serialize, Deserialize, Debug, Clone)]
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
    pub fn get_latest_compatible_release_within_range(&self, name: &str, version_start: &SemVer, version_end: &SemVer) -> Option<&ModuleReleaseEntry> {
        let version_list = self.modules.get(name);
        if version_list.is_none() {
            return None;
        }
        let version_list = version_list.unwrap();
        let mut versions: Vec<(&String, &ModuleReleaseEntry)> = version_list.iter().collect();
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
}