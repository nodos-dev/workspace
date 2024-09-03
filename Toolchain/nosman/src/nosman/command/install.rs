use std::path::PathBuf;

use clap::{ArgMatches};
use colored::Colorize;

use crate::nosman;
use crate::nosman::command::{Command, CommandError, CommandResult};

use zip::result::ZipError;
use nosman::workspace::Workspace;
use crate::nosman::command::CommandError::{GenericError, InvalidArgumentError};
use crate::nosman::index::{PackageType, SemVer};
use crate::nosman::common::download_and_extract;

pub struct InstallCommand {
}

impl From<ZipError> for CommandError {
    fn from(err: ZipError) -> Self {
        CommandError::ZipError { message: format!("{}", err) }
    }
}

impl InstallCommand {
    pub(crate) fn run_install(&self, package_name: &str, version_opt: Option<&String>, exact: bool, output_dir: &PathBuf, prefix: Option<&String>) -> CommandResult {
        // Fetch remotes
        let mut workspace = Workspace::get()?;
        let version;
        if version_opt.is_none() {
            workspace.fetch_package_releases(package_name);
            let latest = workspace.index_cache.get_latest_release(package_name);
            if latest.is_none() {
                return Err(InvalidArgumentError { message: format!("No versions found for package {}", package_name) });
            }
            version = latest.unwrap().1.version.clone();
        } else {
            version = version_opt.unwrap().to_string();
        }
        if !exact {
            // Find or download a version such that 'a.b <= x < a.(b+1)'
            let version_start = SemVer::parse_from_string(version.as_str()).unwrap();
            if version_start.minor.is_none() {
                return Err(InvalidArgumentError { message: "Please provide a minor version too!".to_string() });
            }
            let version_end = version_start.get_one_up();
            return if let Some(installed_module) = workspace.get_latest_installed_module_within_range(package_name, &version_start, &version_end) {
                println!("{}", format!("Found an already installed compatible version for {} version {}: {}", package_name, version, installed_module.info.id.version).as_str().yellow());
                Ok(true)
            } else {
                if let Some((package_type, release)) = workspace.index_cache.get_latest_compatible_release_within_range(package_name, &version_start, &version_end) {
                    if *package_type == PackageType::Nodos || *package_type == PackageType::Engine {
                        return Err(InvalidArgumentError { message: format!("Package {} requires special treatment", package_name) });
                    }
                    self.run_install(package_name, Some(&release.version), true, output_dir, prefix)
                } else {
                    Err(InvalidArgumentError { message: format!("No remote contained a version in range [{}, {}) for module {}", version_start.to_string(), version_end.to_string(), package_name) })
                }
            }
        }
        let mut replace_entry_in_index = false;
        if let Some(existing) = workspace.get_installed_module(package_name, version.as_str()) {
            if existing.get_module_dir().exists() {
                println!("{}", format!("Module {} version {} is already installed", package_name, version).as_str().yellow());
                return Ok(true);
            }
            else {
                replace_entry_in_index = true;
            }
        }
        if let Some((package_type, package)) = workspace.index_cache.get_package(package_name, version.as_str()) {
            let mut install_dir = output_dir.clone();
            if let Some(p) = prefix {
                install_dir = install_dir.join(p);
            } else if package_type.is_module() {
                install_dir = install_dir.join(format!("{}-{}", package_name, version));
            }

            let pkg_type_str = if package_type.is_module() { "module" } else { "package" };

            let final_out_dir = if install_dir.is_relative() && package_type.is_module() { workspace.root.join(install_dir) } else { install_dir };
            let module_name_version = format!("{}-{}", package_name, version);
            println!("Downloading {} {}", pkg_type_str, module_name_version);

            let res = download_and_extract(&package.url, &final_out_dir);
            if res.is_err() {
                return Err(res.err().unwrap());
            }
            println!("Extracted {} {} to {}", pkg_type_str, package_name, final_out_dir.display());
            if package_type.is_module() {
                workspace.scan_modules_in_folder(final_out_dir, replace_entry_in_index);
                println!("Adding to workspace file");
                workspace.save()?;
            }
            println!("{}", format!("{}-{} installed successfully", package_name, version).as_str().green());
            Ok(true)
        } else {
            return Err(GenericError { message: format!("None of the remotes contain package {} version {}. You can try rescan command to update index.", package_name, version) });
        }
    }
}

impl Command for InstallCommand {
    fn matched_args<'a>(&self, args : &'a ArgMatches) -> Option<&'a ArgMatches> {
        args.subcommand_matches("install")
    }

    fn run(&self, args: &ArgMatches) -> CommandResult {
        let module_name = args.get_one::<String>("module").unwrap();
        let version = args.get_one::<String>("version");
        let output_dir = args.get_one::<String>("out_dir").map(|p| PathBuf::from(p)).unwrap_or_else(|| PathBuf::from("."));
        let prefix = args.get_one::<String>("prefix");
        let exact = args.get_one::<bool>("exact").unwrap().clone();
        self.run_install(module_name, version, exact, &output_dir, prefix)
    }
}
