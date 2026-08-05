use rayon::iter::ParallelIterator;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::cmp::PartialEq;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration};
use bitflags::bitflags;
use colored::Colorize;
use crate::nosman::common::get_progress_bar;
use inquire::Select;
use rayon::iter::IntoParallelRefIterator;
use serde::{Deserialize, Serialize};
use crate::nosman::command::{CommandError, CommandResult};

use crate::nosman::command::CommandError::InvalidArgument;
use crate::nosman::index::{Index, PackageReleaseEntry, PackageType, SemVer};
use crate::nosman::plugin::{NodeDefinition, PluginEntry};
use crate::nosman::package::{get_package_manifests, LocalPackageEntry};
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

#[derive(Debug, PartialEq)]
pub enum AutoRescanResult {
    /// No action was needed - workspace was up to date
    NoActionNeeded,
    /// Full workspace rescan was performed due to missing index file
    FullRescanMissingIndex,
    /// Full workspace rescan was performed due to missing manifest files
    FullRescanMissingManifests,
    /// Partial rescan was performed on specific folders with updated manifests
    PartialRescanUpdatedManifests(Vec<PathBuf>),
}

#[derive(Debug, Default)]
struct WorkspaceRuntimeParams {
    status: WorkspaceStatus,
    output_mode_stack: Vec<OutputMode>,
    store_client: Option<nodos_store_client::StoreClient>,
    authenticated_store_client: Option<nodos_store_client::StoreClient>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Workspace {
    #[serde(skip_serializing, skip_deserializing)]
    pub root: PathBuf,
    // name -> version -> all on-disk entries. Keeps every entry; precedence is applied at
    // lookup time, not here.
    #[serde(alias = "installed_modules", deserialize_with = "deserialize_packages")]
    pub packages: HashMap<String, HashMap<String, Vec<LocalPackageEntry>>>,
    #[serde(skip_serializing, skip_deserializing)]
    pub index_cache: Index,
    #[serde(skip_serializing, skip_deserializing)]
    runtime: WorkspaceRuntimeParams,
}

#[derive(Clone, Copy)]
pub struct RescanFlags(u8);

bitflags! {
    impl RescanFlags: u8 {
        const ScanPackages = 0b1;
        const FetchPackageIndex = 0b10;
    }
}

#[derive(Clone, Copy)]
pub struct ScanPackagesFlags(u8);

bitflags! {
    impl ScanPackagesFlags: u8 {
        const ForceReplaceInRegistry = 0b1;
        const RegisterCommands = 0b10;
    }
}

impl PartialEq<u8> for RescanFlags {
    fn eq(&self, other: &u8) -> bool {
        self.bits() == *other
    }
}

fn api_version_to_semver(v: &nodos_store_client::ApiVersion) -> SemVer {
    SemVer::new(v.major, v.minor, v.patch, None)
}

fn releases_to_index_entries(
    package_type: &PackageType,
    releases: Vec<nodos_store_client::Release>,
) -> Vec<PackageReleaseEntry> {
    let mut entries = Vec::new();
    for release in releases {
        for artifact in &release.artifacts {
            entries.push(PackageReleaseEntry {
                version: release.version.clone(),
                artifact_id: Some(artifact.id),
                url: String::new(),
                plugin_api_version: if *package_type == PackageType::Plugin {
                    release.api_version.as_ref().map(api_version_to_semver)
                } else {
                    None
                },
                subsystem_api_version: if *package_type == PackageType::Subsystem {
                    release.api_version.as_ref().map(api_version_to_semver)
                } else {
                    None
                },
                release_date: Some(release.updated_at.clone()),
                dependencies: if release.dependencies.is_empty() {
                    None
                } else {
                    Some(
                        release
                            .dependencies
                            .iter()
                            .map(|d| crate::nosman::package::PackageIdentifier {
                                name: d.name.clone(),
                                version: d.version.clone(),
                            })
                            .collect(),
                    )
                },
                category: None,
                module_tags: None,
                release_tags: if release.tags.is_empty() {
                    None
                } else {
                    Some(release.tags.clone())
                },
                platform: Some(artifact.target_platform.clone()),
                node_names: None,
            });
        }
    }
    entries
}

fn fetch_releases_mt(
    is_silent: bool,
    client: &nodos_store_client::StoreClient,
    package_names: Option<HashSet<String>>,
) -> HashMap<String, (PackageType, Vec<PackageReleaseEntry>)> {
    let releases = Mutex::new(HashMap::new());
    let pb = get_progress_bar(is_silent);
    pb.enable_steady_tick(Duration::from_millis(100));
    pb.set_message("Fetching Nodos Store index...");

    let package_list = match client.list_packages().map_err(|e| e.to_string()) {
        Ok(list) => list,
        Err(e) => {
            pb.println(format!("Failed to fetch package list: {}", e));
            return releases.into_inner().unwrap();
        }
    };

    pb.println(format!("Fetched {} packages from Nodos Store", package_list.len()));
    package_list.par_iter().for_each(|package| {
        if let Some(ref package_names) = package_names {
            if !package_names.contains(&package.name) {
                return;
            }
        }
        let package_type = PackageType::from_str(&package.package_type);
        let res = client.get_releases(&package.name).map_err(|e| e.to_string());
        if let Err(e) = res {
            pb.println(format!("Failed to fetch package releases for {}: {}", package.name, e));
            return;
        }
        let releases_for_package = releases_to_index_entries(&package_type, res.unwrap());
        pb.set_message(format!(
            "Found {} releases for package {}",
            releases_for_package.len(),
            package.name
        ));
        {
            let mut map = releases.lock().unwrap();
            let entry = map
                .entry(package.name.clone())
                .or_insert((package_type.clone(), Vec::new()));
            entry.1.extend(releases_for_package);
        }
    });
    releases.into_inner().unwrap()
}

impl Workspace {
    fn new_empty(path: PathBuf) -> Workspace {
        Workspace {
            root: path,
            packages: HashMap::new(),
            index_cache: Index { packages: HashMap::new() },
            runtime: WorkspaceRuntimeParams { status: WorkspaceStatus::DoesNotExist, output_mode_stack: vec![OutputMode::Default], store_client: None, authenticated_store_client: None },
        }
    }
    pub fn from_root(path: &PathBuf) -> Workspace {
        let index_filepath = get_nosman_index_filepath_for(path);
        let exists = index_filepath.exists();
        let mut workspace = Workspace::new_empty(std::path::absolute(path).unwrap_or_else(|e| panic!("Failed to get absolute path from {:?}: {}", path, e)));
        if !exists {
            workspace.runtime.status = WorkspaceStatus::DoesNotExist;
            return workspace;
        }
        let res = fs::File::open(&index_filepath);
        if let Ok(file) = res {
            let mut parsed_ws = match serde_json::from_reader(file) {
                Ok(workspace) => workspace,
                Err(e) => {
                    eprintln!("{}", format!("Failed to parse workspace file: {}. Rescanning...", e).red());
                    workspace.rescan(RescanFlags::all()).unwrap_or_else(|e| panic!("Failed to rescan workspace: {}", e));
                    workspace.save().unwrap_or_else(|e| panic!("Failed to save workspace: {}", e));
                    workspace
                }
            };
            parsed_ws.root = dunce::canonicalize(path).unwrap_or_else(|e| panic!("Failed to canonicalize path {:?}: {}", path, e));
            parsed_ws.runtime.status = WorkspaceStatus::Ready;
            workspace = parsed_ws;
        } else {
            eprintln!("{}", format!("Failed to open workspace file: {}", index_filepath.display()).red());
            workspace.runtime.status = WorkspaceStatus::FailedToOpen;
        }
        workspace
    }
    pub fn ready(&self) -> bool {
        self.runtime.status == WorkspaceStatus::Ready
    }
    pub fn save(&self) -> Result<(), std::io::Error>{
        fs::create_dir_all(get_nosman_dir_for(&self.root))?;
        let file = fs::File::create(self.get_nosman_index_filepath())?;
        serde_json::to_writer_pretty(file, self)?;
        Ok(())
    }
    pub fn get_nosman_index_filepath(&self) -> PathBuf {
        get_nosman_index_filepath_for(&self.root)
    }
    /// Of the entries for one name+version, the development package wins, else the first.
    fn preferred_entry<'a>(&self, entries: &'a [LocalPackageEntry]) -> Option<&'a LocalPackageEntry> {
        entries
            .iter()
            .find(|e| e.is_development_package(self))
            .or_else(|| entries.first())
    }
    /// The single entry to act on for one (non-empty) name+version. A hard error when more
    /// than one development package claims the version — there is no principled winner.
    fn single_entry<'a>(&self, entries: &'a [LocalPackageEntry]) -> Result<&'a LocalPackageEntry, CommandError> {
        let devs: Vec<&LocalPackageEntry> = entries.iter().filter(|e| e.is_development_package(self)).collect();
        if devs.len() > 1 {
            let paths = devs.iter().map(|e| e.get_package_root().display().to_string()).collect::<Vec<_>>().join(", ");
            return Err(CommandError::InvalidArgument { message: format!("Multiple development packages for {} at: {}. Bump or remove one.", devs[0].info.id, paths) });
        }
        devs.first().copied().or_else(|| entries.first())
            .ok_or_else(|| CommandError::Runtime { message: "package entry list is empty".to_string() })
    }
    /// True if any entry for `name` exactly matches `version`.
    pub fn is_installed(&self, name: &str, version: &str) -> bool {
        self.packages.get(name).and_then(|v| v.get(version)).is_some_and(|e| !e.is_empty())
    }
    /// True if any installed version of `name` matches the prefix.
    pub fn has_local_match(&self, name: &str, version_prefix: &SemVer) -> bool {
        self.packages.get(name).is_some_and(|versions| {
            versions.keys().any(|v| SemVer::parse_from_str(v).is_some_and(|s| s.matches_prefix(version_prefix)))
        })
    }
    /// True if any installed version of `name` matches the requested version prefix string.
    pub fn is_installed_matching(&self, name: &str, requested_version: &str) -> bool {
        SemVer::parse_from_str(requested_version).is_some_and(|p| self.has_local_match(name, &p))
    }
    pub fn get_package(&self, name: &str, version: &str) -> Result<&LocalPackageEntry, CommandError> {
        let entries = self.packages.get(name).and_then(|v| v.get(version)).filter(|e| !e.is_empty())
            .ok_or_else(|| CommandError::InvalidArgument { message: format!("Package {} version {} is not installed", name, version) })?;
        self.single_entry(entries)
    }

    pub fn normalize_to_workspace<P: AsRef<Path>>(
        &self, path: P,
    ) -> Option<PathBuf> {
        let path = path.as_ref();
        let workspace_dir = self.root.as_path();

        // Step 1: If it's absolute, make it relative to workspace
        let relative_path = if path.is_absolute() {
            match path.strip_prefix(workspace_dir) {
                Ok(rel) => rel.to_path_buf(),
                Err(_) => {
                    // Not inside workspace — try to make it relative anyway
                    pathdiff::diff_paths(path, workspace_dir)?
                }
            }
        } else {
            path.to_path_buf()
        };

        // Step 2: Normalize (remove redundant `.` and `..`)
        if let Ok(canon_ws) = workspace_dir.canonicalize() {
            if let Ok(canon_rel) = canon_ws.join(&relative_path).canonicalize() {
                // Step 3: Check that the final path is still inside the workspace
                if canon_rel.starts_with(&canon_ws) {
                    return Some(relative_path);
                }
            }
        }

        None
    }

    pub fn get_package_by_path(&self, path: PathBuf)-> Option<&LocalPackageEntry>{
        let normalized_path = self.normalize_to_workspace(path.clone()).unwrap();
        for version_map in self.packages.values() {
            for entry in version_map.values().flatten() {
                if entry.manifest_path == path || entry.manifest_path == normalized_path {
                    return Some(entry);
                }
            }
        }
        None
    }
    /// One entry per version for `name` (not every duplicate entry).
    pub fn get_packages(&self, name: &str) -> Result<Vec<&LocalPackageEntry>, CommandError> {
        let mut res = Vec::new();
        if let Some(versions) = self.packages.get(name) {
            for entries in versions.values() {
                res.push(self.single_entry(entries)?);
            }
        }
        Ok(res)
    }
    pub fn get_or_select_package(&self, package_name: &String) -> Result<LocalPackageEntry, CommandError> {
        let packages = self.get_packages(package_name)?;
        let package;
        if packages.is_empty() {
            return Err(InvalidArgument { message: format!("Package {} not found", package_name) });
        } else if packages.len() > 1 {
            let selection = Select::new(format!("Multiple packages found with name {}. Please select one:", package_name).as_str(), packages)
                .prompt();
            if let Err(e) = selection {
                return Err(InvalidArgument { message: format!("Failed to select module: {}", e) });
            } else {
                package = selection.unwrap();
            }
        } else {
            package = packages[0];
        }
        Ok(self.absolutize_paths(package))
    }
    pub fn absolutize_paths(&self, package: &LocalPackageEntry) -> LocalPackageEntry {
        let mut new_package = package.clone();
        if new_package.manifest_path.is_relative() {
            new_package.manifest_path = self.root.join(&new_package.manifest_path);
        }
        if let Some(ref path) = new_package.public_include_folder {
            if path.is_relative() {
                new_package.public_include_folder = Some(self.root.join(path));
            }
        }
        for path in &mut new_package.type_schema_files {
            if path.is_relative() {
                *path = self.root.join(&*path);
            }
        }
        new_package
    }
    pub fn get_latest_local_package_for_prefix(&self, name: &str, version_prefix: &SemVer) -> Result<&LocalPackageEntry, CommandError> {
        let not_found = || CommandError::InvalidArgument { message: format!("No installed version matching prefix '{}' for package {}", version_prefix, name) };
        let version_list = self.packages.get(name).ok_or_else(not_found)?;
        // Sort by parsed SemVer, not the raw string: lexicographic string order
        // ranks "2.0.0.b1179" above "2.0.0" (longer string, same prefix), which
        // would prefer a stale pre-release build over the real release.
        let mut versions: Vec<(SemVer, &Vec<LocalPackageEntry>)> = version_list
            .iter()
            .filter_map(|(version, entries)| Some((SemVer::parse_from_str(version)?, entries)))
            .collect();
        versions.sort_by(|a, b| b.0.cmp(&a.0));
        for (semver, entries) in versions {
            if semver.matches_prefix(version_prefix) {
                return self.single_entry(entries);
            }
        }
        Err(not_found())
    }
    pub fn get_latest_local_package_for_version(&self, module_name: &str, requested_version: &str) -> Result<&LocalPackageEntry, CommandError> {
        let Some(version_prefix) = SemVer::parse_from_str(requested_version) else {
            return Err(CommandError::InvalidArgument { message: format!("Invalid semantic version: {}.", requested_version) });
        };
        self.get_latest_local_package_for_prefix(module_name, &version_prefix)
    }
    pub fn get_latest_absent_release_for(&mut self, name: &str, requested_version: &str) -> Result<Option<(&PackageType, &PackageReleaseEntry)>, CommandError> {
        // If the version is not a valid semantic version, return Error
        let semver = SemVer::parse_from_str(requested_version);
        if semver.is_none() {
            return Err(InvalidArgument { message: format!("{} is not a valid semantic version", requested_version) });
        }
        let version_prefix = semver.unwrap();
        if self.has_local_match(name, &version_prefix) {
            // Already installed; surface an ambiguous-development-package error if present.
            self.get_latest_local_package_for_prefix(name, &version_prefix)?;
            return Ok(None);
        }
        let res = self.index_cache.get_latest_compatible_release(name, &version_prefix);
        if res.is_none() {
            return Err(InvalidArgument { message: format!("No releases found for package {} matching prefix '{}'", name, version_prefix) });
        }
        Ok(Some(res.unwrap()))
    }
    pub fn add(&mut self, package: LocalPackageEntry) {
        let versions = self.packages.entry(package.info.id.name.clone()).or_default();
        let entries = versions.entry(package.info.id.version.clone()).or_default();
        // Replace only the entry for the same path; keep entries at other paths.
        if let Some(existing) = entries.iter_mut().find(|e| e.manifest_path == package.manifest_path) {
            *existing = package;
        } else {
            entries.push(package);
        }
    }
    pub fn remove(&mut self, name: &str, version: &str) -> CommandResult {
        let entries = match self.packages.get(name).and_then(|v| v.get(version)) {
            Some(entries) if !entries.is_empty() => entries.clone(),
            _ => return Err(CommandError::InvalidArgument { message: format!("Package {} version {} is not installed", name, version) }),
        };
        println!("Removing package {} version {}", name, version);
        // Delete fetched entries; never delete a development checkout.
        let mut kept = Vec::new();
        let mut deleted = 0;
        for entry in entries {
            if entry.is_development_package(self) {
                println!("{}", format!("Keeping development package at {} (source not deleted)", entry.get_package_root().display()).yellow());
                kept.push(entry);
            } else {
                fs::remove_dir_all(entry.get_package_root())?;
                deleted += 1;
            }
        }
        if let Some(versions) = self.packages.get_mut(name) {
            if kept.is_empty() {
                versions.remove(version);
            } else {
                versions.insert(version.to_string(), kept);
            }
        }
        self.save()?;
        if deleted > 0 {
            println!("{}", format!("Module {} version {} removed successfully", name, version).as_str().green());
        } else {
            println!("{}", format!("Module {} version {} kept; only a development package is present", name, version).as_str().yellow());
        }
        Ok(())
    }
    pub fn remove_all(&mut self) -> CommandResult {
        for (_name, versions) in self.packages.iter() {
            for module in versions.values().flatten() {
                println!("Removing module {}", module.info.id);
                fs::remove_dir_all(module.get_package_root())?;
            }
        }
        self.packages.clear();
        self.save()?;
        println!("{}", "All modules removed successfully".green());
        Ok(())
    }
    pub fn is_silent(&self) -> bool {
        self.runtime.output_mode_stack.last().unwrap_or(&OutputMode::Default) == &OutputMode::Silent
    }
    pub fn store_client(&mut self) -> Result<&nodos_store_client::StoreClient, CommandError> {
        if self.runtime.store_client.is_none() {
            self.runtime.store_client = Some(
                nodos_store_client::StoreClient::builder()
                    .build()
                    .map_err(|e| CommandError::Runtime {
                        message: format!("Failed to build store client: {}", e),
                    })?,
            );
        }
        Ok(self.runtime.store_client.as_ref().unwrap())
    }

    pub fn authenticated_store_client_mut(&mut self) -> Result<&mut nodos_store_client::StoreClient, CommandError> {
        if self.runtime.authenticated_store_client.is_none() {
            self.runtime.authenticated_store_client = Some(
                nodos_store_client::StoreClient::builder()
                    .with_token_store(nodos_store_client::TokenStore::new(std::path::PathBuf::from("nosman")))
                    .with_auth(nodos_store_client::Auth::from_env().unwrap_or(nodos_store_client::Auth::None))
                    .build()
                    .map_err(|e| CommandError::Runtime {
                        message: format!("Failed to build authenticated store client: {}", e),
                    })?,
            );
        }
        Ok(self.runtime.authenticated_store_client.as_mut().unwrap())
    }

    /// The same client as [`Self::authenticated_store_client_mut`], handed out as a shared
    /// reference so several downloads can run at once.
    pub fn authenticated_store_client(&mut self) -> Result<&nodos_store_client::StoreClient, CommandError> {
        self.authenticated_store_client_mut().map(|client| &*client)
    }

    /// Downloads and extracts a store artifact into `target`.
    ///
    /// Uses the authenticated client so private artifacts the signed-in user is
    /// entitled to resolve to a download instead of a not-found response.
    pub fn download_and_extract_artifact(
        &mut self,
        artifact_id: i64,
        target: &std::path::Path,
    ) -> CommandResult {
        self.authenticated_store_client_mut()?
            .install_artifact(artifact_id, target)
            .map_err(|e| CommandError::Runtime {
                message: format!("Failed to install artifact: {}", e),
            })?;
        Ok(())
    }

    pub fn push_output_mode(&mut self, mode: OutputMode) {
        self.runtime.output_mode_stack.push(mode);
    }
    pub fn pop_output_mode(&mut self) {
        if self.runtime.output_mode_stack.len() > 1 {
            self.runtime.output_mode_stack.pop();
        }
    }
    pub fn with_output_mode_scoped<T, F>(&mut self, mode: OutputMode, f: F) -> T 
    where 
        F: FnOnce(&mut Self) -> T 
    {
        self.push_output_mode(mode);
        let result = f(self);
        self.pop_output_mode();
        result
    }
    pub fn scan_packages_in_folder(&mut self, folder: PathBuf, flags: ScanPackagesFlags) {
        // Scan folders with .noscfg and .nossys files
        let folder = dunce::canonicalize(&folder).unwrap_or_else(|e| panic!("Failed to canonicalize path {}: {}", folder.display(), e));
        let package_manifests = get_package_manifests(&folder, self.is_silent());

        let pb = get_progress_bar(self.is_silent());
        pb.enable_steady_tick(Duration::from_millis(100));

        pb.println(format!("Found {} packages in {}", package_manifests.len(), folder.display()).as_str().green().to_string());

        for (ty, path) in package_manifests {
            pb.set_message(format!("Scanning: {}", path.display()));
            let res = LocalPackageEntry::new(self, get_rel_path_based_on(&path, &self.root), ty, flags.contains(ScanPackagesFlags::RegisterCommands));
            if let Err(msg) = res {
                pb.println(format!("Error while scanning {}: {}", path.display(), msg).red().to_string());
                continue;
            }
            let package = res.unwrap();
            let other_paths: Vec<String> = self.packages
                .get(&package.info.id.name)
                .and_then(|v| v.get(&package.info.id.version))
                .map(|entries| entries.iter()
                    .filter(|e| e.manifest_path != package.manifest_path)
                    .map(|e| e.manifest_path.display().to_string())
                    .collect())
                .unwrap_or_default();
            if !other_paths.is_empty() {
                pb.println(format!("Multiple entries for {} found; keeping all: {} alongside {}", package.info.id, path.display(), other_paths.join(", ")));
            }
            self.add(package);
        }
    }
    pub fn scan_packages(&mut self, flags: ScanPackagesFlags) {
       self.scan_packages_in_folder(self.root.clone(), flags);
    }
    pub fn recreate(&mut self) -> Result<(), CommandError> {
        fs::create_dir_all(&self.root)?;
        self.rescan(RescanFlags::all())?;
        Ok(())
    }
    pub fn rescan(&mut self, flags: RescanFlags) -> CommandResult {
        if flags.contains(RescanFlags::FetchPackageIndex) {
            self.index_cache.packages.clear();
            self.fetch_releases(None);
        }
        if flags.contains(RescanFlags::ScanPackages) {
            self.packages.clear();
            self.scan_packages(ScanPackagesFlags::ForceReplaceInRegistry | ScanPackagesFlags::RegisterCommands);
        }
        self.save()?;
        self.runtime.status = WorkspaceStatus::Ready;
        Ok(())
    }
    /// The type the store has for a package, in one call and without pulling in all of its
    /// releases.
    pub fn fetch_package_type(&mut self, package_name: &str) -> Result<PackageType, CommandError> {
        let client = self.store_client()?;
        let package = client.get_package(package_name).map_err(|e| CommandError::Runtime {
            message: format!("Cannot read package {} from the Nodos Store: {}", package_name, e),
        })?;
        Ok(PackageType::from_str(&package.package_type))
    }
    pub fn fetch_package_releases(&mut self, package_name: &str) {
        let result = {
            let client = match self.store_client() {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("Warning: {}", e);
                    return;
                }
            };
            let package_type = client
                .get_package(package_name)
                .map(|pkg| PackageType::from_str(&pkg.package_type))
                .unwrap_or(PackageType::Generic);
            client
                .get_releases(package_name)
                .ok()
                .map(|releases| (package_type.clone(), releases_to_index_entries(&package_type, releases)))
        };
        if let Some((package_type, entries)) = result {
            for entry in entries {
                self.index_cache.add_package(package_name, package_type.clone(), entry);
            }
        }
    }
    pub fn fetch_releases(&mut self, package_names: Option<HashSet<String>>) {
        let is_silent = self.is_silent();
        let client = match self.store_client() {
            Ok(c) => c,
            Err(e) => {
                eprintln!("Warning: {}", e);
                return;
            }
        };
        let res = fetch_releases_mt(is_silent, client, package_names);
        for (name, (package_type, releases)) in res {
            for release in releases {
                self.index_cache.add_package(&name, package_type.clone(), release);
            }
        }
    }
    pub fn get_node_definitions(&self, node_class_name: &str) -> Vec<NodeDefinition> {
        let mut res = Vec::new();
        for versions in self.packages.values() {
            for package in versions.values().flatten() {
                let package_abs = self.absolutize_paths(package);
                if package_abs.package_type == PackageType::Plugin {
                    if let Ok(plugin) = PluginEntry::new(package_abs) {
                        if let Some(found) = plugin.get_node_definition(node_class_name) {
                            res.push(found);
                        }
                    }
                }
            }
        }
        res
    }
    pub fn get_latest_local_packages(&self) -> Vec<&LocalPackageEntry> {
        let mut versions_map = HashMap::new();
        for (package_name, versions) in &self.packages {
            for (version, entries) in versions {
                let module = match self.preferred_entry(entries) {
                    Some(m) => m,
                    None => continue,
                };
                if !versions_map.contains_key(package_name) {
                    versions_map.insert(package_name.clone(), module);
                } else {
                    let existing = versions_map.get(package_name).unwrap();
                    let existing_semver = SemVer::parse_from_str(existing.info.id.version.as_str());
                    let new_semver = SemVer::parse_from_str(version.as_str());
                    if existing_semver.is_none() || new_semver.is_none() {
                        continue;
                    }
                    let existing_semver = existing_semver.unwrap();
                    let new_semver = new_semver.unwrap();
                    if new_semver > existing_semver {
                        versions_map.insert(package_name.clone(), module);
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
    pub fn ensure_ready_if_required(&self, required: bool) -> Result<(), CommandError> {
        if required && !self.ready() {
            return Err(CommandError::Runtime {
                message: format!("Workspace required but not found in {}", self.root.display()),
            });
        }
        Ok(())
    }
    #[allow(dead_code)]
    pub fn get_local_package_count(&self) -> usize {
        let mut count = 0;
        for (_name, versions) in self.packages.iter() {
            count += versions.values().map(|entries| entries.len()).sum::<usize>();
        }
        count
    }

    /// Automatically rescans the workspace if needed based on manifest file status.
    ///   - If any manifest files are missing, performs a full rescan
    ///   - If manifest files are only updated, rescans only the folders containing those files
    ///
    /// Returns an AutoRescanResult indicating what action was taken
    pub fn auto_rescan_if_needed(&mut self) -> Result<AutoRescanResult, CommandError> {
        // If workspace is not ready, return early - no auto-rescan needed
        if !self.ready() {
            return Ok(AutoRescanResult::NoActionNeeded);
        }

        // Get the modification time of the workspace index file
        let index_path = self.get_nosman_index_filepath();
        let index_modified_time = match fs::metadata(&index_path).and_then(|m| m.modified()) {
            Ok(time) => time,
            Err(_) => {
                self.rescan(RescanFlags::ScanPackages)?;
                return Ok(AutoRescanResult::FullRescanMissingIndex);
            }
        };

        // Collect all packages for parallel processing
        let all_packages: Vec<&LocalPackageEntry> = self.packages
            .values()
            .flat_map(|versions| versions.values().flatten())
            .collect();

        // Use parallel iterator to check manifest files
        use rayon::prelude::*;
        use std::sync::atomic::{AtomicBool, Ordering};
        
        let missing_manifests = AtomicBool::new(false);
        let updated_folders = Mutex::new(Vec::new());
        
        all_packages.par_iter().for_each(|package| {
            // If we already found missing manifests, skip further processing
            if missing_manifests.load(Ordering::Relaxed) {
                return;
            }
            
            let manifest_path = self.root.join(&package.manifest_path);
            
            // Check if manifest file still exists
            if !manifest_path.exists() {
                missing_manifests.store(true, Ordering::Relaxed);
                return;
            }
            
            // Check if manifest file has been modified since the index was last saved
            if let Ok(metadata) = fs::metadata(&manifest_path) {
                if let Ok(modified_time) = metadata.modified() {
                    if modified_time > index_modified_time {
                        // Get the folder containing this manifest
                        if let Some(folder) = manifest_path.parent() {
                            let folder = folder.to_path_buf();
                            let mut folders = updated_folders.lock().unwrap();
                            if !folders.contains(&folder) {
                                folders.push(folder);
                            }
                        }
                    }
                }
            }
        });

        let missing_manifests = missing_manifests.load(Ordering::Relaxed);
        let updated_folders = updated_folders.into_inner().unwrap();

        // If any manifests are missing, do a full rescan
        if missing_manifests {
            self.rescan(RescanFlags::ScanPackages)?;
            return Ok(AutoRescanResult::FullRescanMissingManifests);
        }

        // If no updates, nothing to do
        if updated_folders.is_empty() {
            return Ok(AutoRescanResult::NoActionNeeded);
        }

        // Store the folders for the result
        let updated_folders_result = updated_folders.clone();

        // Rescan only the updated folders
        self.with_output_mode_scoped(OutputMode::Silent, |ws| {
            for folder in &updated_folders {
                // Remove packages from this folder first
                let folder_relative = get_rel_path_based_on(folder, &ws.root);
                ws.packages.retain(|_name, versions| {
                    versions.retain(|_version, entries| {
                        entries.retain(|package| {
                            let package_folder = package.manifest_path.parent()
                                .map(|p| p.to_path_buf())
                                .unwrap_or_default();
                            package_folder != folder_relative
                        });
                        !entries.is_empty()
                    });
                    !versions.is_empty()
                });

                // Rescan this specific folder
                ws.scan_packages_in_folder(folder.clone(), ScanPackagesFlags::ForceReplaceInRegistry | ScanPackagesFlags::RegisterCommands);
            }
        });

        // Save the updated workspace
        self.save()?;
        self.runtime.status = WorkspaceStatus::Ready;
        
        Ok(AutoRescanResult::PartialRescanUpdatedManifests(updated_folders_result))
    }
}

/// Accepts both the current per-version list and the older single entry, so existing
/// index files keep loading.
fn deserialize_packages<'de, D>(
    deserializer: D,
) -> Result<HashMap<String, HashMap<String, Vec<LocalPackageEntry>>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum OneOrMany {
        Many(Vec<LocalPackageEntry>),
        One(Box<LocalPackageEntry>),
    }
    let raw: HashMap<String, HashMap<String, OneOrMany>> = HashMap::deserialize(deserializer)?;
    Ok(raw
        .into_iter()
        .map(|(name, versions)| {
            let versions = versions
                .into_iter()
                .map(|(version, entry)| {
                    let entries = match entry {
                        OneOrMany::Many(v) => v,
                        OneOrMany::One(e) => vec![*e],
                    };
                    (version, entries)
                })
                .collect();
            (name, versions)
        })
        .collect())
}

pub fn find_root_from(path: &Path) -> Option<PathBuf> {
    let mut current = path.to_path_buf();
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

pub fn get_nosman_dir_for(path: &Path) -> PathBuf {
    path.join(".nosman")
}

pub fn get_nosman_index_filepath_for(path: &Path) -> PathBuf {
    get_nosman_dir_for(path).join("index")
}
