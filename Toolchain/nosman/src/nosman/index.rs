use std::collections::HashMap;
use std::{fs, io};
use std::process::Output;
use std::time::Duration;
use indicatif::{ProgressBar, ProgressStyle};
use serde::{Deserialize, Serialize};
use crate::nosman::constants;
use crate::nosman::workspace::Workspace;

#[derive(Serialize, Deserialize, Debug, Eq, PartialEq, Hash, Clone)]
pub enum PackageType {
    #[serde(alias = "plugin", alias = "PLUGIN")]
    Plugin,
    #[serde(alias = "subsystem", alias = "SUBSYSTEM")]
    Subsystem,
    #[serde(alias = "nodos", alias = "NODOS")]
    Nodos,
    #[serde(alias = "engine", alias = "ENGINE")]
    Engine,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct PackageIndexEntry {
    pub(crate) name: String,
    #[serde(rename = "url")]
    releases_url: String,
    vendor: String,
    #[serde(rename = "type")]
    package_type: PackageType,
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
    #[serde(alias = "minor", alias = "MINOR", alias = "Minor", skip_serializing_if = "Option::is_none")]
    pub minor: Option<u32>,
    #[serde(alias = "patch", alias = "PATCH", alias = "Patch", skip_serializing_if = "Option::is_none")]
    pub patch: Option<u32>,
    #[serde(alias = "build", alias = "BUILD", alias = "Build", skip_serializing_if = "Option::is_none")]
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
pub struct PackageReleaseEntry {
    pub(crate) version: String,
    pub(crate) url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) plugin_api_version: Option<SemVer>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) subsystem_api_version: Option<SemVer>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) release_date: Option<String>,
    // TODO: Replace with these
    // module_type: String,
    // api_version: SemVer,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct PackageReleases {
    pub(crate) name: String,
    pub(crate) releases: Vec<PackageReleaseEntry>,
}

