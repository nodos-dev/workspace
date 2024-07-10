use std::fs::File;
use std::io::{Read, Write};
#[cfg(not(target_os = "windows"))]
use std::env;
#[cfg(target_os = "windows")]
use std::os::windows::ffi::OsStrExt;
use std::path;
use std::ffi::OsString;
use std::path::PathBuf;
use std::time::Duration;
use clap::{ArgMatches};
use colored::Colorize;
use indicatif::ProgressBar;
use libloading::{Library, library_filename, Symbol};
use serde::{Deserialize, Serialize};
use tempfile::{tempdir};
use zip::write::{SimpleFileOptions};

use crate::nosman::command::{Command, CommandError, CommandResult};
use crate::nosman::command::CommandError::{GenericError, InvalidArgumentError};
use crate::nosman::constants;
use crate::nosman::index::{PackageReleaseEntry, PackageType, SemVer};
use crate::nosman::path::{get_plugin_manifest_file, get_subsystem_manifest_file};
use crate::nosman::workspace::Workspace;

#[derive(Serialize, Deserialize, Debug)]
pub struct PublishOptions {
    #[serde(alias = "globs")]
    pub(crate) release_globs: Vec<String>,
    #[serde(alias = "trigger_publish_globs")]
    pub(crate) additional_publish_triggering_globs: Option<Vec<String>>
}

impl PublishOptions {
    pub fn from_file(nospub_file: &PathBuf) -> (PublishOptions, bool) {
        let mut nospub = PublishOptions { release_globs: vec![], additional_publish_triggering_globs: None };
        let found = nospub_file.exists();
        if found {
            let contents = std::fs::read_to_string(&nospub_file).unwrap();
            nospub = serde_json::from_str(&contents).unwrap();
        }
        else {
            nospub.release_globs.push("**".to_string());
        }
        return (nospub, found);
    }
    pub fn empty() -> PublishOptions {
        PublishOptions { release_globs: vec![], additional_publish_triggering_globs: None }
    }
}

pub struct PublishCommand {
}

