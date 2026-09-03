use std::collections::HashMap;
use std::fs::File;
use std::io::{Read};
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
#[cfg(target_os = "windows")]
use std::io::{Write};
use std::{path};
use std::path::{PathBuf};
use clap::{Arg, ArgAction, ArgMatches};
use colored::Colorize;
use libloading::{Symbol};
use serde::{Deserialize, Serialize};
use tempfile::{tempdir};
#[cfg(target_os = "windows")]
use zip::write::{SimpleFileOptions};
use globwalk::{GlobWalkerBuilder};
use crate::nosman::command::{Command, CommandError, CommandResult};
use crate::nosman::command::CommandError::{Runtime, InvalidArgument};
use crate::nosman::command::publish_interrupt;
use crate::nosman::{common, constants, git};
use crate::nosman::index::{PackageType, SemVer};
use crate::nosman::module::{get_resolved_binary_path, load_module};
use crate::nosman::package::PackageIdentifier;
use crate::nosman::path::get_package_manifest_file;
use crate::nosman::platform::{get_host_platform, Platform};
use crate::nosman::workspace::Workspace;
use crate::nosman::plugin::{PluginEntry};
use crate::nosman::ui;

#[derive(Serialize, Deserialize, Debug)]
#[serde(untagged)]
pub enum GlobsOrPlatformSpecificGlobs {
    Globs(Vec<String>),
    PlatformSpecificGlobs(HashMap<String, Vec<String>>)
}

impl GlobsOrPlatformSpecificGlobs {
    pub fn get_resolved_globs(unresolved: &GlobsOrPlatformSpecificGlobs, target_platform: &Platform) -> Vec<String> {
        match unresolved {
            GlobsOrPlatformSpecificGlobs::Globs(globs) => globs.clone(),
            GlobsOrPlatformSpecificGlobs::PlatformSpecificGlobs(platform_globs) => {
                let platform_globs = platform_globs.get(&target_platform.to_string());
                if platform_globs.is_none() {
                    return vec![];
                }
                platform_globs.unwrap().clone()
            }
        }
    }
}

fn default_true() -> bool { true }

/// `.nospub` `changelog` block. When publishing without an explicit
/// `--changelog` or `CHANGELOG.md`, nosman derives release notes from the git
/// commit log of this package since its previous release tag.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ChangelogOptions {
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Glob used to find the previous release tag (passed to `git describe
    /// --match`). The tokens `{name}` and `{target}` are substituted with the
    /// package name and target platform. Defaults to
    /// `release-{name}-*-{target}`.
    #[serde(default)]
    pub previous_tag_glob: Option<String>,
}

impl Default for ChangelogOptions {
    fn default() -> Self {
        ChangelogOptions { enabled: true, previous_tag_glob: None }
    }
}

/// `.nospub` `tag` block. After a successful publish nosman creates (and by
/// default pushes) a `release-<name>-<version>-<target>` git tag so the next
/// publish has a baseline for changelog generation.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct TagOptions {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_true")]
    pub push: bool,
}

