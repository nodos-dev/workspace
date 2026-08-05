use std::collections::HashSet;
use std::path::PathBuf;
use std::time::Duration;

use clap::{Arg, ArgAction, ArgMatches};
use colored::Colorize;
use rayon::iter::{IntoParallelRefIterator, ParallelIterator};

use crate::nosman;
use crate::nosman::command::{Command, CommandError, CommandResult};

use zip::result::ZipError;
use nosman::workspace::Workspace;
use crate::nosman::command::CommandError::{Runtime, InvalidArgument};
use crate::nosman::common::get_progress_bar;
use crate::nosman::index::{PackageType, SemVer};
use bitflags::bitflags;
use crate::nosman::package::PackageIdentifier;
use crate::nosman::workspace::ScanPackagesFlags;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallOp {
    Installed,
    Skipped,
}

/// How many artifacts to download at once. Past a handful the network is the limit and the
/// temp folder just fills up.
const MAX_PARALLEL_DOWNLOADS: usize = 8;

/// A package resolved to one exact version, and where its artifact goes.
struct PlannedInstall {
    id: PackageIdentifier,
    package_type: PackageType,
    artifact_id: i64,
    target_dir: PathBuf,
    /// Commands are registered for a package installed together with its dependencies.
    register_commands: bool,
}

impl PlannedInstall {
    fn type_name(&self) -> &'static str {
        if self.package_type.is_plugin() { "plugin" } else { "package" }
    }
}