impl PublishCommand {
    fn load_module_with_search_paths(verbose: bool, binary_path: &OsString, additional_search_paths: Vec<PathBuf>) -> Result<Library, CommandError> {
        if verbose {
            println!("Loading dynamic library: {}", binary_path.to_str().unwrap());
        }
        #[cfg(not(target_os = "windows"))]
        {
            // Store the original environment variable values
            #[cfg(target_os = "linux")]
            let original_var = env::var_os("LD_LIBRARY_PATH");

            #[cfg(target_os = "macos")]
            let original_var = env::var_os("DYLD_LIBRARY_PATH");


            {
                for lib_dir in additional_search_paths {
                    // Add this directory to the appropriate environment variable
                    #[cfg(target_os = "linux")]
                    {
                        let mut paths = env::var_os("LD_LIBRARY_PATH").unwrap_or_else(|| "".into());
                        let mut lib_dir = lib_dir.clone();
                        lib_dir.push(":");
                        lib_dir.push(paths);
                        env::set_var("LD_LIBRARY_PATH", lib_dir);
                    }

                    #[cfg(target_os = "macos")]
                    {
                        let mut paths = env::var_os("DYLD_LIBRARY_PATH").unwrap_or_else(|| "".into());
                        let mut lib_dir = lib_dir.clone();
                        lib_dir.push(":");
                        lib_dir.push(paths);
                        env::set_var("DYLD_LIBRARY_PATH", lib_dir);
                    }
                }
            }



            let res;
            // Now load the library
            unsafe {
                res = Library::new(&binary_path)
            }

            {
                // Restore the original environment variable values
                #[cfg(target_os = "linux")]
                if let Some(original) = original_var {
                    env::set_var("LD_LIBRARY_PATH", original);
                } else {
                    env::remove_var("LD_LIBRARY_PATH");
                }

                #[cfg(target_os = "macos")]
                if let Some(original) = original_var {
                    env::set_var("DYLD_LIBRARY_PATH", original);
                } else {
                    env::remove_var("DYLD_LIBRARY_PATH");
                }
            }

            if res.is_err() {
                return Err(GenericError { message: format!("Failed to load dynamic library: {}", res.err().unwrap()) });
            }
            Ok(res.unwrap())
        }

        #[cfg(target_os = "windows")]
        unsafe {
            // Set default DLL directories
            use winapi::um::libloaderapi::{SetDefaultDllDirectories, AddDllDirectory, RemoveDllDirectory};
            use winapi::um::libloaderapi::LOAD_LIBRARY_SEARCH_DEFAULT_DIRS;
            if 0 == SetDefaultDllDirectories(LOAD_LIBRARY_SEARCH_DEFAULT_DIRS) {
                // Get last error
                let err = std::io::Error::last_os_error();
                return Err(GenericError { message: format!("Failed to set default DLL directories: {}", err) });
            }
            let mut dll_cookies = vec![];
            for lib_dir in additional_search_paths {
                if !lib_dir.exists() {
                    println!("{}", format!("Warning: DLL search path {} does not exist", lib_dir.display()).yellow().to_string());
                    continue;
                }
                let lib_dir_canonical = dunce::canonicalize(&lib_dir).expect(format!("Failed to canonicalize path: {}", lib_dir.display()).as_str());
                if verbose {
                    println!("\tAdding DLL search path: {}", lib_dir_canonical.display());
                }
                let wdir: Vec<u16> = lib_dir_canonical.as_os_str().encode_wide().chain(Some(0)).collect();
                let cookie = AddDllDirectory(wdir.as_ptr());
                if cookie.is_null() {
                    let err = std::io::Error::last_os_error();
                    return Err(GenericError { message: format!("Failed to add DLL search path {}: {}", lib_dir_canonical.display(), err) });
                }
                dll_cookies.push(cookie);
            }
            let res = Library::new(&binary_path);
            for cookie in dll_cookies {
                RemoveDllDirectory(cookie);
            }
            if res.is_err() {
                return Err(GenericError { message: format!("Failed to load dynamic library: {}", res.err().unwrap()) });
            }
            Ok(res.unwrap())
        }
    }
    fn is_name_valid(name: &String) -> bool {
        // Should be lowercase alphanumeric, with only . and _ symbols are permitted
        name.chars().all(|c| c == '.' || c == '_' || c.is_numeric() || c.is_ascii_lowercase())
    }
    pub fn run_publish(&self, dry_run: bool, verbose: bool, path: &PathBuf, mut name: Option<String>, mut version: Option<String>, version_suffix: &String,
                   mut package_type: Option<PackageType>, remote_name: &String, vendor: Option<&String>,
                   publisher_name: Option<&String>, publisher_email: Option<&String>) -> CommandResult {
        // Check if git and gh is installed.
        let git_installed = std::process::Command::new("git")
            .arg("--version")
            .output()
            .is_ok();
        if !git_installed {
            return Err(GenericError { message: "git is not on PATH".to_string() });
        }
        let gh_installed = std::process::Command::new("gh")
            .arg("--version")
            .output()
            .is_ok();
        if !gh_installed {
            return Err(GenericError { message: "GitHub CLI client 'gh' is not on PATH".to_string() });
        }

        if !path.exists() {
            return Err(InvalidArgumentError { message: format!("Path {} does not exist", path.display()) });
        }

        let abs_path = dunce::canonicalize(path).expect(format!("Failed to canonicalize path: {}", path.display()).as_str());

        let mut nospub = PublishOptions::empty();

        let mut api_version: Option<SemVer> = None;

        // If path is a directory, search for a manifest file
        let mut manifest_file = None;
        if abs_path.is_dir() {
            let res = get_plugin_manifest_file(&abs_path);
            if res.is_err() {
                return Err(InvalidArgumentError { message: res.err().unwrap() });
            }
            let plugin_manifest_file = res.unwrap();
            let res = get_subsystem_manifest_file(&abs_path);
            if res.is_err() {
                return Err(InvalidArgumentError { message: res.err().unwrap() });
            }
            let subsystem_manifest_file = res.unwrap();
            if plugin_manifest_file.is_some() && subsystem_manifest_file.is_some() {
                return Err(InvalidArgumentError { message: format!("Multiple module manifest files found in {}", abs_path.display()) });
            }

            if plugin_manifest_file.is_some() {
                package_type = Some(PackageType::Plugin);
            } else if subsystem_manifest_file.is_some() {
                package_type = Some(PackageType::Subsystem);
            }

            manifest_file = plugin_manifest_file.or(subsystem_manifest_file);
            if manifest_file.is_some() {
                let package_type = package_type.as_ref().unwrap();
                let manifest_file = manifest_file.as_ref().unwrap();
                let contents = std::fs::read_to_string(manifest_file).unwrap();
                let manifest: serde_json::Value = serde_json::from_str(&contents).unwrap();
                name = Some(manifest["info"]["id"]["name"].as_str().expect(format!("Module manifest file {:?} must contain info.id.name field!", manifest_file).as_str()).to_string());
                version = Some(manifest["info"]["id"]["version"].as_str().expect(format!("Module manifest file {:?} must contain info.id.version field!", manifest_file).as_str()).to_string());
                let binary_path = manifest["binary_path"].as_str();
                if binary_path.is_some() {
                    // Binary path is relative to the manifest file
                    let module_dir = manifest_file.parent().unwrap();
                    let binary_path = module_dir.join(binary_path.unwrap());
                    let binary_path = library_filename(&binary_path);
                    let mut additional_search_paths: Vec<PathBuf> = Vec::new();
                    for path_str in manifest["additional_search_paths"].as_array().unwrap_or(&vec![]).iter() {
                        let path = module_dir.join(path_str.as_str().unwrap());
                        additional_search_paths.push(path);
                    }
                    // Add search paths of dependencies
                    for dep in manifest["info"]["dependencies"].as_array().unwrap_or(&vec![]) {
                        let dep_name = dep["name"].as_str().unwrap();
                        let dep_version = dep["version"].as_str().unwrap();
                        let ws = Workspace::get()?;
                        let dep_res = ws.get_latest_installed_module_for_version(dep_name, dep_version);
                        if let Ok(installed_module) = dep_res {
                            let dep_manifest_file_path = ws.root.join(&installed_module.config_path);
                            let dep_manifest_file_contents = std::fs::read_to_string(&dep_manifest_file_path).expect("Failed to read dependency manifest file");
                            let dep_manifest: serde_json::Value = serde_json::from_str(&dep_manifest_file_contents).expect("Failed to parse dependency manifest file");
                            for path_str in dep_manifest["additional_search_paths"].as_array().unwrap_or(&vec![]) {
                                let module_dir = dep_manifest_file_path.parent().unwrap();
                                let path = module_dir.join(path_str.as_str().unwrap());
                                additional_search_paths.push(path);
                            }
                        }
                    }
                    // Load the dynamic library
                    unsafe {
                        let lib = Self::load_module_with_search_paths(verbose, &binary_path, additional_search_paths);
                        if lib.is_err() {
                            return Err(InvalidArgumentError { message: format!("Could not load dynamic library {}: {}. \
                                Make sure all the dependencies are present in the system and the search paths.", &binary_path.to_str().unwrap(), lib.err().unwrap()) });
                        }
                        if verbose {
                            println!("Module {} loaded successfully. Checking Nodos {:?} API version...", name.as_ref().unwrap(), &package_type);
                        }
                        let lib = lib.unwrap();
                        let func_name = match package_type {
                            PackageType::Plugin => "nosGetPluginAPIVersion",
                            PackageType::Subsystem => "nosGetSubsystemAPIVersion",
                            _ => panic!("Invalid package type")
                        };
                        let func: Symbol<unsafe extern "C" fn(*mut i32, *mut i32, *mut i32)> = lib.get(func_name.as_bytes()).unwrap();
                        let mut major = 0;
                        let mut minor = 0;
                        let mut patch = 0;
                        func(&mut major, &mut minor, &mut patch);
                        api_version = Some(SemVer { major: (major as u32), minor: Some(minor as u32), patch: Some(patch as u32), build_number: None });
                        println!("{}", format!("{} uses Nodos {:?} API version: {}.{}.{}", name.as_ref().unwrap(), &package_type, major, minor, patch).as_str().yellow());
                    }
                }
            }

            let (options, found) = PublishOptions::from_file(&abs_path.join(constants::PUBLISH_OPTIONS_FILE_NAME));
            nospub = options;
            if !found {
                println!("{}", format!("No {} file found in {}. All files will be included in the release.", constants::PUBLISH_OPTIONS_FILE_NAME, abs_path.display()).as_str().yellow());
            }
        }
        let package_type = package_type.unwrap();

        if name.is_none() {
            return Err(InvalidArgumentError { message: "Name is not provided and could not be inferred".to_string() });
        }
        if version.is_none() {
            return Err(InvalidArgumentError { message: "Version is not provided and could not be inferred".to_string() });
        }
        let name = name.unwrap();
        let version = version.unwrap() + version_suffix;
        let tag = format!("{}-{}", name, version);

        let pb: ProgressBar = ProgressBar::new_spinner();
        pb.enable_steady_tick(Duration::from_millis(100));
        pb.println(format!("Publishing {}", tag).as_str().yellow().to_string());
        pb.set_message("Preparing release");
        if !Self::is_name_valid(&name) {
            return Err(InvalidArgumentError { message: format!("Name {} is not valid. It should match regex [a-z0-9._]", name) });
        }
        if None == SemVer::parse_from_string(version.as_str()) {
            return Err(InvalidArgumentError { message: format!("Version should be semantic-versioning compatible: {}", version) });
        }
        let workspace = Workspace::get()?;

        let artifact_file_path;
        let temp_dir = tempdir().unwrap();
        if abs_path.is_dir() {
            pb.println("Following files will be included in the release:".yellow().to_string().as_str());
            pb.set_message("Scanning files".to_string());
            let mut files_to_release = vec![];

            let walker = globwalk::GlobWalkerBuilder::from_patterns(&abs_path, &nospub.release_globs)
                .build()
                .expect(format!("Failed to glob dirs: {:?}", nospub.release_globs).as_str());
            for entry in walker {
                let entry = entry.unwrap();
                if entry.file_type().is_dir() {
                    continue;
                }
                let path = entry.path().to_path_buf();
                pb.println(format!("\t{}", path.display()).as_str());
                files_to_release.push(path);
            }
            let zip_file_name = format!("{}.zip", tag);
            let zip_file_path = temp_dir.path().join(&zip_file_name);
            let file = File::create(&zip_file_path).unwrap();
            let mut zip = zip::ZipWriter::new(file);

            let options = SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated);

            let mut buffer = Vec::new();
            for file_path in files_to_release.iter() {
                let mut file = File::open(file_path).unwrap();
                pb.set_message(format!("Creating a release: {}", file_path.display()).as_str().to_string());
                file.read_to_end(&mut buffer).unwrap();
                // If this is the manifest file, update the version
                if let Some(m) = &manifest_file {
                    if file_path == m {
                        let mut manifest: serde_json::Value = serde_json::from_slice(&buffer).unwrap();
                        manifest["info"]["id"]["version"] = serde_json::Value::String(version.clone());
                        pb.println(format!("Updated version to {} in manifest file: {}", version.clone(), m.display()).as_str());
                        buffer = serde_json::to_vec_pretty(&manifest).unwrap();
                    }
                }
                zip.start_file(file_path.strip_prefix(&abs_path)
                                   .expect(format!("Failed to strip prefix {} from {}", abs_path.display(), file_path.display()).as_str()).to_str()
                                   .expect("Failed to convert path to string"), options)
                    .expect(format!("Failed to start file in zip: {}", file_path.display()).as_str());
                zip.write_all(&buffer).expect(format!("Failed to write to zip: {}", file_path.display()).as_str());
                buffer.clear();
            }

            zip.finish().unwrap();
            artifact_file_path = zip_file_path;
        } else {
            pb.set_message(format!("Creating a release: {}", abs_path.display()).as_str().to_string());
            artifact_file_path = abs_path.clone();
        }