impl Default for TagOptions {
    fn default() -> Self {
        TagOptions { enabled: true, push: true }
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct PublishOptionsFileContent {
    #[serde(alias = "globs")]
    pub release_globs: GlobsOrPlatformSpecificGlobs,
    pub target_platforms: Option<Vec<String>>,
    #[serde(default)]
    pub changelog: Option<ChangelogOptions>,
    #[serde(default)]
    pub tag: Option<TagOptions>,
}

impl PublishOptionsFileContent {
    pub fn from_file(nospub_file: &PathBuf) -> (PublishOptionsFileContent, bool) {
        let mut nospub = Self::empty();
        let found = nospub_file.exists();
        if found {
            match common::read_file_contents(nospub_file, "publish options") {
                Ok(contents) => {
                    nospub = serde_json::from_str(&contents).unwrap();
                }
                Err(e) => {
                    ui::warn(format!("could not read the publish options file: {}", e));
                    return (Self::empty(), false);
                }
            }
        }
        (nospub, found)
    }
    pub fn empty() -> PublishOptionsFileContent {
        PublishOptionsFileContent { release_globs: GlobsOrPlatformSpecificGlobs::Globs(vec![]), target_platforms: None, changelog: None, tag: None }
    }
}

pub struct PublishOptions {
    pub(crate) release_globs: Vec<String>,
    pub(crate) target_platforms: Option<Vec<String>>,
    pub(crate) changelog: ChangelogOptions,
    pub(crate) tag: TagOptions,
}

impl PublishOptions {
    pub fn from_file(nospub_file: &PathBuf) -> (PublishOptions, bool) {
        let (nospub, found) = PublishOptionsFileContent::from_file(nospub_file);
        if !found {
            return (PublishOptions::all(), false);
        }
        let mut options = PublishOptions::empty();
        options.release_globs = GlobsOrPlatformSpecificGlobs::get_resolved_globs(&nospub.release_globs, &get_host_platform());
        options.target_platforms = nospub.target_platforms;
        options.changelog = nospub.changelog.unwrap_or_default();
        options.tag = nospub.tag.unwrap_or_default();
        (options, true)
    }
    pub fn empty() -> PublishOptions {
        PublishOptions { release_globs: vec![], target_platforms: None, changelog: ChangelogOptions::default(), tag: TagOptions::default() }
    }
    pub fn all() -> PublishOptions {
        let mut options = PublishOptions::empty();
        options.release_globs = vec!["**".to_string()];
        options
    }
}

/// Result of a successful publish: the published package identifier and the
/// git release tag created for it (if any), so callers can roll the tag back.
#[derive(Debug)]
pub struct PublishOutcome {
    pub id: PackageIdentifier,
    pub created_tag: Option<String>,
}

pub struct PublishCommand {
}

impl PublishCommand {
    fn is_name_valid(name: &str) -> bool {
        // Should be lowercase alphanumeric, with only . and _ symbols are permitted
        name.chars().all(|c| c == '.' || c == '_' || c.is_numeric() || c.is_ascii_lowercase())
    }

    /// Resolve the changelog for a release. Priority: an explicit `--changelog`
    /// value, then a `CHANGELOG.md` in the package directory, then a changelog
    /// generated from the git commit log of this package since its previous
    /// release tag. Returns `None` when none are available, leaving the Nodos
    /// Store to generate a changelog from the artifact diff.
    fn resolve_changelog(changelog_arg: Option<String>, abs_path: &PathBuf, name: &str,
                         target_platform: &Platform, cfg: &ChangelogOptions) -> Option<String> {
        if let Some(text) = changelog_arg {
            ui::detail("using the changelog given on the command line");
            return Some(text);
        }
        if abs_path.is_dir() {
            let changelog_path = abs_path.join("CHANGELOG.md");
            if changelog_path.exists() {
                match std::fs::read_to_string(&changelog_path) {
                    Ok(contents) => {
                        ui::detail(format!("using the changelog at {}", changelog_path.display()));
                        return Some(contents);
                    }
                    Err(e) => {
                        ui::warn(format!(
                            "could not read {} ({}), falling back to the git or artifact changelog",
                            changelog_path.display(), e
                        ));
                    }
                }
            }
        }
        Self::generate_git_changelog(abs_path, name, target_platform, cfg)
    }

    /// Best-effort fetch of tags (and full history when the clone is shallow)
    /// so changelog baseline detection and release-tag de-duplication work even
    /// under CI's shallow `--depth 1` checkouts. Failures are warnings only.
    pub fn ensure_tags_fetched(abs_path: &PathBuf, verbose: bool) {
        if !git::is_inside_work_tree(abs_path) {
            return;
        }
        let unshallow = git::is_shallow(abs_path);
        match git::fetch_tags(abs_path, unshallow) {
            Ok(()) => {
                if verbose {
                    ui::detail(format!("fetched git tags{}", if unshallow { " and unshallowed history" } else { "" }));
                }
            }
            Err(e) => ui::warn(format!("could not fetch git tags ({}), so the changelog and tag may be incomplete", e)),
        }
    }

    /// Build a changelog from the git commit log of this package since its
    /// previous release tag. Returns `None` when generation is not possible
    /// (disabled, not a git repo, no previous tag, or no new commits), in which
    /// case the Nodos Store falls back to the artifact diff.
    fn generate_git_changelog(abs_path: &PathBuf, name: &str, target_platform: &Platform,
                              cfg: &ChangelogOptions) -> Option<String> {
        if !cfg.enabled {
            return None;
        }
        if !git::is_inside_work_tree(abs_path) {
            return None;
        }
        let glob = cfg.previous_tag_glob.clone()
            .map(|g| g.replace("{name}", name).replace("{target}", &target_platform.to_string()))
            .unwrap_or_else(|| format!("release-{}-*-{}", name, target_platform));
        let baseline = git::describe_latest_tag(abs_path, &glob)?;
        let range = format!("{}..HEAD", baseline);
        let subjects = git::log_subjects(abs_path, &range, abs_path);
        if subjects.is_empty() {
            return None;
        }
        ui::detail(format!("using a changelog built from {} since {}", ui::plural(subjects.len(), "commit"), baseline));
        let mut out = format!("## Changes since {}\n\n", baseline);
        for s in subjects {
            out.push_str(&s);
            out.push('\n');
        }
        Some(out)
    }

    /// Tag the current HEAD as `release-<name>-<version>-<target>` and, when
    /// `push` is set, push it to `origin`. Returns the created tag name (for
    /// rollback), or `None` when nothing was created. A push failure is a
    /// warning only — the release has already been published.
    fn create_release_tag(abs_path: &PathBuf, name: &str, version: &str,
                          target_platform: &Platform, push: bool, verbose: bool) -> Option<String> {
        if !git::is_inside_work_tree(abs_path) {
            if verbose {
                ui::detail("not inside a git work tree, so no release tag");
            }
            return None;
        }
        let tag = format!("release-{}-{}-{}", name, version, target_platform);
        if git::tag_exists(abs_path, &tag) {
            ui::step_skipped("Kept", format!("the git tag {} that is already there", tag));
            return None;
        }
        match git::create_annotated_tag(abs_path, &tag, &format!("{} {}", name, version)) {
            Ok(()) => ui::step("Tagged", &tag),
            Err(e) => {
                ui::warn(format!("could not create git tag {}: {}", tag, e));
                return None;
            }
        }
        if push {
            match git::push_tag(abs_path, &tag) {
                Ok(()) => ui::step("Pushed", format!("tag {}", tag)),
                Err(e) => ui::warn(format!("could not push git tag {} ({}), the release is published either way", tag, e)),
            }
        }
        Some(tag)
    }

    fn get_plugin_api_version_from_binary(verbose: bool, package_type: &PackageType, manifest: &serde_json::Value, manifest_dir: &PathBuf, workspace: &Workspace) -> Result<Option<SemVer>, CommandError> {
        let binary_path = get_resolved_binary_path(package_type, manifest, manifest_dir);
        if binary_path.is_err() {
            return Ok(None);
        }
        let binary_path = binary_path.unwrap();
        if !binary_path.exists() {
            return Ok(None);
        }
        let lib = load_module(verbose, package_type, manifest, manifest_dir.clone(), workspace)?;
        if verbose {
            ui::detail(format!("loaded {}, reading its Nodos {:?} API version", binary_path.display(), package_type));
        }
        let get_api_version_func_name = "nosGetPluginAPIVersion";
        let mut api_version_opt: Option<SemVer>;
        unsafe {
                let get_api_version_func = lib.get::<Symbol<unsafe extern "C" fn(*mut i32, *mut i32, *mut i32)>>(get_api_version_func_name.as_bytes())
                    .or_else(|_| {
                        lib.get::<Symbol<unsafe extern "C" fn(*mut i32, *mut i32, *mut i32)>>("nosGetSubsystemAPIVersion".as_bytes())
                    }).unwrap_or_else(|e| panic!("Failed to get symbol {}: {}", get_api_version_func_name, e));
                let mut major = 0;
                let mut minor = 0;
                let mut patch = 0;
                get_api_version_func(&mut major, &mut minor, &mut patch);
                api_version_opt = Some(SemVer { major: (major as u32), minor: Some(minor as u32), patch: Some(patch as u32), build_number: None });
                ui::detail(format!("{:?} uses Nodos {:?} API version {}.{}.{}", binary_path, package_type, major, minor, patch));

                {
                    let get_min_required_minor_func_name = "nosGetMinimumRequiredPluginAPIMinorVersion";
                    if let Ok(get_min_required_minor_func) = lib.get::<Symbol<unsafe extern "C" fn(*mut i32)>>(get_min_required_minor_func_name.as_bytes()) {
                        let mut min_required_minor: i32 = 0;
                        get_min_required_minor_func(&mut min_required_minor);
                        if min_required_minor > 0 {
                            api_version_opt.as_mut().unwrap().minor = Some(min_required_minor as u32);
                            ui::detail(format!("{:?} needs at least Nodos {:?} API minor version {}", binary_path, package_type, min_required_minor));
                        }
                    }
                }
            }
        Ok(api_version_opt)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn publish(&self, workspace: &mut Workspace, dry_run: bool, verbose: bool, path: &PathBuf,
                   mut name: Option<String>, mut version: Option<String>, version_suffix: &str,
                   mut package_type: Option<PackageType>, release_tags: &Vec<String>,
                   opt_target_platform: Option<&String>,
                   visibility: nodos_store_client::PackageVisibility,
                   changelog_arg: Option<String>,
                   create_tag: bool, push_tag: bool, fetch_tags: bool,
    ) -> Result<PublishOutcome, CommandError> {

        let target_platform = match opt_target_platform {
            Some(platform_str) => Platform::from_str(platform_str).expect("Invalid target platform"),
            None => {
                let current_platform = get_host_platform();
                ui::detail(format!("no target platform given, using this one: {}", current_platform));
                current_platform
            }
        };

        if !path.exists() {
            return Err(InvalidArgument { message: format!("Path {} does not exist", path.display()) });
        }

        let abs_path = dunce::canonicalize(path).unwrap_or_else(|e| panic!("Failed to canonicalize path {}: {}", path.display(), e));

        let mut publish_options = PublishOptions::empty();

        let mut api_version_opt: Option<SemVer> = None;

        let mut dependencies: Option<Vec<PackageIdentifier>> = None;
        let mut package_tags: Option<Vec<String>> = None;
        let mut node_class_names: Vec<String> = vec![];

        // If path is a directory, search for a manifest file
        let mut manifest_file = None;
        if abs_path.is_dir() {
            let (options, found) = PublishOptions::from_file(&abs_path.join(constants::PUBLISH_OPTIONS_FILE_NAME));
            publish_options = options;
            if !found {
                ui::warn(format!("no {} in {}, so every file goes into the release", constants::PUBLISH_OPTIONS_FILE_NAME, abs_path.display()));
            } else if let Some(targets) = publish_options.target_platforms {
                if !targets.contains(&target_platform.to_string()) {
                    return Err (InvalidArgument { message: format!("Target platform {} is not in the list of target platforms in {}", target_platform, constants::PUBLISH_OPTIONS_FILE_NAME) });
                }
            }

            let res = get_package_manifest_file(&abs_path);
            if let Err(msg) = res {
                return Err(Runtime { message: msg });
            }
            if let Ok(Some((pkg_type, file))) = res {
                manifest_file = Some(file);
                package_type = Some(pkg_type);
            }
            if let Some(manifest_file) = manifest_file.as_ref() {
                let package_type = package_type.as_ref().unwrap();
                let contents = std::fs::read_to_string(manifest_file)?;
                let res = serde_json::from_str(&contents);
                if let Err(err) = res {
                    return Err(Runtime { message: format!("Failed to parse package manifest file {:?}: {}", manifest_file, err) });
                }
                let manifest: serde_json::Value = res.unwrap();
                name = Some(manifest["info"]["id"]["name"].as_str().unwrap_or_else(|| panic!("Package manifest file {:?} must contain info.id.name field!", manifest_file)).to_string());
                version = Some(manifest["info"]["id"]["version"].as_str().unwrap_or_else(|| panic!("Package manifest file {:?} must contain info.id.version field!", manifest_file)).to_string());
                // display_name/description/category are store-owned metadata and
                // are intentionally not read from the manifest or sent on publish.
                let dependencies_json = manifest["info"]["dependencies"].as_array();
                if let Some(deps_json) = dependencies_json {
                    let mut deps = vec![];
                    for dep in deps_json {
                        let dep_name = dep["name"].as_str().unwrap();
                        let dep_version = dep["version"].as_str().unwrap();
                        deps.push(PackageIdentifier { name: dep_name.to_string(), version: dep_version.to_string() });
                    }
                    dependencies = Some(deps);
                }
                package_tags = manifest["info"]["tags"].as_array().map(|a| a.iter().map(|v| v.as_str().unwrap().to_string()).collect());
                if package_type.is_plugin() {
                    let sdk_version = manifest["sdk_version"].as_str().map(|s| s.to_string());

                    // 1.4+ plugins spesify sdk_version in manifest, and we can get node class names from manifest without loading the binary.
                    if sdk_version.is_some(){
                        api_version_opt = sdk_version.as_ref().and_then(|s| SemVer::parse_from_str(s.as_str()));

                        if workspace.is_installed(name.as_ref().unwrap(), version.as_ref().unwrap()) {
                            let local_package = workspace.get_package(name.as_ref().unwrap(), version.as_ref().unwrap())?.clone();
                            let local_package = workspace.absolutize_paths(&local_package);
                            if let Ok(plugin) = PluginEntry::new(local_package) {
                                plugin.get_node_definitions().iter().for_each(|node_def| {
                                    node_class_names.push(node_def.class_name.clone());  
                                });  
                            }  
                        }
                    }
                    // 1.3 and below plugins do not specify sdk_version, and we have to load the binary to get the API version.
                    // Node class names are not published.
                    else {
                        if !workspace.ready() {
                            return Err(Runtime { message: format!(
                                "Package {} has no sdk_version in its manifest (legacy plugin). Detecting the API version requires loading the binary with workspace-resolved dependency search paths, but no workspace was found at {}. Run this command from within a workspace, or update the plugin manifest to include sdk_version.",
                                name.as_ref().unwrap(), workspace.root.display()) });
                        }
                        api_version_opt = Self::get_plugin_api_version_from_binary(verbose, package_type, &manifest, &abs_path, workspace)?;
                    }
                }
            }
        }
        let package_type = package_type.unwrap();

        if name.is_none() {
            return Err(InvalidArgument { message: "Name is not provided and could not be inferred".to_string() });
        }
        if version.is_none() {
            return Err(InvalidArgument { message: "Version is not provided and could not be inferred".to_string() });
        }

        ui::detail(format!("target platform {:?}", target_platform));

        let name = name.unwrap();
        let version = version.unwrap() + version_suffix;
        let tag = format!("{}-{}-{}", name, version, target_platform);
        let dependencies = dependencies.unwrap_or_default();
        let mut combined_tags = package_tags.unwrap_or_default();
        for release_tag in release_tags {
            if !combined_tags.iter().any(|tag| tag == release_tag) {
                combined_tags.push(release_tag.clone());
            }
        }

        ui::step("Publishing", &tag);
        let pb = ui::spinner("Preparing release");
        if !Self::is_name_valid(&name) {
            return Err(InvalidArgument { message: format!("Name {} is not valid. It should match regex [a-z0-9._]", name) });
        }
        if SemVer::parse_from_str(version.as_str()).is_none() {
            return Err(InvalidArgument { message: format!("Version should be semantic-versioning compatible: {}", version) });
        }
        let artifact_file_path;
        let temp_dir = tempdir()?;

        if abs_path.is_dir() {
            pb.println("Following files will be included in the release:".yellow().to_string().as_str());
            pb.set_message("Scanning files".to_string());
            let mut files_to_release = vec![];

            let walker = GlobWalkerBuilder::from_patterns(&abs_path, &publish_options.release_globs)
                .build()
                .unwrap_or_else(|e| panic!("Failed to glob dirs {:?}: {}", publish_options.release_globs, e));
            for entry in walker {
                let entry = entry.unwrap();
                if entry.file_type().is_dir() {
                    continue;
                }
                let path = entry.path().to_path_buf();
                pb.println(format!("\t{}", path.display()).as_str());
                files_to_release.push(path);
            }

            let host_platform = get_host_platform();
            if target_platform.os != host_platform.os {
                pb.println(format!("Target OS ({}) is different from host OS ({}). Using hosts archive format.", target_platform.os, host_platform.os).yellow().to_string().as_str());
            }

            let mut file_buffer_pairs = vec![];
            for file_path in files_to_release.iter() {
                let mut file = File::open(file_path).unwrap_or_else(|e| panic!("Failed to open file {:?}: {}", file_path, e));
                let mut buffer = Vec::new();
                file.read_to_end(&mut buffer).unwrap_or_else(|e| panic!("Failed to read file {:?}: {}", file_path, e));
                // If this is the manifest file, update the version
                if let Some(m) = &manifest_file {
                    if file_path == m {
                        let mut manifest: serde_json::Value = serde_json::from_slice(&buffer).unwrap();
                        manifest["info"]["id"]["version"] = serde_json::Value::String(version.clone());
                        if package_type == PackageType::Generic {
                            let schema_ver = manifest["schema_version"].as_str();
                            if schema_ver.is_none() {
                                manifest["schema_version"] = serde_json::Value::String(constants::GENERIC_PACKAGE_MANIFEST_SCHEMA_VERSION.to_string());
                            }
                        }
                        pb.println(format!("Updated version to {} in manifest file: {}", version.clone(), m.display()).as_str());
                        buffer = serde_json::to_vec_pretty(&manifest).unwrap();
                    }
                }
                file_buffer_pairs.push((file_path.clone(), buffer));
            }

            let archive_file_name = format!("{}.{}", tag, if host_platform.os == "windows" { "zip" } else { "tar.gz" });
            let archive_file_path = temp_dir.path().join(&archive_file_name);
            let archive_file = File::create(&archive_file_path).unwrap_or_else(|e| panic!("Failed to create file {:?}: {}", archive_file_path, e));

            #[cfg(target_os = "windows")]
            let mut writer = zip::ZipWriter::new(archive_file);

            #[cfg(target_os = "windows")]
            let options = SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated);

            #[cfg(unix)]
            let mut writer = tar::Builder::new(flate2::write::GzEncoder::new(archive_file, flate2::Compression::default()));

            for (file_path, buffer) in file_buffer_pairs.iter() {
                pb.set_message(format!("Creating a release: {}", file_path.display()).as_str().to_string());
                let stripped = file_path.strip_prefix(&abs_path)
                    .unwrap_or_else(|e| panic!("Failed to strip prefix {:?} from {:?}: {}", abs_path, file_path, e));
                #[cfg(target_os = "windows")]
                {
                    writer.start_file(stripped.to_str()
                                          // Convert backslashes to forward slashes
                                          .map(|s| s.replace("\\", "/"))
                                          .expect("Failed to convert path to string"), options)
                        .unwrap_or_else(|e| panic!("Failed to start file in zip {:?}: {}", file_path, e));
                    writer.write_all(buffer).unwrap_or_else(|e| panic!("Failed to write to zip {:?}: {}", file_path, e));
                }
                #[cfg(unix)]
                {
                    let mut header = tar::Header::new_gnu();
                    header.set_path(stripped
                        .to_str().expect("Failed to convert path to string").to_string()).expect("Failed to set path");
                    header.set_size(buffer.len() as u64);
                    let metadata = file_path.metadata().expect("Failed to get metadata");
                    header.set_mode(metadata.permissions().mode());
                    // Seconds since the Unix epoch
                    if let Ok(modified) = metadata.modified() {
                        header.set_mtime(modified.duration_since(std::time::SystemTime::UNIX_EPOCH).unwrap().as_secs());
                    }
                    header.set_cksum();
                    writer.append(&header, &mut buffer.as_slice()).expect(format!("Failed to append file to tar: {}", file_path.display()).as_str());
                }
            }

            writer.finish().unwrap_or_else(|e| panic!("Failed to finish archive {:?}: {}", archive_file_path, e));
            artifact_file_path = archive_file_path;
        } else {
            pb.set_message(format!("Creating a release: {}", abs_path.display()).as_str().to_string());
            artifact_file_path = abs_path.clone();
        }

        ui::finish_progress();

        if !node_class_names.is_empty() && verbose {
            ui::detail(format!("node definitions found for {}: {:?}", name, node_class_names));
        }

        ui::step("Publishing", format!("{}=={} to the Nodos Store", name, version));

        let mut created_tag = None;
        if dry_run {
            ui::step("Dry run", format!("would publish {}=={} for {}", name, version, target_platform));
            if create_tag && publish_options.tag.enabled {
                ui::step("Dry run", format!("would create the git tag release-{}-{}-{}", name, version, target_platform));
            }
        } else {
            if fetch_tags {
                Self::ensure_tags_fetched(&abs_path, verbose);
            }
            let changelog = Self::resolve_changelog(changelog_arg, &abs_path, &name, &target_platform, &publish_options.changelog);
            let api_version = api_version_opt.as_ref().map(|v| nodos_store_client::ApiVersion {
                major: v.major,
                minor: v.minor,
                patch: v.patch,
            });
            let deps: Vec<nodos_store_client::PackageDependency> = dependencies
                .iter()
                .map(|d| nodos_store_client::PackageDependency {
                    name: d.name.clone(),
                    version: d.version.clone(),
                })
                .collect();
            {
                let target_platform_name = target_platform.to_string();
                let client = workspace.authenticated_store_client_mut()?;
                client
                    .discard_drafts_for_release(&name, &version, &target_platform_name)
                    .map_err(|e| Runtime { message: e.to_string() })?;
                let session = client
                    .create_publish_session(&nodos_store_client::PublishSessionCreateRequest {
                        name: name.clone(),
                        package_type: nodos_store_client::PackageType::from_str(&package_type.to_string()),
                        version: version.clone(),
                        api_version,
                        dependencies: deps,
                        tags: combined_tags,
                        artifacts: vec![nodos_store_client::PublishDraftArtifact {
                            target_platform: target_platform_name.clone(),
                        }],
                        visibility,
                        changelog,
                    })
                    .map_err(|e| Runtime { message: e.to_string() })?;
                let session_id = session.session.id;
                let cleanup = nodos_store_client::PublishSessionCleanup::new(client, session_id);
                let _watching =
                    publish_interrupt::watch(session_id, &name, &version, &target_platform_name);

                client
                    .upload_release_artifact(
                        session_id,
                        &target_platform_name,
                        nodos_store_client::ArtifactSource::File(&artifact_file_path),
                    )
                    .and_then(|_| client.finalize_publish_session(session_id))
                    .map_err(|e| Runtime { message: e.to_string() })?;

                cleanup.keep();
            }

            if create_tag && publish_options.tag.enabled {
                created_tag = Self::create_release_tag(&abs_path, &name, &version, &target_platform,
                                                       push_tag && publish_options.tag.push, verbose);
            }
        }
        ui::step("Published", format!("{}=={}", name, version));
        Ok(PublishOutcome { id: PackageIdentifier { name, version }, created_tag })
    }

    #[allow(clippy::too_many_arguments)]
    pub fn run_publish(&self, workspace: &mut Workspace, dry_run: bool, verbose: bool, path: &PathBuf,
                       name: Option<String>, version: Option<String>, version_suffix: &str,
                       package_type: Option<PackageType>, release_tags: &Vec<String>,
                       opt_target_platform: Option<&String>,
                       visibility: nodos_store_client::PackageVisibility,
                       changelog: Option<String>,
                       create_tag: bool, push_tag: bool, fetch_tags: bool,
    ) -> CommandResult {
        let res = self.publish(workspace, dry_run, verbose,
                               path, name, version,
                               version_suffix, package_type, release_tags, opt_target_platform,
                               visibility, changelog, create_tag, push_tag, fetch_tags);
        if res.is_err() {
            return Err(res.err().unwrap());
        }
        Ok(())
    }
}

pub fn get_cli() -> clap::Command {
    clap::Command::new("publish")
        .about("Publish a package")
        .after_help("This command will publish a package to the Nodos Store.\n\
    If there is an existing Nodos release, updates it (note that this will remove all installed Nodos engines!)")
        .arg(Arg::new("path")
            .long("path")
            .short('p')
            .help(format!("Path to the root folder of the package (or a file) to be published.\n\
        If not provided, the current directory will be used.\n\
        If the path is a folder and it does not contain a {} file, it will add all files to the release.", constants::PUBLISH_OPTIONS_FILE_NAME))
            .default_value(".")
        )
        .arg(Arg::new("name")
            .long("name")
            .short('n')
            .help("Name of the package. It will be overridden by the package manifest files under <path> if present.\n\
        If the <path> does not contain a package manifest file, this parameter is required."))
        .arg(Arg::new("version")
            .long("version")
            .help("Version of the package. It will be overridden by the package manifest files under <path> if present.\n\
        If the <path> does not contain a package manifest file, this parameter is required.")
        )
        .arg(Arg::new("version_suffix")
            .long("version-suffix")
            .help("Suffix to append to the version of the package.")
            .default_value("")
        )
        .arg(Arg::new("type")
            .long("type")
            .short('t')
            .value_parser(clap::builder::PossibleValuesParser::new(["plugin", "subsystem", "nodos", "engine", "generic"]))
            .help("Type of the package. It will be overridden by the package manifest files under <path> if present.\n\
        If the <path> does not contain a package manifest file, this parameter is required.")
        )
        .arg(Arg::new("dry_run")
            .action(ArgAction::SetTrue)
            .long("dry-run")
            .help("Do not actually publish the package, just show what would be done.")
            .num_args(0)
            .required(false)
        )
        .arg(Arg::new("tag")
            .action(ArgAction::Append)
            .long("tag")
            .help("Add a tag to the release. Can be specified multiple times.")
            .required(false)
            .num_args(1)
        )
        .arg(Arg::new("target_platform")
            .long("target-platform")
            .help("Target architecture and operating system of the package to be published. If not provided, the current platform will be used.")
            .required(false)
        )
        .arg(Arg::new("changelog")
            .long("changelog")
            .help("Changelog / release notes for this version. If omitted, a CHANGELOG.md file in the package directory is used when present; otherwise the Nodos Store generates a changelog from the artifact diff.")
            .required(false)
        )
        .arg(Arg::new("visibility")
            .long("visibility")
            .value_parser(clap::builder::PossibleValuesParser::new(["public", "private"]))
            .default_value("public")
            .help("Package visibility on first publish. 'public' lets anyone download; 'private' restricts downloads to namespace members and explicitly granted accounts. Ignored when the package already exists on the store (manage existing-package visibility from the Nodos Store dashboard).")
            .required(false)
        )
        .arg(Arg::new("no_tag")
            .action(ArgAction::SetTrue)
            .long("no-tag")
            .help("Do not create a release-<name>-<version>-<target> git tag after a successful publish.")
            .num_args(0)
            .required(false)
        )
        .arg(Arg::new("no_push_tag")
            .action(ArgAction::SetTrue)
            .long("no-push-tag")
            .help("Create the release git tag locally but do not push it to the remote.")
            .num_args(0)
            .required(false)
        )
        .arg(Arg::new("no_fetch_tags")
            .action(ArgAction::SetTrue)
            .long("no-fetch-tags")
            .help("Do not fetch tags / unshallow the repo before generating the changelog. By default nosman fetches tags (and unshallows a shallow clone) so changelog generation works under CI's shallow checkouts.")
            .num_args(0)
            .required(false)
        )
}

fn parse_visibility(value: &str) -> nodos_store_client::PackageVisibility {
    nodos_store_client::PackageVisibility::from_str(value)
}

impl Command for PublishCommand {
    fn matched_args<'a>(&self, _workspace: &Workspace, args : &'a ArgMatches) -> Option<&'a ArgMatches> {
        args.subcommand_matches("publish")
    }

