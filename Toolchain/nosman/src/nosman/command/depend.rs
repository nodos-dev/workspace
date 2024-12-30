use std::fs;
use clap::{ArgMatches};
use serde_json::{json, Value};
use crate::nosman::command::{Command, CommandResult};

use crate::nosman::command::CommandError::InvalidArgument;
use crate::nosman::index::{SemVer};
use crate::nosman::module::{get_dependency_arguments, PackageIdentifier};
use crate::nosman::workspace::{Workspace};
pub struct DependsCommands{
}
impl DependsCommands {
    pub(crate) fn run_depends(&self, workspace: &mut Workspace, module_name: &String, deps: &Vec<PackageIdentifier>) -> CommandResult {
        let module_manifest_path;
        let mut manifest_json;
        {
            let module = workspace.select_installed_module(module_name)?;
            module_manifest_path = module.manifest_path.clone();
            manifest_json = module.read_manifest().expect("Failed to read module manifest file");
        }

        let manifest_info = manifest_json
            .get_mut("info")
            .and_then(Value::as_object_mut)
            .expect("Missing 'info' field in module manifest file");

        let manifest_deps = manifest_info
            .entry("dependencies")
            .or_insert_with(|| json!([])) // Ensure the field exists, defaulting to an empty array
            .as_array_mut()
            .expect("Failed to access 'dependencies' as array");

        for dep_id in deps {
            let mut dep = PackageIdentifier {
                name: "".to_string(),
                version: "".to_string(),
            };

            workspace.fetch_package_releases(&dep_id.name);

            if dep_id.version == "any" {
                if let Ok(module) = workspace.select_installed_module(&dep_id.name) {
                    dep = module.info.id.clone();
                } else if let Some(remote_package) = workspace.index_cache.get_latest_release(&dep_id.name) {
                    dep.name = dep_id.name.clone();
                    dep.version = remote_package.1.version.clone();
                    eprintln!("Found latest version {} for {}", dep.version, dep.name);
                }
            } else {
                if let Ok(module) = workspace.get_latest_installed_module_for_version(&dep_id.name, &dep_id.version) {
                    dep = module.info.id.clone();
                } else {
                    // Convert the `Option` from `parse_from_string` to a `Result` so we can use `map_err`
                    let version_start = SemVer::parse_from_string(&dep_id.version)
                        .ok_or(InvalidArgument { message: "Invalid version format".to_string() })?;

                    if version_start.minor.is_none() {
                        return Err(InvalidArgument { message: "Please provide a minor version too!".to_string() });
                    }

                    let version_end = version_start.get_one_up();
                    if let Some(remote_package) = workspace.index_cache.get_latest_compatible_release_within_range(
                        &dep_id.name, &version_start, &version_end
                    ) {
                        dep.name = dep_id.name.clone();
                        dep.version = remote_package.1.version.clone();  // Assuming remote_package.1 has a `version` field
                        eprintln!("Found latest version {} for {}", dep.version, dep.name);}
                }
            }

            if dep.name.is_empty() {
                return Err(InvalidArgument { message: format!("Dependency {} not found neither in local nor remotes", dep_id.name) });
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
        let manifest_str = serde_json::to_string_pretty(&manifest_json).expect("Failed to serialize manifest");
        fs::write(&module_manifest_path, manifest_str).expect("Failed to write manifest file");
        Ok(true)
    }
}

impl Command for DependsCommands {
    fn matched_args<'a>(&self, _workspace: &Workspace, args: &'a ArgMatches) -> Option<&'a ArgMatches> {
        args.subcommand_matches("depend")
    }

    fn needs_workspace(&self) -> bool {
        true
    }

    fn run(&self, workspace: &mut Workspace, _command_name: Option<&str>, args: &ArgMatches) -> CommandResult {
        let module_name = args.get_one::<String>("module").unwrap();
        let mut success = false;
        let deps = get_dependency_arguments(args, true, &mut success);
        if !success{
            return Err(InvalidArgument { message: format!("Invalid dependency format") });
        }
        self.run_depends(workspace, module_name, &deps)
    }
}