        // Create index entry for the release
        let remote = workspace.find_remote(remote_name);
        if remote.is_none() {
            return Err(InvalidArgumentError { message: format!("Remote {} not found", remote_name) });
        }
        let remote = remote.unwrap();

        let release = PackageReleaseEntry {
            version: version.clone(),
            url: format!("{}/releases/download/{}/{}", remote.url, tag, artifact_file_path.file_name().unwrap().to_str().unwrap()),
            plugin_api_version: match package_type {
                PackageType::Plugin => api_version.clone(),
                _ => None
            },
            subsystem_api_version: match package_type {
                PackageType::Subsystem => api_version,
                _ => None
            },
            release_date: None,
        };
        pb.finish_and_clear();

        println!("Adding package {} version {} release entry to remote {}", name, version, remote.name);
        let res = remote.fetch_add(dry_run, verbose, &workspace, &name, vendor, &package_type, release, publisher_name, publisher_email);
        if res.is_err() {
            return Err(GenericError { message: res.err().unwrap() });
        }
        let commit_sha = res.unwrap();

        println!("Uploading release {} for package {} version {} on remote {}", format!("{}-{}", name, version), name, version, remote.name);
        let res = remote.create_gh_release(dry_run, verbose, &workspace, &commit_sha, &name, &version, vec![artifact_file_path]);
        if res.is_err() {
            return Err(GenericError { message: res.err().unwrap() });
        }
        println!("{}", format!("Release {} for package {} version {} on remote {} created successfully", format!("{}-{}", name, version), name, version, remote.name).as_str().green().to_string());
        Ok(true)
    }
}

