use clap::{Arg, ArgAction, ArgMatches};
use crate::nosman::command::{Command, CommandError, CommandResult};
use crate::nosman::package::LocalPackageEntry;
use crate::nosman::workspace::{Workspace, OutputMode, ScanPackagesFlags};
use std::path::{PathBuf};

pub struct PackageQuery<'a> {
    pub name: &'a str,
    pub version_prefix: &'a str,
}

pub struct InfoCommand {
}

impl InfoCommand {
    pub fn run_get_info(&self, workspace: &mut Workspace, package_query: Option<&PackageQuery>, relaxed: bool, manifest_path: Option<&str>) -> CommandResult {
        // Validate that we have either package query or manifest, but not both
        if package_query.is_some() && manifest_path.is_some() {
            return Err(CommandError::InvalidArgument {
                message: "Cannot use both package/version and --manifest at the same time".to_string()
            });
        }
        
        if package_query.is_none() && manifest_path.is_none() {
            return Err(CommandError::InvalidArgument {
                message: "Either provide package name and version, or use --manifest with a path".to_string()
            });
        }
        
        let mut package = workspace.with_output_mode_scoped(OutputMode::Silent, |ws| {
            Self::get_package(package_query, relaxed, manifest_path, ws)
        })?;
        if package.needs_rescan(workspace, false) {
            package = workspace.with_output_mode_scoped(OutputMode::Silent, |ws| {
                let full_path = ws.root.join(&package.manifest_path.parent().unwrap());
                ws.scan_packages_in_folder(full_path.to_path_buf(), ScanPackagesFlags::all());
                ws.save()?;
                Self::get_package(package_query, relaxed, manifest_path, ws)
            })?;
        }
        // Convert paths to full paths:
        let mut m = package;
        m.manifest_path = workspace.root.join(&m.manifest_path);
        for file in &mut m.type_schema_files {
            *file = workspace.root.join(file.clone());
        }
        if let Some(ref mut folder) = m.public_include_folder {
            *folder = workspace.root.join(folder.clone());
        }
        let json_str = serde_json::to_string_pretty(&m).unwrap();
        println!("{}", json_str);
        Ok(())
    }

    fn get_package(package_query: Option<&PackageQuery>, relaxed: bool, manifest_path: Option<&str>, ws: &mut Workspace) -> Result<LocalPackageEntry, CommandError> {
        Ok(if let Some(manifest_path) = manifest_path {
            // Using manifest path
            let res = ws.get_package_by_path(PathBuf::from(manifest_path));
            if res.is_none() {
                return Err(CommandError::InvalidArgument { message: format!("There is no package in {}", manifest_path) });
            }
            res.unwrap().clone()
        } else if let Some(query) = package_query {
            // Using package name and version prefix
            if relaxed {
                let res = ws.get_latest_local_package_for_version(query.name, query.version_prefix);
                if let Err(msg) = res {
                    return Err(CommandError::InvalidArgument { message: msg });
                }
                res.unwrap().clone()
            } else {
                let res = ws.get_package(query.name, query.version_prefix);
                if res.is_none() {
                    return Err(CommandError::InvalidArgument { message: format!("Package {} version {} is not installed", query.name, query.version_prefix) });
                }
                res.unwrap().clone()
            }
        } else {
            return Err(CommandError::InvalidArgument { 
                message: "Either provide package name and version, or use --manifest with a path".to_string() 
            });
        })
    }
}

pub fn get_cli() -> clap::Command {
    clap::Command::new("info")
        .about("Returns information about an installed package in JSON format.\n\
    If no such package is installed, it will return an error.")
        .arg(Arg::new("package")
            .help("Package name")
            .required(false))
        .arg(Arg::new("version")
            .help("Package version")
            .required(false))
        .arg(Arg::new("relaxed")
            .action(ArgAction::SetTrue)
            .help("If set, version parameter will be interpreted as minimum required version within that minor/patch version.\n\
        It will return information about a version 'x' found among installed packages such that 'a.b <= x < a.(b+1)'.")
            .long("relaxed")
            .num_args(0)
            .required(false)
        )
        .arg(Arg::new("manifest")
            .help("Path to package manifest")
            .long("manifest")
            .required(false)
            .conflicts_with_all(&["package", "version"]))
}

impl Command for InfoCommand {
    fn matched_args<'a>(&self, _workspace: &Workspace, args : &'a ArgMatches) -> Option<&'a ArgMatches> {
        args.subcommand_matches("info")
    }

    fn run(&self, workspace: &mut Workspace, _command_name: Option<&str>, args: &ArgMatches) -> CommandResult {
        let package_name = args.get_one::<String>("package").map(|s| s.as_str());
        let version = args.get_one::<String>("version").map(|s| s.as_str());
        let relaxed = args.get_one::<bool>("relaxed").copied().unwrap_or(false);
        let manifest_path = args.get_one::<String>("manifest").map(|s| s.as_str());
        
        // Validate that we have either (package + version) or manifest, but not both
        let has_package_info = package_name.is_some() || version.is_some();
        let has_manifest = manifest_path.is_some();
        
        if has_package_info && has_manifest {
            return Err(CommandError::InvalidArgument {
                message: "Cannot use both package/version and --manifest at the same time".to_string()
            });
        }
        
        if !has_package_info && !has_manifest {
            return Err(CommandError::InvalidArgument {
                message: "Either provide package name and version, or use --manifest with a path".to_string()
            });
        }
        
        // If using package/version, both must be provided
        if has_package_info && (package_name.is_none() || version.is_none()) {
            return Err(CommandError::InvalidArgument {
                message: "Both package name and version must be provided".to_string()
            });
        }
        
        let package_query = package_name.and_then(|name| version.map(|ver| PackageQuery { name, version_prefix: ver }));
        self.run_get_info(workspace, package_query.as_ref(), relaxed, manifest_path)
    }

    fn needs_workspace(&self) -> bool {
        true
    }
}