    fn run(&self, workspace: &mut Workspace, _command_name: Option<&str>, args: &ArgMatches) -> CommandResult {
        let path = path::PathBuf::from(args.get_one::<String>("path").unwrap());
        let opt_name = args.get_one::<String>("name");
        let opt_version = args.get_one::<String>("version");
        let version_suffix = args.get_one::<String>("version_suffix").unwrap();
        let package_type: Option<PackageType> = args.get_one::<String>("type").map(|s| serde_json::from_str(format!("\"{}\"", &s).as_str()).unwrap());
        let version = opt_version.cloned();
        let name = opt_name.cloned();
        let dry_run = args.get_one::<bool>("dry_run").unwrap();
        let verbose = ui::is_verbose();
        let release_tags_ref: Vec<&String> = args.get_many::<String>("tag").unwrap_or_default().collect();
        let release_tags: Vec<String> = release_tags_ref.iter().map(|s| s.to_string()).collect();
        let target_platform: Option<&String> = args.get_one::<String>("target_platform");
        let visibility = parse_visibility(args.get_one::<String>("visibility").map(String::as_str).unwrap_or("public"));
        let changelog = args.get_one::<String>("changelog").cloned();
        let create_tag = !*args.get_one::<bool>("no_tag").unwrap();
        let push_tag = !*args.get_one::<bool>("no_push_tag").unwrap();
        let fetch_tags = !*args.get_one::<bool>("no_fetch_tags").unwrap();
        self.run_publish(
            workspace,
            *dry_run,
            verbose,
            &path,
            name,
            version,
            version_suffix,
            package_type,
            &release_tags,
            target_platform,
            visibility,
            changelog,
            create_tag,
            push_tag,
            fetch_tags,
        )
    }

    fn needs_workspace(&self) -> bool {
        false
    }
}