impl Command for PublishCommand {
    fn matched_args<'a>(&self, args : &'a ArgMatches) -> Option<&'a ArgMatches> {
        return args.subcommand_matches("publish");
    }

    fn needs_workspace(&self) -> bool {
        true
    }

    fn run(&self, args: &ArgMatches) -> CommandResult {
        let path = path::PathBuf::from(args.get_one::<String>("path").unwrap());
        let opt_name = args.get_one::<String>("name");
        let opt_version = args.get_one::<String>("version");
        let version_suffix = args.get_one::<String>("version_suffix").unwrap();
        let package_type: Option<PackageType> = args.get_one::<String>("type").map(|s| serde_json::from_str(format!("\"{}\"", &s).as_str()).unwrap());
        let remote_name = args.get_one::<String>("remote").unwrap();
        let version = if opt_version.is_some() { Some(opt_version.unwrap().clone()) } else { None };
        let name = if opt_name.is_some() { Some(opt_name.unwrap().clone()) } else { None };
        let vendor = args.get_one::<String>("vendor");
        let dry_run = args.get_one::<bool>("dry_run").unwrap();
        let verbose = args.get_one::<bool>("verbose").unwrap();
        let publisher_name = args.get_one::<String>("publisher_name");
        let publisher_email = args.get_one::<String>("publisher_email");
        self.run_publish(*dry_run, *verbose, &path, name, version, version_suffix, package_type, &remote_name, vendor, publisher_name, publisher_email)
    }
}
