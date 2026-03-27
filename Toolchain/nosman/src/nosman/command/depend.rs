use std::fs;
use clap::{Arg, ArgAction, ArgMatches};
use serde_json::{json, Value};
use crate::nosman::command::{Command, CommandResult};

use crate::nosman::command::CommandError::InvalidArgument;
use crate::nosman::index::{SemVer};
use crate::nosman::module::{get_dependency_arguments};
use crate::nosman::package::PackageIdentifier;
use crate::nosman::workspace::{Workspace};

pub struct DependCommand {
}
impl DependCommand {
    pub(crate) fn run_depend(&self, workspace: &mut Workspace, package_name: &String, deps: &Vec<PackageIdentifier>) -> CommandResult {
        let package_manifest_path;
        let mut manifest_json;
        {
            let package = workspace.get_or_select_package(package_name)?;
            package_manifest_path = package.manifest_path.clone();
            manifest_json = package.read_manifest();
        }

        let manifest_info = manifest_json
            .get_mut("info")
            .and_then(Value::as_object_mut)
            .unwrap_or_else(|| panic!("Missing 'info' field in package manifest file {}", package_manifest_path.display()));

        let manifest_deps = manifest_info
            .entry("dependencies")
            .or_insert_with(|| json!([])) // Ensure the field exists, defaulting to an empty array
            .as_array_mut()
            .unwrap_or_else(|| panic!("Failed to access 'dependencies' as array: {}", package_manifest_path.display()));

        for dep_id in deps {
            let mut dep = PackageIdentifier {
                name: "".to_string(),
                version: "".to_string(),
            };

            workspace.fetch_package_releases(&dep_id.name);

            if dep_id.version == "any" {
                if let Ok(module) = workspace.get_or_select_package(&dep_id.name) {
                    dep = module.info.id.clone();
                } else if let Some(remote_package) = workspace.index_cache.get_latest_release(&dep_id.name) {
                    dep.name = dep_id.name.clone();
                    dep.version = remote_package.1.version.clone();
                    println!("Found latest version {} for {}", dep.version, dep.name);
                }
            } else if let Ok(module) = workspace.get_latest_local_package_for_version(&dep_id.name, &dep_id.version) {
                dep = module.info.id.clone();
            } else {
                // Convert the `Option` from `parse_from_string` to a `Result` so we can use `map_err`
                let version_prefix = SemVer::parse_from_str(&dep_id.version)
                    .ok_or(InvalidArgument { message: "Invalid version format".to_string() })?;

                if let Some(remote_package) = workspace.index_cache.get_latest_compatible_release(
                    &dep_id.name, &version_prefix) {
                    dep.name = dep_id.name.clone();
                    dep.version = remote_package.1.version.clone();  // Assuming remote_package.1 has a `version` field
                    println!("Found latest version {} for {}", dep.version, dep.name);}
            }

            if dep.name.is_empty() {
                return Err(InvalidArgument { message: format!("Dependency {} not found locally or on the Nodos Store", dep_id.name) });
            }

            // Check if the dependency is already in the manifest
            let mut is_in_manifest = false;
            for mani_dep in manifest_deps.iter_mut() {
                if mani_dep["name"] == dep_id.name {
                    *mani_dep = serde_json::json!({"name": dep.name, "version": dep.version});
                    is_in_manifest = true;
                    break;
                }
            }
            if !is_in_manifest {
                manifest_deps.push(serde_json::json!({"name": dep.name, "version": dep.version}));
            }
        }
        let manifest_str = serde_json::to_string_pretty(&manifest_json).unwrap_or_else(|e| panic!("Failed to serialize manifest {}: {}", package_manifest_path.display(), e));
        fs::write(&package_manifest_path, manifest_str).unwrap_or_else(|e| panic!("Failed to write manifest file {}: {}", package_manifest_path.display(), e));
        Ok(())
    }
}

pub fn get_cli() -> clap::Command {
    clap::Command::new("depend")
        .about("Add dependency to a Nodos package")
        .arg(Arg::new("package")
            .alias("module")
            .required(true)
            .help("Name of the package to add a dependency to.")
        )
        .arg(Arg::new("dependency")
            .help("Dependency to be added. Can be specified multiple times. Version is not required. Format: <package_name>-<version>")
            .required(false)
            .action(ArgAction::Append)
            .num_args(1..)
        )
}

impl Command for DependCommand {
    fn matched_args<'a>(&self, _workspace: &Workspace, args: &'a ArgMatches) -> Option<&'a ArgMatches> {
        args.subcommand_matches("depend")
    }

    fn needs_workspace(&self) -> bool {
        true
    }

    fn run(&self, workspace: &mut Workspace, _command_name: Option<&str>, args: &ArgMatches) -> CommandResult {
        let package_name = args.get_one::<String>("package").unwrap();
        let mut success = false;
        let deps = get_dependency_arguments(args, true, &mut success);
        if !success{
            return Err(InvalidArgument { message: format!("Invalid dependency format") });
        }
        self.run_depend(workspace, package_name, &deps)
    }
}