impl InstallCommand {
    /// Resolves a package and everything it depends on to exact versions, dependencies
    /// first. Nothing is downloaded here; an empty plan means it is all already installed.
    fn plan_install(&self, workspace: &mut Workspace, package_name: &str, version_opt: Option<&String>, output_dir: &Option<PathBuf>, prefix: Option<&String>, flags : InstallFlags) -> Result<Vec<PlannedInstall>, CommandError> {
        let install_with_deps = !flags.contains(InstallFlags::WithoutDependencies);
        let mut exact_no_fetch = flags;
        exact_no_fetch.remove(InstallFlags::UpdatePackageIndex);
        exact_no_fetch.insert(InstallFlags::InstallExactVersion);
        if let Some(version) = version_opt {
            if !flags.contains(InstallFlags::InstallExactVersion) {
                let version_prefix = SemVer::parse_from_str(version.as_str()).unwrap_or_else(|| panic!("Failed to parse semantic version"));
                if workspace.has_local_match(package_name, &version_prefix) {
                    let installed_package = workspace.get_latest_local_package_for_prefix(package_name, &version_prefix)?;
                    println!("{}", format!("Found an already installed compatible version for {} version {}: {}", package_name, version, installed_package.info.id.version).as_str().yellow());
                    return Ok(Vec::new())
                }
            } else if workspace.is_installed(package_name, version.as_str()) {
                let existing = workspace.get_package(package_name, version.as_str())?;
                if existing.get_package_root().exists() {
                    println!("{}", format!("package {} version {} is already installed", package_name, version).as_str().yellow());
                    return Ok(Vec::new());
                }
            }
        }
        // Fetch package metadata from the Nodos Store.
        if flags.contains(InstallFlags::UpdatePackageIndex) {
            println!("Fetching index...");
            workspace.fetch_package_releases(package_name);
            if install_with_deps {
                workspace.fetch_releases(None);
            }
        }
        let version = if let Some(version_val) = version_opt {
            version_val.to_string()
        } else {
            let latest = workspace.index_cache.get_latest_release(package_name);
            if latest.is_none() {
                return Err(InvalidArgument { message: format!("No versions found for package {}", package_name) });
            }
            let ver = latest.unwrap().1.version.clone();
            println!("Installing latest version {} of {}", ver, package_name);
            return self.plan_install(workspace, package_name, Some(&ver), output_dir, prefix, exact_no_fetch);
        };
        if !flags.contains(InstallFlags::InstallExactVersion) {
            // Find or download a version matching the provided prefix
            let version_prefix = SemVer::parse_from_str(version.as_str()).unwrap_or_else(|| panic!("Failed to parse semantic version"));
            println!("Installing {} matching version prefix '{}'", package_name, version_prefix);
            return {
                let latest_compatible_opt = workspace.index_cache.get_latest_compatible_release(package_name, &version_prefix);
                let compatible_package = if let Some((package_type, release)) = latest_compatible_opt {
                    if *package_type == PackageType::Nodos || *package_type == PackageType::Engine {
                        return Err(InvalidArgument { message: format!("Package {} requires special treatment", package_name) });
                    }
                    Some(release.version.clone()) // Clone version to avoid lifetime issues.
                } else {
                    return Err(InvalidArgument { message: format!("Nodos Store does not contain a version matching prefix '{}' for package {}", version_prefix, package_name) });
                };
                self.plan_install(workspace, package_name, compatible_package.as_ref(), output_dir, prefix, exact_no_fetch)
            }
        }
        let Some((package_type, package)) = workspace.index_cache.get_package_cpy(package_name, version.as_str()) else {
            return Err(InvalidArgument { message: format!("Nodos Store does not contain package {} version {}. You can try rescan command to update index.", package_name, version) })
        };

        let mut plans = Vec::new();
        if install_with_deps {
            // Collect dependencies and plan them before the package itself
            let mut remaining = vec![(package_name.to_string(), package.clone())];
            let mut deps_to_install = HashSet::new();
            while let Some((rem_pkg_name, pkg)) = remaining.pop() {
                if pkg.dependencies.is_none() {
                    continue;
                }
                for dep in pkg.dependencies.as_ref().unwrap() {
                    let dep_name = dep.name.clone();
                    let dep_version = dep.version.clone();
                    // Fetch releases for dependencies not in the public index (e.g. private packages)
                    if workspace.index_cache.get_package_releases(&dep_name).is_empty() {
                        workspace.fetch_package_releases(&dep_name);
                    }
                    let res = workspace.get_latest_absent_release_for(&dep_name, &dep_version);
                    if let Err(e) = res {
                        return Err(Runtime { message: format!("\nUnable to satisfy dependency {}\n\tRequested version: {}\n\tRequired by: {}-{}\n\tReason: {}",
                                                              dep_name, dep_version, rem_pkg_name, pkg.version, e) });
                    }
                    let opt_absent_release = res?;
                    if opt_absent_release.is_none() {
                        println!("Dependency {} {} already installed", dep_name, dep_version);
                        continue;
                    }
                    let (_, resolved_dep_pkg) = opt_absent_release.unwrap();
                    let to_install = PackageIdentifier {
                        name: dep_name.clone(),
                        version: resolved_dep_pkg.version.clone(),
                    };
                    if deps_to_install.contains(&to_install) {
                        continue;
                    }
                    deps_to_install.insert(to_install);
                    remaining.push((dep_name.clone(), resolved_dep_pkg.clone()));
                }
            }
            // Since we install it without deps, no need to top-sort it
            let mut dep_install_flags = exact_no_fetch;
            dep_install_flags.insert(InstallFlags::WithoutDependencies);
            for dep in deps_to_install {
                let dep_name = dep.name.clone();
                let dep_version = dep.version.clone();
                if !workspace.is_installed(&dep_name, &dep_version) {
                    println!("Installing dependency {} {}", dep_name, dep_version);
                    plans.extend(self.plan_install(workspace, &dep_name, Some(&dep_version), output_dir, prefix, dep_install_flags)?);
                } else {
                    println!("Dependency {} {} already installed", dep_name, dep_version);
                }
            }
        }
        let output_dir_inferred = if let Some(out_dir) = output_dir {
            out_dir
        } else {
            &if package_type.is_plugin() {
                workspace.root.join("Module/Downloaded") // TODO: Some day this will be Plugin
            } else { // TODO: Engine installations?
                workspace.root.join("Package/Downloaded")
            }
        };
        let mut install_dir = output_dir_inferred.clone();
        if let Some(p) = prefix {
            install_dir = install_dir.join(p);
        } else {
            install_dir = install_dir.join(package_name).join(version.clone());
        }

        let final_out_dir = if install_dir.is_relative() {
            workspace.root.join(install_dir)
        } else {
            install_dir
        };

        let artifact_id = package.artifact_id.ok_or_else(|| CommandError::InvalidArgument {
            message: format!("No artifact ID found for {} version {}", package_name, version),
        })?;
        plans.push(PlannedInstall {
            id: PackageIdentifier { name: package_name.to_string(), version },
            package_type,
            artifact_id,
            target_dir: final_out_dir,
            register_commands: install_with_deps,
        });
        Ok(plans)
    }