pub fn run_if_not(dry_run: &bool, cmd: &mut std::process::Command) -> Option<Result<Output, io::Error>> {
    if *dry_run {
        println!("Would run: {:?}", cmd);
        None
    } else {
        Some(cmd.output())
    }
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
    pub fn fetch(&self, workspace: &Workspace) -> Result<Vec<PackageIndexEntry>, String> {
        let repo_dir = workspace.get_remote_repo_dir(&self);
        if !repo_dir.parent().unwrap().exists() {
            fs::create_dir_all(repo_dir.parent().unwrap()).unwrap();
        }
        if !repo_dir.exists() {
            let output = std::process::Command::new("git")
                .arg("clone")
                .arg(&self.url)
                .arg(&repo_dir)
                .output();
            if output.is_err() {
                return Err(format!("Failed to clone the remote repository: {}", output.err().unwrap().to_string()));
            }
        } else {
            let output = std::process::Command::new("git")
                .current_dir(&repo_dir)
                .arg("clean")
                .arg("-ffdx")
                .output();
            if output.is_err() {
                return Err(format!("Failed to clean the remote repository: {}", output.err().unwrap().to_string()));
            }
            let output = std::process::Command::new("git")
                .current_dir(&repo_dir)
                .arg("reset")
                .arg("--hard")
                .output();
            if output.is_err() {
                return Err(format!("Failed to clean the remote repository: {}", output.err().unwrap().to_string()));
            }
            let output = std::process::Command::new("git")
                .current_dir(&repo_dir)
                .arg("pull")
                .arg("--force")
                .output();
            if output.is_err() {
                return Err(format!("Failed to pull the remote repository: {}", output.err().unwrap().to_string()));
            }
        }
        let res = fs::read_to_string(repo_dir.join(constants::PACKAGE_INDEX_ROOT_FILE));
        if let Err(e) = res {
            return Err(format!("Failed to read remote package index: {}", e));
        }
        let res = serde_json::from_str(&res.unwrap());
        if let Err(e) = res {
            return Err(format!("Failed to parse remote package index: {}", e.to_string()));
        }
        let package_list: Vec<PackageIndexEntry> = res.unwrap();
        Ok(package_list)
    }
    pub fn get_default_branch_name(&self, workspace: &Workspace) -> String {
        let repo_dir = workspace.get_remote_repo_dir(&self);
        let output = std::process::Command::new("git")
            .current_dir(&repo_dir)
            .arg("symbolic-ref")
            .arg("HEAD")
            .arg("--short")
            .output();
        let branch_name = String::from_utf8(output.unwrap().stdout).unwrap();
        branch_name.trim().to_string()
    }
    pub fn fetch_add(&self, dry_run: &bool, workspace: &Workspace, name: &String, vendor: Option<&String>, package_type: &PackageType, release: PackageReleaseEntry) -> Result<(), String> {
        let repo_dir = workspace.get_remote_repo_dir(&self);
        let mut package_list: Vec<PackageIndexEntry> = self.fetch(workspace)?;
        // If package does not exist, add it
        let mut found = false;
        for package in &package_list {
            if package.name == *name {
                found = true;
                break;
            }
        }
        if !found {
            if vendor.is_none() {
                return Err("Vendor name was not provided!".to_string());
            }

            println!("Adding package {} to remote {}", name, self.name);

            // TODO: GitHub specific index code should be removed once we have a proper release server
            // Get organization name and repo name from the URL.
            let url_parts: Vec<&str> = self.url.split('/').collect();
            if url_parts.len() < 2 {
                return Err("Invalid URL".to_string());
            }
            let org_name = url_parts[url_parts.len() - 2];
            let repo_name = url_parts[url_parts.len() - 1];
            let branch_name = self.get_default_branch_name(workspace);

            let package = PackageIndexEntry {
                name: name.clone(),
                releases_url: format!("https://raw.githubusercontent.net/{}/{}/{}/releases/{}.json", org_name, repo_name, branch_name, name),
                vendor: vendor.unwrap().clone(),
                package_type: package_type.clone(),
            };
            package_list.push(package);

            let root_file = repo_dir.join(constants::PACKAGE_INDEX_ROOT_FILE);
            let res = fs::write(root_file, serde_json::to_string_pretty(&package_list).unwrap());
            if let Err(e) = res {
                return Err(format!("Failed to write remote package index: {}", e));
            }
        }

        let release_list_file = repo_dir.join("releases").join(format!("{}.json", name));
        if !release_list_file.parent().unwrap().exists() {
            fs::create_dir_all(release_list_file.parent().unwrap()).unwrap();
        }
        let mut release_list = PackageReleases{ name : name.clone(), releases: vec![] };
        if release_list_file.exists() {
            release_list = serde_json::from_str(&fs::read_to_string(&release_list_file).unwrap()).unwrap();
        }
        let version = release.version.clone();
        release_list.releases.insert(0, release);
        let res = fs::write(release_list_file, serde_json::to_string_pretty(&release_list).unwrap());
        if let Err(e) = res {
            return Err(format!("Failed to write remote package releases: {}", e));
        }

        // Commit and push
        let res = run_if_not(dry_run, std::process::Command::new("git")
            .current_dir(&repo_dir)
            .arg("add")
            .arg("."));
        if res.is_some() {
            let output = res.unwrap();
            if output.is_err() {
                return Err(format!("Failed to add files to the remote repository: {}", output.err().unwrap().to_string()));
            }
        }

        let res = run_if_not(dry_run, std::process::Command::new("git")
            .current_dir(&repo_dir)
            .arg("commit")
            .arg("-m")
            .arg(format!("Add package {} version {}", name, version)));
        if res.is_some() {
            let output = res.unwrap();
            if output.is_err() {
                return Err(format!("Failed to commit to the remote repository: {}", output.err().unwrap().to_string()));
            }
        }
        // If push fails, pull with rebase first and then push
        let mut res = run_if_not(dry_run, std::process::Command::new("git")
            .current_dir(&repo_dir)
            .arg("push"));
        if res.is_some() {
            let mut output = res.unwrap();
            let mut tries = 3;
            while output.is_err() && tries > 0 {
                println!("Failed to push to the remote repository: {}. Trying again.", output.err().unwrap().to_string());
                output = std::process::Command::new("git")
                    .current_dir(&repo_dir)
                    .arg("pull")
                    .arg("--rebase")
                    .output();
                if output.is_err() {
                    return Err(format!("Failed to pull with rebase from the remote repository: {}. If there were conflicts, manually solve them under {}.", output.err().unwrap().to_string(), repo_dir.display()));
                }
                output = std::process::Command::new("git")
                    .current_dir(&repo_dir)
                    .arg("push")
                    .output();
                tries -= 1;
            }
            if output.is_err() {
                return Err(format!("Failed to publish: {}", output.err().unwrap().to_string()));
            }
        }
        println!("Published package {} version {} to remote {}", name, version, self.name);
        return Ok(());
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Index {
    pub packages: HashMap<String, (PackageType, HashMap<String, PackageReleaseEntry>)>, // name -> version -> ModuleReleaseEntry
}

impl Index {
    pub fn fetch(workspace: &Workspace) -> Index {
        println!("Fetching package index...");
        let pb = ProgressBar::new(0);
        pb.set_style(ProgressStyle::default_spinner()
            .template("{spinner} {wide_msg}").unwrap());
        pb.enable_steady_tick(Duration::from_millis(100));
        let mut index = Index {
            packages: HashMap::new(),
        };
        for remote in &workspace.remotes {
            pb.set_message(format!("Fetching remote {}", remote.name));
            let res = remote.fetch(&workspace);
            if let Err(e) = res {
                pb.println(format!("Failed to fetch remote: {}", e));
                continue;
            }
            let package_list: Vec<PackageIndexEntry> = res.unwrap();
            pb.println(format!("Fetched {} packages from remote {}", package_list.len(), remote.name));
            // For each module in list
            for package in package_list {
                let res = reqwest::blocking::get(&package.releases_url);
                if let Err(e) = res {
                    pb.println(format!("Failed to fetch package releases: {}", e));
                    continue;
                }
                let res = res.unwrap().json();
                if let Err(e) = res {
                    pb.println(format!("Failed to parse package releases: {}", e));
                    continue;
                }
                let versions: PackageReleases = res.unwrap();
                pb.set_message(format!("Remote {}: Found {} releases for package {}", remote.name, versions.releases.len(), versions.name));
                // For each version in list
                for release in versions.releases {
                    index.add_package(&versions.name, package.package_type.clone(), release);
                }
            }
        }
        index
    }
    pub fn add_package(&mut self, name: &String, package_type: PackageType, package: PackageReleaseEntry) {
        let type_versions = self.packages.entry(name.clone()).or_insert((package_type, HashMap::new()));
        type_versions.1.insert(package.version.clone(), package);
    }
    pub fn get_module(&self, name: &str, version: &str) -> Option<&PackageReleaseEntry> {
        self.packages.get(name).and_then(|m| {
            if m.0 == PackageType::Plugin || m.0 == PackageType::Subsystem {
                m.1.get(version)
            } else {
                None
            }
        })
    }
    pub fn get_latest_compatible_release_within_range(&self, name: &str, version_start: &SemVer, version_end: &SemVer) -> Option<&PackageReleaseEntry> {
        let version_list = self.packages.get(name);
        if version_list.is_none() {
            return None;
        }
        let version_list = &version_list.unwrap().1;
        let mut versions: Vec<(&String, &PackageReleaseEntry)> = version_list.iter().collect();
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