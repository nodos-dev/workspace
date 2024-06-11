use std::path::PathBuf;

use clap::{ArgMatches};
use colored::Colorize;

use crate::nosman;
use crate::nosman::command::{Command, CommandError, CommandResult};

use zip::result::ZipError;
use nosman::workspace::Workspace;
use crate::nosman::command::CommandError::{GenericError, InvalidArgumentError};
use crate::nosman::index::{PackageType, SemVer};
use crate::nosman::util::download_and_extract;

pub struct InstallCommand {
}

impl From<ZipError> for CommandError {
    fn from(err: ZipError) -> Self {
        CommandError::ZipError { message: format!("{}", err) }
    }
}

impl InstallCommand {
    fn run_install(&self, module_name: &str, version: &str, exact: bool, output_dir: &PathBuf, prefix: Option<&String>) -> CommandResult {
        // Fetch remotes
        let mut workspace = Workspace::get()?;
        if !exact {
            // Find or download a version such that 'a.b <= x < a.(b+1)'
            let version_start = SemVer::parse_from_string(version).unwrap();
            if version_start.minor.is_none() {
                return Err(InvalidArgumentError { message: "Please provide a minor version too!".to_string() });
            }
            let version_end = version_start.get_one_up();
            return if let Some(installed_module) = workspace.get_latest_installed_module_within_range(module_name, &version_start, &version_end) {
                println!("{}", format!("Found an already installed compatible version for {} version {}: {}", module_name, version, installed_module.info.id.version).as_str().yellow());
                Ok(true)
            } else {
                println!("{}", format!("No installed version in range [{}, {}) for module {}", version_start.to_string(), version_end.to_string(), module_name).as_str().yellow());
                if let Some((package_type, release)) = workspace.index_cache.get_latest_compatible_release_within_range(module_name, &version_start, &version_end) {
                    if *package_type != PackageType::Plugin && *package_type != PackageType::Subsystem {
                        return Err(InvalidArgumentError { message: format!("Package {} found in the index is not a plugin or subsystem", module_name) });
                    }
                    self.run_install(module_name, &release.version, true, output_dir, prefix)
                } else {
                    Err(InvalidArgumentError { message: format!("No remote contained a version in range [{}, {}) for module {}", version_start.to_string(), version_end.to_string(), module_name) })
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
        let mut install_dir = output_dir.clone();
        if let Some(p) = prefix {
            install_dir = install_dir.join(p);
        } else {
            install_dir = install_dir.join(format!("{}-{}", module_name, version));
        }
        if let Some(module) = workspace.index_cache.get_module(module_name, version) {
            let module_dir = if install_dir.is_relative() { workspace.root.join(install_dir) } else { install_dir };
            let module_name_version = format!("{}-{}", module_name, version);
            println!("Downloading module {}", module_name_version);

            let res = download_and_extract(&module.url, &module_dir);
            if res.is_err() {
                return Err(res.err().unwrap());
            }
            println!("Extracted module {} to {}", module_name, module_dir.display());
            workspace.scan_modules_in_folder(module_dir, replace_entry_in_index);

            println!("Adding to workspace file");
            workspace.save()?;
            println!("{}", format!("Module {} version {} installed successfully", module_name, version).as_str().green());
            Ok(true)
        } else {
            return Err(GenericError { message: format!("None of the remotes contain module {} version {}. You can try rescan command to update index.", module_name, version) });
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
        let output_dir = args.get_one::<String>("out_dir").map(|p| PathBuf::from(p)).unwrap_or_else(|| PathBuf::from("."));
        let prefix = args.get_one::<String>("prefix");
        let exact = args.get_one::<bool>("exact").unwrap().clone();
        self.run_install(module_name, version, exact, &output_dir, prefix)
    }
}
