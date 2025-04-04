use std::collections::HashSet;
use std::path::PathBuf;

use clap::{ArgMatches};
use colored::Colorize;

use crate::nosman;
use crate::nosman::command::{Command, CommandError, CommandResult};

use zip::result::ZipError;
use nosman::workspace::Workspace;
use crate::nosman::command::CommandError::{Runtime, InvalidArgument};
use crate::nosman::index::{PackageType, SemVer};
use crate::nosman::common::download_and_extract;
use bitflags::bitflags;
use crate::nosman::module::PackageIdentifier;
use crate::nosman::workspace::ScanModulesFlags;

pub struct InstallCommand {
}

impl From<ZipError> for CommandError {
    fn from(err: ZipError) -> Self {
        CommandError::Zip { message: format!("{}", err) }
    }
}

#[derive(Clone, Copy)]
pub struct InstallFlags(u8);
bitflags! {
    impl InstallFlags: u8 {
        const UpdatePackageIndex = 0b1;
        const WithoutDependencies = 0b10;
        const InstallExactVersion = 0b100;
    }
}

impl InstallCommand {
    pub(crate) fn run_install(&self, workspace: &mut Workspace, package_name: &str, version_opt: Option<&String>, output_dir: &PathBuf, prefix: Option<&String>, flags : InstallFlags) -> CommandResult {
        let install_with_deps = !flags.contains(InstallFlags::WithoutDependencies);
        let mut exact_no_fetch = flags;
        exact_no_fetch.remove(InstallFlags::UpdatePackageIndex);
        exact_no_fetch.insert(InstallFlags::InstallExactVersion);
        // Fetch remotes
        if flags.contains(InstallFlags::UpdatePackageIndex) {
            println!("Fetching index...");
            workspace.fetch_package_releases(package_name);
            if install_with_deps {
                workspace.fetch_releases(None);
            }
        }
        let version;
        if version_opt.is_none() {
            let latest = workspace.index_cache.get_latest_release(package_name);
            if latest.is_none() {
                return Err(InvalidArgument { message: format!("No versions found for package {}", package_name) });
            }
            version = latest.unwrap().1.version.clone();
            println!("Installing latest version {} of {}", version, package_name);
            return self.run_install(workspace, package_name, Some(&version), output_dir, prefix, exact_no_fetch);
        } else {
            version = version_opt.unwrap().to_string();
        }
        if !flags.contains(InstallFlags::InstallExactVersion) {
            // Find or download a version such that 'a.b <= x < a.(b+1)'
            let version_start = SemVer::parse_from_string(version.as_str()).unwrap_or_else(|| panic!("Failed to parse semantic version"));
            if version_start.minor.is_none() {
                return Err(InvalidArgument { message: "Please provide a minor version too!".to_string() });
            }
            let version_end = version_start.get_one_up();
            println!("Installing {} with a version in range [{}, {})", package_name, version_start.to_string(), version_end.to_string());
            return if let Some(installed_module) = workspace.get_latest_installed_module_within_range(package_name, &version_start, &version_end) {
                println!("{}", format!("Found an already installed compatible version for {} version {}: {}", package_name, version, installed_module.info.id.version).as_str().yellow());
                Ok(())
            } else {
                let latest_compatible_opt = workspace.index_cache.get_latest_compatible_release_within_range(package_name, &version_start, &version_end);
                let compatible_package = if let Some((package_type, release)) = latest_compatible_opt {
                    if *package_type == PackageType::Nodos || *package_type == PackageType::Engine {
                        return Err(InvalidArgument { message: format!("Package {} requires special treatment", package_name) });
                    }
                    Some(release.version.clone()) // Clone version to avoid lifetime issues.
                } else {
                    return Err(InvalidArgument { message: format!("No remote contained a version in range [{}, {}) for module {}", version_start.to_string(), version_end.to_string(), package_name) });
                };
                self.run_install(workspace, package_name, compatible_package.as_ref(), output_dir, prefix, exact_no_fetch)
            }
        }
        let mut replace_entry_in_index = false;
        if let Some(existing) = workspace.get_installed_module(package_name, version.as_str()) {
            if existing.get_module_dir().exists() {
                println!("{}", format!("Module {} version {} is already installed", package_name, version).as_str().yellow());
                return Ok(());
            } else {
                replace_entry_in_index = true;
            }
        }
        let Some((package_type, package)) = workspace.index_cache.get_package_cpy(package_name, version.as_str()) else {
            return Err(InvalidArgument { message: format!("None of the remotes contain package {} version {}. You can try rescan command to update index.", package_name, version) })
        };

        // Now, we actually install this package.
        if install_with_deps {
            // Collect dependencies and install them
            let mut remaining = vec![(package_name.to_string(), package.clone())];
            let mut deps_to_install = HashSet::new();
            while let Some((rem_pkg_name, pkg)) = remaining.pop() {
                if pkg.dependencies.is_none() {
                    continue;
                }
                for dep in package.dependencies.as_ref().unwrap() {
                    let res = workspace.get_latest_absent_release_for(&dep.name, &dep.version);
                    if let Err(e) = res {
                        return Err(Runtime { message: format!("\nUnable to satisfy dependency {}\n\tRequested version: {}\n\tRequired by: {}-{}\n\tReason: {}",
                                                              dep.name, dep.version, rem_pkg_name, pkg.version, e) });
                    }
                    let opt_absent_release = res?;
                    if opt_absent_release.is_none() {
                        println!("Dependency {} {} already installed", dep.name, dep.version);
                        continue;
                    }
                    let (_, resolved_dep_pkg) = opt_absent_release.unwrap();
                    let to_install = PackageIdentifier {
                        name: dep.name.clone(),
                        version: resolved_dep_pkg.version.clone(),
                    };
                    if deps_to_install.contains(&to_install) {
                        continue;
                    }
                    deps_to_install.insert(to_install);
                    remaining.push((dep.name.clone(), resolved_dep_pkg.clone()));
                }
            }
            // Since we install it without deps, no need to top-sort it
            let mut dep_install_flags = exact_no_fetch;
            dep_install_flags.insert(InstallFlags::WithoutDependencies);
            for dep in deps_to_install {
                let dep_name = dep.name.clone();
                let dep_version = dep.version.clone();
                if workspace.get_installed_module(&dep_name, &dep_version).is_none() {
                    println!("Installing dependency {} {}", dep_name, dep_version);
                    self.run_install(workspace, &dep_name, Some(&dep_version), output_dir, prefix, dep_install_flags)?;
                } else {
                    println!("Dependency {} {} already installed", dep_name, dep_version);
                }
            }
        }
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

        download_and_extract(&package.url, &final_out_dir)?;

        println!("Extracted {} {} to {}", pkg_type_str, package_name, final_out_dir.display());
        if package_type.is_module() {
            let mut scan_flags = ScanModulesFlags::empty();
            if install_with_deps {
                scan_flags.insert(ScanModulesFlags::RegisterCommands);
            }
            if replace_entry_in_index {
                scan_flags.insert(ScanModulesFlags::ForceReplaceInRegistry);
            }
            workspace.scan_modules_in_folder(final_out_dir, scan_flags);
            println!("Adding to workspace file");
            workspace.save()?;
        }
        println!("{}", format!("{}-{} installed successfully", package_name, version).as_str().green());
        Ok(())
    }
}

