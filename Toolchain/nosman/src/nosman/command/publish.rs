use std::fs::File;
use std::io::{Read, Write};
use std::path;
use clap::{ArgMatches};
use colored::Colorize;
use libloading::{Library, library_filename};
use serde::{Deserialize, Serialize};
use tempfile::tempdir;
use zip::unstable::write::FileOptionsExt;
use zip::write::{FileOptions, SimpleFileOptions};

use crate::nosman::command::{Command, CommandResult};
use crate::nosman::command::CommandError::{GenericError, InvalidArgumentError};
use crate::nosman::constants;
use crate::nosman::index::{PackageReleaseEntry, PackageType, SemVer};
use crate::nosman::path::{get_plugin_manifest_file, get_subsystem_manifest_file};
use crate::nosman::workspace::Workspace;


#[derive(Serialize, Deserialize, Debug)]
struct PublishOptions {
    #[serde(alias = "additional_globs")]
    globs: Vec<String>
}

pub struct PublishCommand {
}

impl PublishCommand {
    fn run_publish(&self, dry_run: &bool, path: &path::PathBuf, mut name: Option<String>, mut version: Option<String>, version_suffix: &String,
                          mut package_type: Option<PackageType>, remote_name: &String, vendor: Option<&String>) -> CommandResult {
        // Check if git and gh is installed.
        let git_installed = std::process::Command::new("git")
            .arg("--version")
            .output()
            .is_ok();
        if !git_installed {
            return Err(GenericError { message: "git is not installed".to_string() });
        }
        let gh_installed = std::process::Command::new("gh")
            .arg("--version")
            .output()
            .is_ok();
        if !gh_installed {
            return Err(GenericError { message: "GitHub CLI client 'gh' is not installed".to_string() });
        }

        if !path.exists() {
            return Err(InvalidArgumentError { message: format!("Path {} does not exist", path.display()) });
        }

        let mut nospub = PublishOptions { globs: vec![] };

        let mut api_version: Option<SemVer> = None;

        // If path is a directory, search for a manifest file
        if path.is_dir() {
            let res = get_plugin_manifest_file(path);
            if res.is_err() {
                return Err(InvalidArgumentError { message: res.err().unwrap() });
            }
            let plugin_manifest_file = res.unwrap();
            let res = get_subsystem_manifest_file(path);
            if res.is_err() {
                return Err(InvalidArgumentError { message: res.err().unwrap() });
            }
            let subsystem_manifest_file = res.unwrap();
            if plugin_manifest_file.is_some() && subsystem_manifest_file.is_some() {
                return Err(InvalidArgumentError { message: format!("Multiple module manifest files found in {}", path.display()) });
            }

            if plugin_manifest_file.is_some() {
                package_type = Some(PackageType::Plugin);
            } else if subsystem_manifest_file.is_some() {
                package_type = Some(PackageType::Subsystem);
            }

            let manifest_file = plugin_manifest_file.or(subsystem_manifest_file);
            if manifest_file.is_some() {
                let package_type = package_type.as_ref().unwrap();
                let manifest_file = manifest_file.unwrap();
                let contents = std::fs::read_to_string(&manifest_file).unwrap();
                let manifest: serde_json::Value = serde_json::from_str(&contents).unwrap();
                name = Some(manifest["info"]["id"]["name"].as_str().unwrap().to_string());
                version = Some(manifest["info"]["id"]["version"].as_str().unwrap().to_string());
                let binary_path = manifest["binary_path"].as_str();
                if binary_path.is_some() {
                    // Binary path is relative to the manifest file
                    let binary_path = manifest_file.parent().unwrap().join(binary_path.unwrap());
                    let binary_path = library_filename(&binary_path);
                    // Load the dynamic library
                    unsafe {
                        let lib = Library::new(&binary_path);
                        if lib.is_err() {
                            return Err(InvalidArgumentError { message: format!("Could not load dynamic library {}: {}", &binary_path.to_str().unwrap(), lib.err().unwrap()) });
                        }
                        let lib = lib.unwrap();
                        let func_name = match package_type {
                            PackageType::Plugin => "nosGetPluginAPIVersion",
                            PackageType::Subsystem => "nosGetSubsystemAPIVersion",
                            _ => panic!("Invalid package type")
                        };
                        let func: libloading::Symbol<unsafe extern "C" fn(*mut i32, *mut i32, *mut i32)> = lib.get(func_name.as_bytes()).unwrap();
                        let mut major = 0;
                        let mut minor = 0;
                        let mut patch = 0;
                        func(&mut major, &mut minor, &mut patch);
                        api_version = Some(SemVer { major: (major as u32), minor: Some(minor as u32), patch: Some(patch as u32), build_number: None });
                        // Unload the library
                        drop(lib);
                        println!("{}", format!("{} uses Nodos {:?} API version: {}.{}.{}", name.as_ref().unwrap(), &package_type, major, minor, patch).as_str().yellow());
                    }
                }
            }

            let mut nospub_file = path.join(constants::PUBLISH_OPTIONS_FILE_NAME);
            if nospub_file.exists() {
                let contents = std::fs::read_to_string(&nospub_file).unwrap();
                nospub = serde_json::from_str(&contents).unwrap();
            }
            else {
                println!("{}", format!("No {} file found in {}. All files will be included in the release.", constants::PUBLISH_OPTIONS_FILE_NAME, path.display()).as_str().yellow());
                nospub.globs.push("**".to_string());
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

        let mut files_to_release = vec![];
        for glob in nospub.globs.iter() {
            let mut walker = globwalk::GlobWalkerBuilder::from_patterns(path, &[glob])
                .build()
                .unwrap();
            for entry in walker {
                let entry = entry.unwrap();
                if entry.file_type().is_dir() {
                    continue;
                }
                files_to_release.push(entry.path().to_path_buf());
            }
        }
        println!("{}", "Following files will be included in the release:".to_string().as_str().yellow());
        for file in files_to_release.iter() {
            println!("\t{}", file.display());
        }

        let workspace = Workspace::get()?;
        println!("{}", "Creating release...".to_string().as_str().yellow());
        let temp_dir = tempdir().unwrap();
        // Zip the files
        let tag = format!("{}-{}", name, version);
        let zip_file_name = format!("{}.zip", tag);
        let zip_file_path = temp_dir.path().join(&zip_file_name);
        let file = File::create(&zip_file_path).unwrap();
        let mut zip = zip::ZipWriter::new(file);

        let options = SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);

        let mut buffer = Vec::new();
        for file_path in files_to_release.iter() {
            let mut file = File::open(file_path).unwrap();
            file.read_to_end(&mut buffer).unwrap();
            zip.start_file(file_path.strip_prefix(path).unwrap().to_str().unwrap(), options).unwrap();
            zip.write_all(&buffer).unwrap();
            buffer.clear();
        }

        zip.finish().unwrap();

        // Create index entry for the release
        let remote = workspace.find_remote(remote_name);
        if remote.is_none() {
            return Err(InvalidArgumentError { message: format!("Remote {} not found", remote_name) });
        }
        let remote = remote.unwrap();

        let release = PackageReleaseEntry {
            version,
            url: format!("{}/releases/download/{}/{}", remote.url, tag, zip_file_name),
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

        let res = remote.fetch_add(&dry_run, &workspace, &name, vendor, &package_type, release);
        if res.is_err() {
            return Err(GenericError { message: res.err().unwrap() });
        }

        let res = remote.create_gh_release(&dry_run, &workspace, &name, &version, vec![zip_file_path]);
        if res.is_err() {
            return Err(GenericError { message: res.err().unwrap() });
        }
        // Remove the temporary directory
        temp_dir.close().unwrap();
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
        self.run_publish(dry_run, &path, name, version, version_suffix, package_type, &remote_name, vendor)
    }
}
