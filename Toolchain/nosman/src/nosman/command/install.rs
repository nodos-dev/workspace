use std::{fs, path};
use std::path::Path;

use clap::{ArgMatches};
use colored::Colorize;

use crate::nosman;
use crate::nosman::command::{Command, CommandError, CommandResult};

use zip::result::ZipError;
use zip::ZipArchive;
use nosman::workspace::Workspace;

pub struct InstallCommand {
}

impl From<ZipError> for CommandError {
    fn from(err: ZipError) -> Self {
        CommandError::ZipError { message: format!("{}", err) }
    }
}

impl InstallCommand {
    fn run_install(&self, module_name: &str, version: &str, exact: bool, output_dir: &path::PathBuf) -> CommandResult {
        // Fetch remotes
        let mut workspace = Workspace::get();
        if !exact {
            // Find or download a version such that 'a.b <= x < a.(b+1)'
            let version_semver = nosman::index::SemVer::parse_from_string(version).unwrap();
            if version_semver.minor.is_none() {
                return Err(CommandError::InvalidArgumentError { message: "Please provide a minor version too!".to_string() });
            }
            let version_start = version_semver.clone();
            let version_end = if version_start.patch.is_none() {
                version_start.upper_minor()
            } else {
                version_start.upper_patch()
            };
            return if let Some(installed_module) = workspace.get_latest_installed_module_within_range(module_name, &version_start, &version_end) {
                println!("{}", format!("Found an already installed compatible version for {} version {}: {}", module_name, version, installed_module.info.id.version).as_str().yellow());
                Ok(true)
            } else {
                println!("{}", format!("No compatible installed version found for {} version {}", module_name, version).as_str().yellow());
                if let Some(release) = workspace.index.get_latest_compatible_release_within_range(module_name, &version_start, &version_end) {
                    self.run_install(module_name, &release.version, true, output_dir)
                } else {
                    Err(CommandError::InvalidArgumentError { message: format!("No compatible version found for {} version {}", module_name, version) })
                }
            }
        }
        let mut replace_entry_in_index = false;
        if let Some(existing) = workspace.get_installed_module(module_name, version) {
            if existing.get_module_dir().exists() {
                println!("{}", format!("Module {} version {} is already installed", module_name, version).as_str().yellow());
                return Ok(true);
            }
            else {
                replace_entry_in_index = true;
            }
        }
        if let Some(module) = workspace.index.get_module(module_name, version) {
            let module_dir = workspace.root.join(output_dir);
            let mut tmpfile = tempfile::tempfile().unwrap();

            println!("Downloading module {} version {}", module_name, version);
            reqwest::blocking::get(&module.url)
                .expect(format!("Failed to fetch module {}", module_name).as_str()).copy_to(&mut tmpfile)
                .expect(format!("Failed to write to tempfile for module {}", module_name).as_str());

            println!("Extracting module {} to {}", module_name, module_dir.display());
            let mut archive = ZipArchive::new(tmpfile)?;
            fs::create_dir_all(module_dir.clone())?;
            for i in 0..archive.len() {
                let mut file = archive.by_index(i)?;
                let outpath = Path::new(&module_dir.clone()).join(file.name());

                if file.is_dir() {
                    fs::create_dir_all(&outpath)?;
                } else {
                    if let Some(parent) = outpath.parent() {
                        if !parent.exists() {
                            fs::create_dir_all(parent)?;
                        }
                    }
                    let mut outfile = fs::File::create(&outpath)?;
                    std::io::copy(&mut file, &mut outfile)?;
                }
            }

            workspace.scan_folder(module_dir, replace_entry_in_index);

            println!("Adding to workspace file");
            workspace.save()?;
            println!("{}", format!("Module {} version {} installed successfully", module_name, version).as_str().green());
            Ok(true)
        } else {
            return Err(CommandError::InvalidArgumentError { message: format!("None of the remotes contain module {} version {}. You can try rescan command to update index.", module_name, version) });
        }
    }
}

impl Command for InstallCommand {
    fn matched_args<'a>(&self, args : &'a ArgMatches) -> Option<&'a ArgMatches> {
        args.subcommand_matches("install")
    }

    fn run(&self, args: &ArgMatches) -> CommandResult {
        let module_name = args.get_one::<String>("module").unwrap();
        let version = args.get_one::<String>("version").unwrap();
        let mut output_dir = args.get_one::<String>("out_dir").map(|p| path::PathBuf::from(p)).unwrap_or_else(|| path::PathBuf::from("."));
        if let Some(prefix) = args.get_one::<String>("prefix") {
            output_dir = output_dir.join(prefix);
        }
        else {
            output_dir = output_dir.join(format!("{}-{}", module_name, version));
        }
        let exact = args.get_one::<bool>("exact").unwrap().clone();
        self.run_install(module_name, version, exact, &output_dir)
    }
}