impl Command for InstallCommand {
    fn matched_args<'a>(&self, _workspace: &Workspace, args : &'a ArgMatches) -> Option<&'a ArgMatches> {
        args.subcommand_matches("install")
    }

    fn run(&self, workspace: &mut Workspace, _command_name: Option<&str>, args: &ArgMatches) -> CommandResult {
        let module_name = args.get_one::<String>("module").unwrap();
        let version = args.get_one::<String>("version");
        let output_dir = args.get_one::<String>("out_dir").map(|p| PathBuf::from(p)).unwrap_or_else(|| PathBuf::from("."));
        let prefix = args.get_one::<String>("prefix");
        let mut flag : InstallFlags = InstallFlags::UpdatePackageIndex;
        if *args.get_one::<bool>("exact").unwrap() {
            flag.insert(InstallFlags::InstallExactVersion);
        }
        if *args.get_one::<bool>("without-deps").unwrap() {
            flag.insert(InstallFlags::WithoutDependencies);
        }
        self.run_install(workspace, module_name, version, &output_dir, prefix, flag)
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use super::*;
    use crate::nosman::workspace::Workspace;
    use std::path::PathBuf;
    use log::info;
    use crate::nosman::module::PackageIdentifier;

    struct WorkspaceGuard {
        workspace: Workspace,
    }
    
    impl WorkspaceGuard {
        fn new(path: &str) -> Self {
            let mut ws = Workspace::from_root(&PathBuf::from(format!("./test_workspaces/{}", path)));
            if !ws.ready() {
                ws.recreate().expect("Failed to create test workspace");
            }
            WorkspaceGuard { workspace: ws }
        }
        fn new_random() -> Self {
            let random_string: String = (0..8)
                .map(|_| rand::random::<u8>() % 26 + b'a')
                .map(char::from)
                .collect();
            WorkspaceGuard::new(random_string.as_str())
        }
    }

    impl Drop for WorkspaceGuard {
        fn drop(&mut self) {
            info!("Cleaning up test workspace {}", self.workspace.root.display());
            fs::remove_dir_all(&self.workspace.root).expect("Unable to remove test workspace");
        }
    }

    #[test]
    fn install_no_deps() {
        let mut test = WorkspaceGuard::new_random();
        let package_name = "nos.sys.vulkan";
        let version = String::from("6.20.0.b616");
        InstallCommand{}.run_install(&mut test.workspace, package_name, Some(&version), &PathBuf::from("."), None, InstallFlags::UpdatePackageIndex | InstallFlags::WithoutDependencies)
            .unwrap_or_else(|_| panic!("Failed to install {}", package_name));
        let versions = test.workspace.get_installed_modules(package_name);
        assert_eq!(versions.len(), 1);
    }


    #[test]
    fn install_brings_dependencies() {
        let mut test = WorkspaceGuard::new_random();
        let package_name = "nos.sys.vulkan";
        let version = String::from("6.20.0.b616");
        test.workspace.fetch_package_releases(package_name);
        let index_entry = test.workspace.index_cache.get_package(package_name, version.as_str());
        assert!(index_entry.is_some());
        let index_entry = index_entry.unwrap().1;
        let mut requested_modules = index_entry.dependencies.clone().expect("No dependencies found");
        requested_modules.push(PackageIdentifier {
            name: package_name.to_string(),
            version: version.clone(),
        });
        InstallCommand{}.run_install(&mut test.workspace, package_name, Some(&version), &PathBuf::from("."), None, InstallFlags::UpdatePackageIndex)
            .unwrap_or_else(|_| panic!("Failed to install {}", package_name));
        assert_eq!(requested_modules.len(), test.workspace.get_installed_module_count());
        for requested in requested_modules {
            let installed = test.workspace.get_installed_modules(requested.name.as_str());
            assert_eq!(installed.len(), 1);
            let installed = installed[0].info.clone();
            let requested_version = SemVer::parse_from_string(requested.version.as_str()).expect("Failed to parse semantic version");
            let installed_version = SemVer::parse_from_string(installed.id.version.as_str()).expect("Failed to parse semantic version");
            assert_eq!(installed.id.name, requested.name);
            assert!(installed_version.satisfies_requested_version(&requested_version));
        }
    }
}