    /// Downloads the planned artifacts, several at a time, then registers what landed.
    fn install_planned(&self, workspace: &mut Workspace, plans: &[PlannedInstall]) -> CommandResult {
        let pb = get_progress_bar(workspace.is_silent());
        pb.enable_steady_tick(Duration::from_millis(100));
        {
            // One shared client, so the downloads overlap instead of queueing up.
            let client = workspace.authenticated_store_client()?;
            let pool = rayon::ThreadPoolBuilder::new()
                .num_threads(plans.len().min(MAX_PARALLEL_DOWNLOADS))
                .build()
                .map_err(|e| Runtime { message: format!("Failed to start downloads: {}", e) })?;
            pool.install(|| {
                plans.par_iter().try_for_each(|planned| {
                    pb.println(format!("Downloading {} {}-{}", planned.type_name(), planned.id.name, planned.id.version));
                    pb.set_message(format!("Downloading {}-{}", planned.id.name, planned.id.version));
                    client.install_artifact(planned.artifact_id, &planned.target_dir)
                        .map_err(|e| Runtime { message: format!("Failed to install artifact: {}", e) })
                })
            })?;
        }
        pb.finish_and_clear();

        let mut any_under_workspace = false;
        for planned in plans {
            println!("Extracted {} {} to {}", planned.type_name(), planned.id.name, planned.target_dir.display());
            // If the package is installed under workspace, register it.
            if planned.target_dir.starts_with(&workspace.root) {
                let mut scan_flags = ScanPackagesFlags::ForceReplaceInRegistry;
                if planned.register_commands {
                    scan_flags.insert(ScanPackagesFlags::RegisterCommands);
                }
                workspace.scan_packages_in_folder(planned.target_dir.clone(), scan_flags);
                any_under_workspace = true;
            } else {
                println!("{}", "Note: Package is installed outside the workspace.".yellow());
            }
            println!("{}", format!("{}-{} installed successfully", planned.id.name, planned.id.version).as_str().green());
        }
        if any_under_workspace {
            println!("Adding to workspace file");
            workspace.save()?;
        }
        Ok(())
    }

    pub fn run_install(&self, workspace: &mut Workspace, package_name: &str, version_opt: Option<&String>, output_dir: &Option<PathBuf>, prefix: Option<&String>, flags : InstallFlags) -> Result<InstallOp, CommandError> {
        let plans = self.plan_install(workspace, package_name, version_opt, output_dir, prefix, flags)?;
        if plans.is_empty() {
            return Ok(InstallOp::Skipped);
        }
        self.install_planned(workspace, &plans)?;
        Ok(InstallOp::Installed)
    }

    /// Installs several packages: everything is resolved first, then all of the downloads
    /// run together. A package two of them depend on is only fetched once.
    pub fn run_install_many(&self, workspace: &mut Workspace, packages: &[PackageIdentifier], output_dir: &Option<PathBuf>, prefix: Option<&String>, flags : InstallFlags) -> CommandResult {
        let mut plans: Vec<PlannedInstall> = Vec::new();
        let mut planned_ids = HashSet::new();
        for package in packages {
            for planned in self.plan_install(workspace, &package.name, Some(&package.version), output_dir, prefix, flags)? {
                if planned_ids.insert(planned.id.clone()) {
                    plans.push(planned);
                }
            }
        }
        if plans.is_empty() {
            return Ok(());
        }
        self.install_planned(workspace, &plans)
    }
}

pub fn get_cli() -> clap::Command {
    clap::Command::new("install")
        .about("Install a package")
        .arg(Arg::new("package").required(true))
        .arg(Arg::new("version").required(false))
        .arg(Arg::new("exact")
            .action(ArgAction::SetTrue)
            .help("If not set, version parameter will be interpreted as minimum required version within that minor/patch version.\n\
        If no version 'x' such that 'a.b <= x < a.(b+1)' is found among installed packages, latest such version will be installed.\n\
        If version is set to 'latest' or has no minor component, it will fail.")
            .long("exact")
            .num_args(0)
            .required(false)
        )
        .arg(Arg::new("without_deps")
            .long("without-deps")
            .help("Do not install dependencies of the package")
            .action(ArgAction::SetTrue)
        )
        .arg(Arg::new("prefix")
            .help("Folder path relative to out_dir. The package contents will be under this folder. By default, its '<package_name>/<version>'.")
            .long("prefix")
            .required(false)
        )
        .arg(Arg::new("out_dir")
            .help("The directory where the package will be installed. By default, it is '<package_type>/Downloaded'")
            .long("out-dir")
            .required(false)
        )
}

impl Command for InstallCommand {
    fn matched_args<'a>(&self, _workspace: &Workspace, args : &'a ArgMatches) -> Option<&'a ArgMatches> {
        args.subcommand_matches("install")
    }

    fn run(&self, workspace: &mut Workspace, _command_name: Option<&str>, args: &ArgMatches) -> CommandResult {
        let package_name = args.get_one::<String>("package").unwrap();
        let version = args.get_one::<String>("version");
        let output_dir = args.get_one::<String>("out_dir").map(PathBuf::from);
        let prefix = args.get_one::<String>("prefix");
        let mut flag : InstallFlags = InstallFlags::UpdatePackageIndex;
        if *args.get_one::<bool>("exact").unwrap() {
            flag.insert(InstallFlags::InstallExactVersion);
        }
        if *args.get_one::<bool>("without_deps").unwrap() {
            flag.insert(InstallFlags::WithoutDependencies);
        }
        self.run_install(workspace, package_name, version, &output_dir, prefix, flag)?;
        Ok(())
    }
}
