use std::collections::HashMap;
use std::{fs};
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;
use indicatif::{ProgressBar};
use serde::{Deserialize, Serialize};
use rayon::prelude::*;
use crate::nosman::constants;
use crate::nosman::workspace::Workspace;
use crate::nosman::common::{run_if_not};
use crate::nosman::module::{PackageIdentifier};
use crate::nosman::platform::get_host_platform;

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
    #[serde(alias = "generic", alias = "GENERIC")]
    Generic,
}

impl PackageType {
    pub fn is_module(&self) -> bool {
        match self {
            PackageType::Plugin | PackageType::Subsystem => true,
            _ => false,
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct PackageIndexEntry {
    pub(crate) name: String,
    #[serde(rename = "url")]
    pub(crate) releases_url: String,
    vendor: String,
    #[serde(rename = "type")]
    pub(crate) package_type: PackageType,
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

impl std::cmp::Ord for SemVer {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.partial_cmp(other).unwrap()
    }
}

impl SemVer {
    pub fn parse_from_string(s: &str) -> Option<SemVer> {
        // Parse 1.2.3.b4 -> (1, 2, 3, Some(4))
        // Parse 1.2.3.4 -> (1, 2, 3, Some(4))
        // Parse 1.2.3 -> (1, 2, 3, None)
        // Parse 1.2 -> (1, 2, 0, None)
        // Parse 1 -> (1, 0, 0, None)
        let parts: Vec<&str> = s.split('.').collect();
        let opt_major = parts.get(0).and_then(|s| s.parse::<u32>().ok());
        let opt_minor = parts.get(1).and_then(|s| s.parse::<u32>().ok());
        let opt_patch = parts.get(2).and_then(|s| s.parse::<u32>().ok());
        let opt_build_number = parts.get(3).and_then(|s|
            if s.starts_with("b") {
                s.get(1..).and_then(|s| s.parse::<u32>().ok())
            } else {
                s.parse::<u32>().ok()
            });
        if opt_major.is_none() {
            return None;
        }
        let major = opt_major.unwrap();
        Some(SemVer {
            major,
            minor: opt_minor,
            patch: opt_patch,
            build_number: opt_build_number,
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
    pub fn upper_build(&self) -> SemVer {
        SemVer {
            major: self.major,
            minor: self.minor,
            patch: self.patch,
            build_number: self.build_number.map(|b| b + 1),
        }
    }
    pub fn get_one_up(&self) -> SemVer {
        let version_start = self.clone();
        if version_start.patch.is_none() {
            version_start.upper_minor()
        } else if version_start.build_number.is_none() {
            version_start.upper_patch()
        } else {
            version_start.upper_build()
        }
    }
	pub fn satisfies_requested_version(&self, requested: &SemVer) -> bool {
		if self.major != requested.major {
			return false;
		}
		return self.minor >= requested.minor;
	}
}

#[derive(Serialize, Deserialize, Debug)]
pub struct PackageReleaseEntry {
    pub(crate) version: String,
    pub(crate) url: String,
    // TODO: Replace plugin_api_version & subsystem_api_version with these
    // module_type: String,
    // api_version: SemVer,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) plugin_api_version: Option<SemVer>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) subsystem_api_version: Option<SemVer>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) release_date: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dependencies: Option<Vec<PackageIdentifier>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub module_tags: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub release_tags: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub platform: Option<String>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct PackageReleases {
    pub(crate) name: String,
    pub(crate) releases: Vec<PackageReleaseEntry>,
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
            if let Err(e) = output {
                return Err(format!("Failed to clone the remote repository: {}", e));
            }
        } else {
            let output = std::process::Command::new("git")
                .current_dir(&repo_dir)
                .arg("clean")
                .arg("-ffdx")
                .output();
            if let Err(e) = output {
                return Err(format!("Failed to clean the remote repository: {}", e));
            }
            let output = std::process::Command::new("git")
                .current_dir(&repo_dir)
                .arg("reset")
                .arg("--hard")
                .output();
            if let Err(e) = output {
                return Err(format!("Failed to clean the remote repository: {}", e));
            }
            let output = std::process::Command::new("git")
                .current_dir(&repo_dir)
                .arg("pull")
                .arg("--force")
                .output();
            if let Err(e) = output {
                return Err(format!("Failed to pull the remote repository: {}", e));
            }
        }
        let package_index_root_fp = repo_dir.join(constants::PACKAGE_INDEX_ROOT_FILE);
        if !package_index_root_fp.exists() {
            let res = fs::remove_dir_all(&repo_dir);
            if let Err(e) = res {
                return Err(format!("Unable to remove remote module index repo {}: {}", repo_dir.display(), e));
            }
            return self.fetch(workspace);
        }
        let res = fs::read_to_string(&package_index_root_fp);
        if let Err(e) = res {
            return Err(format!("Failed to read remote package index ({}): {}", package_index_root_fp.display(),  e));
        }
        let res = serde_json::from_str(&res.unwrap());
        if let Err(e) = res {
            return Err(format!("Failed to parse remote package index: {}", e.to_string()));
        }
        let package_list: Vec<PackageIndexEntry> = res.unwrap();
        Ok(package_list)
    }
    pub fn get_gh_remote_org_repo(&self) -> (String, String) {
        let url_parts: Vec<&str> = self.url.split('/').collect();
        if url_parts.len() < 2 {
            return ("".to_string(), "".to_string());
        }
        let org_name = url_parts[url_parts.len() - 2];
        let repo_name = url_parts[url_parts.len() - 1];
        (org_name.to_string(), repo_name.to_string())
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
    pub fn fetch_add(&self, dry_run: bool, verbose: bool, workspace: &Workspace, name: &String,
                     vendor: Option<&String>, package_type: &PackageType,
                     release: PackageReleaseEntry, publisher_name: Option<&String>,
                     publisher_email: Option<&String>) -> Result<String, String> {
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

            // TODO: GitHub specific index code should be removed once we have a proper release server
            // Get organization name and repo name from the URL.
            let (org_name, repo_name) = self.get_gh_remote_org_repo();
            let branch_name = self.get_default_branch_name(workspace);

            let package = PackageIndexEntry {
                name: name.clone(),
                releases_url: format!("https://raw.githubusercontent.com/{}/{}/{}/releases/{}.json", org_name, repo_name, branch_name, name),
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

        // Set author email and name
        if let Some(user_name) = publisher_name {
            let res = run_if_not(dry_run, verbose, std::process::Command::new("git")
                .current_dir(&repo_dir)
                .arg("config")
                .arg("user.name")
                .arg(user_name));
            if let Some(output) = res {
                if !output.status.success() {
                    return Err(format!("Failed to set user name: {}", String::from_utf8_lossy(&output.stderr)));
                }
            }
        }
        if let Some(user_email) = publisher_email {
            let res = run_if_not(dry_run, verbose, std::process::Command::new("git")
                .current_dir(&repo_dir)
                .arg("config")
                .arg("user.email")
                .arg(user_email));
            if let Some(output) = res {
                if !output.status.success() {
                    return Err(format!("Failed to set user email: {}", String::from_utf8_lossy(&output.stderr)));
                }
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
        let platform = release.platform.clone();
        // Check if target_platform exists for the same version in release_list
        for existing_release in &release_list.releases {
            if existing_release.platform.is_some() && release.platform.is_some() && existing_release.version == version {
                if existing_release.platform == release.platform {
                    return Err(format!("Release {}-{} for platform {} already exists!", name, version, &release.platform.unwrap()));
                }
            }
        }
        release_list.releases.insert(0, release);
        let res = fs::write(release_list_file, serde_json::to_string_pretty(&release_list).unwrap());
        if let Err(e) = res {
            return Err(format!("Failed to write remote package releases: {}", e));
        }
        self.update_remote(dry_run, verbose, format!("Add package {}-{} targeting {}", name, version, platform.unwrap_or("unknown".to_string())), &repo_dir)
    }
    pub fn remove_release(&self, dry_run: bool, verbose: bool, workspace: &Workspace, name: &String, version_opt: Option<&String>) -> Result<String, String> {
        let repo_dir = workspace.get_remote_repo_dir(&self);
        let release_list_file = repo_dir.join("releases").join(format!("{}.json", name));
        if !release_list_file.exists() {
            return Err(format!("No releases found for package {}", name));
        }
        let mut release_list: PackageReleases = serde_json::from_str(&fs::read_to_string(&release_list_file).unwrap()).unwrap();
        let commit_msg;
        if let Some(version) = version_opt {
            let mut found = false;
            for i in 0..release_list.releases.len() {
                if release_list.releases[i].version == *version {
                    release_list.releases.remove(i);
                    found = true;
                }
            }
            if !found {
                return Err(format!("No release found for package {} version {}", name, version));
            }
            commit_msg = format!("Remove package {} version {}", name, version);
            let res = fs::write(release_list_file, serde_json::to_string_pretty(&release_list).unwrap());
            if let Err(e) = res {
                return Err(format!("Failed to write remote package releases: {}", e));
            }
        } else {
            // Removing all releases
            commit_msg = format!("Remove all releases for package {}", name);
            if release_list.releases.len() == 0 {
                return Err(format!("No releases found for package {}", name));
            }
            let res = fs::remove_file(&release_list_file);
            if let Err(e) = res {
                return Err(format!("Failed to remove remote package releases: {}", e));
            }
            let index_file = repo_dir.join(constants::PACKAGE_INDEX_ROOT_FILE);
            let mut package_list: Vec<PackageIndexEntry> = serde_json::from_str(&fs::read_to_string(&index_file).unwrap()).unwrap();
            let mut found = false;
            for i in 0..package_list.len() {
                if package_list[i].name == *name {
                    package_list.remove(i);
                    found = true;
                    break;
                }
            }
            if !found {
                return Err(format!("No package found for package {}", name));
            }
            let res = fs::write(index_file, serde_json::to_string_pretty(&package_list).unwrap());
            if let Err(e) = res {
                return Err(format!("Failed to write remote package index: {}", e));
            }
        }
        self.update_remote(dry_run, verbose, commit_msg, &repo_dir)
    }
    fn update_remote(&self, dry_run: bool, verbose: bool, commit_msg: String, repo_dir: &PathBuf) -> Result<String, String> {
        // Commit and push
        let res = run_if_not(dry_run, verbose, std::process::Command::new("git")
            .current_dir(&repo_dir)
            .arg("add")
            .arg("."));
        if let Some(output) = res {
            if !output.status.success() {
                return Err(format!("Failed to add files to the remote repository: {}", String::from_utf8_lossy(&output.stderr)));
            }
        }

        let res = run_if_not(dry_run, verbose, std::process::Command::new("git")
            .current_dir(&repo_dir)
            .arg("commit")
            .arg("-m")
            .arg(commit_msg));
        if let Some(output) = res {
            if !output.status.success() {
                return Err(format!("Failed to commit to the remote repository: {}", String::from_utf8_lossy(&output.stderr)));
            }
        }

        // If push fails, pull with rebase first and then push
        let res = run_if_not(dry_run, verbose, std::process::Command::new("git")
            .current_dir(&repo_dir)
            .arg("push"));
        if res.is_some() {
            let mut output = res.unwrap();
            let mut tries = 3;
            while !output.status.success() && tries > 0 {
                output = run_if_not(false, verbose, std::process::Command::new("git")
                    .current_dir(&repo_dir)
                    .arg("pull")
                    .arg("--rebase")).unwrap();
                if !output.status.success() {
                    return Err(format!("Failed to pull with rebase from the remote {}. If there were conflicts, manually solve them under {}.", self.name, repo_dir.display()));
                }
                output = run_if_not(false, verbose,
                                    std::process::Command::new("git")
                                        .current_dir(&repo_dir)
                                        .arg("push")).unwrap();
                tries -= 1;
            }
            if !output.status.success() {
                return Err(format!("Failed to publish: {}", String::from_utf8_lossy(&output.stderr)));
            }
        }

        // Get commit SHA
        let res = run_if_not(dry_run, verbose, std::process::Command::new("git")
            .current_dir(&repo_dir)
            .arg("rev-parse")
            .arg("HEAD"));
        let mut commit_sha = "COMMIT_SHA_DRY_RUN".to_string();
        if let Some(output) = res {
            if !output.status.success() {
                return Err(format!("Failed to get commit SHA from the remote repository: {}", String::from_utf8_lossy(&output.stderr)));
            }
            commit_sha = String::from_utf8(output.stdout).unwrap().trim().to_string();
        }

        Ok(commit_sha)
    }
    pub fn create_gh_release(&self, dry_run: bool, verbose: bool, workspace: &Workspace, commit_sha: &String, name: &String, version: &String, target_platform: &String, tag: &String, artifacts: Vec<PathBuf>) -> Result<(), String> {
        let repo_dir = workspace.get_remote_repo_dir(&self);
        let (org_name, repo_name) = self.get_gh_remote_org_repo();

        let res = run_if_not(dry_run, verbose, std::process::Command::new("gh")
            .current_dir(&repo_dir)
            .arg("release")
            .arg("create")
            .arg(tag)
            .arg("--title")
            .arg(format!("{} {} ({})", name, version, target_platform))
            .arg("--repo")
            .arg(format!("{}/{}", org_name, repo_name))
            .arg("--target")
            .arg(commit_sha)
            .args(artifacts.iter().map(|p| p.to_str().unwrap())));
        if let Some(output) = res {
            if !output.status.success() {
                return Err(format!("Failed to create release: {}", String::from_utf8_lossy(&output.stderr)));
            }
        }
        Ok(())
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Index {
    pub packages: HashMap<String, (PackageType, Vec<PackageReleaseEntry>)>, // name -> version -> ModuleReleaseEntry
}

fn sort_version_list(versions: &mut Vec<&PackageReleaseEntry>) {
    versions.sort_by(|a, b| {
        let semver_a = SemVer::parse_from_string(&a.version);
        let semver_b = SemVer::parse_from_string(&b.version);
        if semver_a.is_none() || semver_b.is_none() {
            return std::cmp::Ordering::Equal;
        }
        let semver_a = semver_a.unwrap();
        let semver_b = semver_b.unwrap();
        semver_a.cmp(&semver_b)
    });
}

impl Index {
    pub fn fetch(workspace: &Workspace) -> Index {
        let pb = ProgressBar::new_spinner();
        pb.enable_steady_tick(Duration::from_millis(100));
        pb.println("Fetching package index");
        let index = Mutex::new(Index {
            packages: HashMap::new(),
        });
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

                for release in versions.releases {
                    let mut index = index.lock().unwrap();
                    index.add_package(&versions.name, package.package_type.clone(), release);
                }
            });
        });
        let index = index.into_inner().unwrap();
        pb.finish_and_clear();
        index
    }
    pub fn add_package(&mut self, name: &String, package_type: PackageType, package: PackageReleaseEntry) {
        let type_versions = self.packages.entry(name.clone()).or_insert((package_type, Vec::new()));
        type_versions.1.push(package);
    }
    pub fn get_package(&self, name: &str, version: &str) -> Option<(&PackageType, &PackageReleaseEntry)> {
        let res = self.packages.get(name);
        if res.is_none() {
            return None;
        }
        let (package_type, version_list) = res.unwrap();
        let platform = get_host_platform().to_string();
        for module in version_list {
            if module.version == version && (module.platform.is_none() || module.platform.as_ref().unwrap() == &platform) {
                return Some((package_type, module));
            }
        }
        None
    }
    pub fn get_latest_release(&self, name: &str) -> Option<(&PackageType, &PackageReleaseEntry)> {
        let res = self.packages.get(name);
        if res.is_none() {
            return None;
        }
        let (package_type, version_list) = res.unwrap();
        let mut versions: Vec<&PackageReleaseEntry> = version_list.iter().collect();
        sort_version_list(&mut versions);
        versions.reverse();
        if versions.len() == 0 {
            return None;
        }
        let platform = get_host_platform().to_string();
        for module in versions {
            if module.platform.is_none() || module.platform.as_ref().unwrap() == &platform {
                return Some((package_type, &module));
            }
        }
        None
    }
    pub fn get_latest_compatible_release_within_range(&self, name: &str, version_start: &SemVer, version_end: &SemVer) -> Option<(&PackageType, &PackageReleaseEntry)> {
        let res = self.packages.get(name);
        if res.is_none() {
            return None;
        }
        let (package_type, version_list) = res.unwrap();
        let mut versions: Vec<&PackageReleaseEntry> = version_list.iter().collect();
        sort_version_list(&mut versions);
        versions.reverse();
        let platform = get_host_platform().to_string();
        for module in versions {
            let semver = SemVer::parse_from_string(&module.version);
            if semver.is_none() {
                return None;
            }
            let semver = semver.unwrap();
            if semver >= *version_start && semver < *version_end && (module.platform.is_none() || module.platform.as_ref().unwrap() == &platform) {
                return Some((package_type, module));
            }
        }
        None
    }
}