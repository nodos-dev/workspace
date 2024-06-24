use std::collections::HashMap;
use std::{fs};
use std::path::PathBuf;
use std::time::Duration;
use git2::Repository;
use indicatif::{ProgressBar};
use serde::{Deserialize, Serialize};
use crate::nosman::constants;
use crate::nosman::workspace::Workspace;
use crate::nosman::common::{run_fn, run_if_not};

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
    pub fn fetch_repo(&self, workspace: &Workspace) -> Result<(), String> {
        let repo_dir = workspace.get_remote_repo_dir(&self);
        if !repo_dir.parent().unwrap().exists() {
            fs::create_dir_all(repo_dir.parent().unwrap()).unwrap();
        }
        if !repo_dir.exists() {
            let res = Repository::clone_recurse(&self.url, &repo_dir);
            if let Err(e) = res {
                return Err(format!("Failed to clone the remote repository: {}", e.to_string()));
            }
        } else {
            let repo = match Repository::open(&repo_dir) {
                Ok(repo) => repo,
                Err(e) => return Err(format!("Failed to open the remote repository: {}", e.to_string())),
            };
            let mut status_opts = git2::StatusOptions::new();
            let res = repo.statuses(Some(&mut status_opts));
            if let Err(e) = res {
                return Err(format!("Failed to get status of the remote repository: {}", e.to_string()));
            }
            let statuses = res.unwrap();
            for entry in statuses.iter() {
                if entry.status().contains(git2::Status::INDEX_NEW) || entry.status().contains(git2::Status::IGNORED) {
                    let res = fs::remove_file(repo_dir.join(entry.path().unwrap()));
                    if let Err(e) = res {
                        return Err(format!("Failed to remove new file {}: {}", entry.path().unwrap(), e));
                    }
                }
            }
            // Reset
            let mut checkout_builder = git2::build::CheckoutBuilder::new();
            checkout_builder.force();
            let res = repo.checkout_head(Some(&mut checkout_builder));
            if let Err(e) = res {
                return Err(format!("Failed to reset the remote repository: {}", e.to_string()));
            }
            // Pull
            let res = repo.find_remote("origin");
            if let Err(e) = res {
                return Err(format!("Failed to find remote origin: {}", e.to_string()));
            }
            let mut remote: git2::Remote = res.unwrap();
            let refspec= format!("refs/remotes/origin/{}", self.get_default_branch_name(workspace));
            let res = remote.fetch(&[refspec], None, None);
            if let Err(e) = res {
                return Err(format!("Failed to fetch the remote repository: {}", e.to_string()));
            }
            // Merge
            let annotated_commit = repo.reference_to_annotated_commit(&repo.find_reference("refs/remotes/origin/HEAD").unwrap());
            if let Err(e) = annotated_commit {
                return Err(format!("Failed to get annotated commit: {}", e.to_string()));
            }
            let res = repo.merge_analysis(&[&annotated_commit.unwrap()]);
            if let Err(e) = res {
                return Err(format!("Failed to analyze the remote repository: {}", e.to_string()));
            }
            let analysis = res.unwrap();
            if analysis.0.is_fast_forward() {
                let annotated_commit = repo.reference_to_annotated_commit(&repo.head().unwrap());
                if let Err(e) = annotated_commit {
                    return Err(format!("Failed to get annotated commit: {}", e.to_string()));
                }
                let res = repo.merge(&[], None, None);
                if let Err(e) = res {
                    return Err(format!("Failed to merge the remote repository: {}", e.to_string()));
                }
                let res = repo.cleanup_state();
                if let Err(e) = res {
                    return Err(format!("Failed to cleanup the remote repository: {}", e.to_string()));
                }
            }
        }
        Ok(())
    }
    pub fn fetch(&self, workspace: &Workspace) -> Result<Vec<PackageIndexEntry>, String> {
        let repo_dir = workspace.get_remote_repo_dir(&self);
        let res = self.fetch_repo(workspace);
        if let Err(e) = res {
            return Err(format!("Failed to fetch remote module index repo {}: {}", repo_dir.display(), e));
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
        let repo = Repository::open(&repo_dir).expect(format!("Failed to open the remote repository {}", repo_dir.display()).as_str());
        let head = repo.head().expect("Failed to get HEAD");
        let branch_name = head.name().expect("Failed to get branch name");
        branch_name.to_string()
    }
    pub fn fetch_add(&self, dry_run: bool, verbose: bool, workspace: &Workspace, name: &String,
                     vendor: Option<&String>, package_type: &PackageType,
                     release: PackageReleaseEntry, publisher_name: &String,
                     publisher_email: &String) -> Result<String, String> {
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

        let res = self.fetch_repo(workspace);
        if let Err(e) = res {
            return Err(format!("Failed to fetch remote module index repo {}: {}", repo_dir.display(), e));
        }

        // Set author email and name
        let repo = Repository::open(&repo_dir).expect(format!("Failed to open the remote repository {}", repo_dir.display()).as_str());
        repo.config().expect("Failed to get config").set_str("user.name", &publisher_name).unwrap();
        repo.config().expect("Failed to get config").set_str("user.email", &publisher_email).unwrap();

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

        // Add files
        let res = run_fn("Add files", dry_run, verbose, || {
            let mut index = repo.index().expect("Failed to get index");
            index.add_all(["*"].iter(), git2::IndexAddOption::DEFAULT, None).expect("Failed to add files to index");
            index.write().expect("Failed to write index");
            Ok(())
        });
        if let Err(msg) = res {
            return Err(format!("Failed to add files to the remote repository: {}", msg));
        }
        // Commit
        let mut commit_id= "COMMIT_SHA_DRY_RUN".to_string();
        let res = run_fn("Commit", dry_run, verbose, || {
           let mut index = repo.index().expect("Failed to get index");
            let tree_id = index.write_tree().expect("Failed to write tree");
            let tree = repo.find_tree(tree_id).expect("Failed to find tree");
            let head = repo.head().expect("Failed to get HEAD");
            let parent = repo.find_commit(head.target().expect("Failed to get HEAD target")).expect("Failed to find commit");
            let sig = repo.signature().expect("Failed to get signature");
            commit_id = repo.commit(Some("HEAD"), &sig, &sig, &format!("Add package {} version {}", name, version), &tree, &[&parent]).expect("Failed to commit").to_string();
            Ok(())
        });
        if let Err(msg) = res {
            return Err(format!("Failed to commit to the remote repository: {}", msg));
        }

        // Push with HTTPS
        let default_branch_name = self.get_default_branch_name(workspace);
        let res = run_fn("Push", dry_run, verbose, || {
            let mut origin = repo.find_remote("origin").expect("Failed to find remote origin");
            let mut callbacks = git2::RemoteCallbacks::new();
            callbacks.credentials(|url, _username_from_url, _allowed_types| {
                git2::Cred::credential_helper(&repo.config().expect("Failed to get git config"), url, Some(publisher_name))
            });
            let mut opts = git2::PushOptions::new();
            opts.remote_callbacks(callbacks);
            origin.push(&[format!("refs/heads/{}:refs/heads/{}", default_branch_name, default_branch_name).as_str()], Some(&mut opts)).expect("Failed to push");
            Ok(())
        });
        if let Err(msg) = res {
            return Err(format!("Failed to push to the remote repository: {}", msg));
        }
       Ok(commit_id)
    }
    pub fn create_gh_release(&self, dry_run: bool, verbose: bool, workspace: &Workspace, commit_sha: &String, name: &String, version: &String, artifacts: Vec<PathBuf>) -> Result<(), String> {
        let repo_dir = workspace.get_remote_repo_dir(&self);
        let (org_name, repo_name) = self.get_gh_remote_org_repo();

        let res = run_if_not(dry_run, verbose, std::process::Command::new("gh")
            .current_dir(&repo_dir)
            .arg("release")
            .arg("create")
            .arg(format!("{}-{}", name, version))
            .arg("--title")
            .arg(format!("{} {}", name, version))
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
    pub packages: HashMap<String, (PackageType, HashMap<String, PackageReleaseEntry>)>, // name -> version -> ModuleReleaseEntry
}

impl Index {
    pub fn fetch(workspace: &Workspace) -> Index {
        let pb = ProgressBar::new_spinner();
        pb.enable_steady_tick(Duration::from_millis(100));
        pb.println("Fetching package index");
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
                    pb.println(format!("Failed to fetch package releases for {}: {}", package.name, e));
                    continue;
                }
                let res = res.unwrap().json();
                if let Err(e) = res {
                    pb.println(format!("Failed to parse package releases for {}: {}", package.name, e));
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
    pub fn get_latest_release(&self, name: &str) -> Option<(&PackageType, &PackageReleaseEntry)> {
        let res = self.packages.get(name);
        if res.is_none() {
            return None;
        }
        let (package_type, version_list) = res.unwrap();
        let mut versions: Vec<(&String, &PackageReleaseEntry)> = version_list.iter().collect();
        versions.sort_by(|a, b| a.0.cmp(b.0));
        versions.reverse();
        if versions.len() == 0 {
            return None;
        }
        Some((package_type, versions[0].1))
    }
    pub fn get_latest_compatible_release_within_range(&self, name: &str, version_start: &SemVer, version_end: &SemVer) -> Option<(&PackageType, &PackageReleaseEntry)> {
        let res = self.packages.get(name);
        if res.is_none() {
            return None;
        }
        let (package_type, version_list) = res.unwrap();
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
                return Some((package_type, module));
            }
        }
        None
    }
}