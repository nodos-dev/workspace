use clap::{Arg, ArgAction, ArgMatches};
use crate::nosman::command::{Command, CommandError, CommandResult};
use crate::nosman::package::LocalPackageEntry;
use crate::nosman::workspace::{Workspace, OutputMode, ScanPackagesFlags};
use std::path::{PathBuf};

pub struct InfoCommand {
}

impl InfoCommand {
    fn run_get_info(&self, workspace: &mut Workspace, package_name: &str, version: &str, relaxed: bool, path: &str) -> CommandResult {
        let mut package = workspace.with_output_mode_scoped(OutputMode::Silent, |ws| {
            Self::get_package(package_name, version, relaxed, path, ws)
        })?;
        if package.needs_rescan(workspace, false) {
            package = workspace.with_output_mode_scoped(OutputMode::Silent, |ws| {
                let full_path = ws.root.join(&package.manifest_path.parent().unwrap());
                ws.scan_packages_in_folder(full_path.to_path_buf(), ScanPackagesFlags::all());
                ws.save()?;
                Self::get_package(package_name, version, relaxed, path, ws)
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

    fn get_package(package_name: &str, version: &str, relaxed: bool, path: &str, ws: &mut Workspace) -> Result<LocalPackageEntry, CommandError> {
        Ok(if relaxed {
                let res = ws.get_latest_local_package_for_version(package_name, version);
                if let Err(msg) = res {
                    return Err(CommandError::InvalidArgument { message: msg });
                }
            res.unwrap().clone()
            } else {
        
            if !path.is_empty() {
                let res = ws.get_package_by_path(PathBuf::from(path));
                if res.is_none() {
                    return Err(CommandError::InvalidArgument { message: format!("There is no package in {}", path) });
                }
                res.unwrap().clone()
            }
            else{
                let res = ws.get_package(package_name, version);
                if res.is_none() {
                    return Err(CommandError::InvalidArgument { message: format!("Package {} version {} is not installed", package_name, version) });
                }
                res.unwrap().clone()
            }
        })
    }
}

pub fn get_cli() -> clap::Command {
    clap::Command::new("info")
        .about("Returns information about an installed package in JSON format.\n\
    If no such package is installed, it will return an error.")
        .arg(Arg::new("package").required(false).default_value(""))
        .arg(Arg::new("version").required(false).default_value(""))
        .arg(Arg::new("relaxed")
            .action(ArgAction::SetTrue)
            .help("If set, version parameter will be interpreted as minimum required version within that minor/patch version.\n\
        It will return information about a version 'x' found among installed packages such that 'a.b <= x < a.(b+1)'.")
            .long("relaxed")
            .num_args(0)
            .required(false)
        )
        .arg(Arg::new("path").required(false).default_value(""))
}

impl Command for InfoCommand {
    fn matched_args<'a>(&self, _workspace: &Workspace, args : &'a ArgMatches) -> Option<&'a ArgMatches> {
        args.subcommand_matches("info")
    }

    fn run(&self, workspace: &mut Workspace, _command_name: Option<&str>, args: &ArgMatches) -> CommandResult {
        let package_name = args.get_one::<String>("package").unwrap();
        let version = args.get_one::<String>("version").unwrap();
        let relaxed = args.get_one::<bool>("relaxed").unwrap();
        let path = args.get_one::<String>("path").unwrap();
        self.run_get_info(workspace, package_name, version, *relaxed, path)
    }

    fn needs_workspace(&self) -> bool {
        true
    }
}