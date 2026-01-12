use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;
use clap::{Arg, ArgAction, ArgMatches};
use colored::Colorize;
use crate::nosman::common::copy_include_dir_recursive;
use crate::nosman::command::{get_lang_tool_arg, get_nodos_version_from_args, Command, CommandResult};
use crate::nosman::command::CommandError::InvalidArgument;
use crate::nosman::index::{PluginType, SemVer};
use include_dir::{include_dir, Dir};
use crate::nosman::command::sdk_info::get_engine_sdk_infos;
use crate::nosman::common::{DEFAULT_NODOS_VERSION_INDEX, SUPPORTED_NODOS_VERSIONS};
use crate::nosman::module::get_dependency_arguments;
use crate::nosman::package::{get_plugin_manifest_file_ext, PackageIdentifier};
use crate::nosman::workspace::{ScanPackagesFlags, Workspace};

pub struct CreateCommand {}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum LangTool {
    CppCMake,
}

impl std::fmt::Display for LangTool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LangTool::CppCMake => write!(f, "cpp/cmake"),
        }
    }
}

impl LangTool {
    fn lang(&self) -> &'static str {
        match self {
            LangTool::CppCMake => "cpp",
        }
    }
    fn tool(&self) -> &'static str {
        match self {
            LangTool::CppCMake => "cmake",
        }
    }
}

static DATA_DIR: Dir = include_dir!("$CARGO_MANIFEST_DIR/data");

fn get_template_dir_for<'a>(name: &str, plugin_type: &PluginType, version: &str) -> Option<&'a Dir<'a>> {
    let template_dir = if *plugin_type == PluginType::Default {
        DATA_DIR.get_dir(format!("templates/nodos-{}/{}/plugin", version, name))
    } else {
        DATA_DIR.get_dir(format!("templates/nodos-{}/{}/subsystem", version, name))
    };
    template_dir
}

impl CreateCommand {
    fn replace_lang_placeholders(_content: &mut String, lang: &str) {
        if lang == "cpp" {
            // If any placeholders are added in the future
        }
    }

    fn replace_tool_placeholders(workspace: &Workspace, content: &mut String, module_name: &str, deps: &Vec<PackageIdentifier>, tool: &str) {
        let mut nos_version = None;
        if let Ok(engines) = get_engine_sdk_infos(workspace) {
            for engine in engines {
                if let Some(cur_ver) = SemVer::parse_from_str(engine.version.as_str()) {
                    if nos_version.is_none() || cur_ver > *nos_version.as_ref().unwrap() {
                        nos_version = Some(cur_ver);   
                    }
                }
            }
        }
        if nos_version.is_none() {
            nos_version = Some(SemVer::new(1, Some(3), Some(0), None));
        }
        let nos_version = nos_version.unwrap();
        if tool == "cmake" {
            *content = content
                .replace("<CMAKE_PROJECT_NAME>", module_name)
                .replace("<CMAKE_LATEST_NOS_VERSION>", nos_version.to_string().as_str())
                .replace("<CMAKE_MODULE_DEPENDENCIES>", &deps.iter().map(|dep| {
                    format!("\"{}-{}\"", dep.name, dep.version)
                }).collect::<Vec<_>>().join(" "));
        }
    }

    pub fn run_create(&self, workspace: &mut Workspace, plugin_name: &str, plugin_type: PluginType, lang_tool: LangTool,
                      output_dir: &PathBuf, deps: Vec<PackageIdentifier>, description: &str, nodos_version: Option<SemVer>) -> CommandResult {
        println!("{}", format!("Creating a new Nodos plugin project of type '{:?}'", plugin_type).green());

        // Check module name contains at least one namespace
        if plugin_name.split('.').count() < 2 {
            return Err(InvalidArgument { message: "Plugin name must contain a company/organization prefix".to_string() });
        }

        let mut selected_version: Option<SemVer> = None;
        if let Some(version) = nodos_version {
            selected_version = Some(version.clone());
        } else if workspace.ready() {
            let engines = get_engine_sdk_infos(workspace);
            if let Ok(engines) = engines {
                let mut major_minors = HashSet::<SemVer>::new();
                for engine in engines {
                    if let Some(mut semver) = SemVer::parse_from_str(engine.version.as_str()) {
                        semver.patch = None;
                        semver.build_number = None;
                        major_minors.insert(semver);
                    }
                }
                if major_minors.len() > 1 {
                    // Multiple versions found, select the latest one:
                    let mut versions: Vec<SemVer> = major_minors.into_iter().collect();
                    versions.sort_by(|a, b| {
                        b.cmp(&a) // Descending order
                    });
                    selected_version = Some(versions[0].clone());
                } else if major_minors.len() == 1 {
                    // Only one version found, use it
                    selected_version = Some(major_minors.into_iter().next().unwrap());
                }
            }
        }

        // Default to 1.4
        let version_str = if let Some(v) = selected_version.as_ref() {
            v.to_string()
        } else {
            SUPPORTED_NODOS_VERSIONS[DEFAULT_NODOS_VERSION_INDEX].to_string()
        };

        fs::create_dir_all(&output_dir)?;

        let tool_template_dir = get_template_dir_for(lang_tool.tool(), &plugin_type, &version_str);
        let lang_template_dir = get_template_dir_for(lang_tool.lang(), &plugin_type, &version_str);

        if tool_template_dir.is_none() || lang_template_dir.is_none() {
            return Err(InvalidArgument { message: format!("No template found for plugin type {:?} and version {}", plugin_type, version_str) });
        }
        let tool_template_dir = tool_template_dir.unwrap();
        let lang_template_dir = lang_template_dir.unwrap();

        let manifest_path_ext = get_plugin_manifest_file_ext(selected_version.as_ref(), &plugin_type);

        // Copy .noscfg if plugin or .nossys
        let manifest_template_file = if plugin_type == PluginType::Default {
            DATA_DIR.get_file(format!("templates/nodos-{}/Plugin.{}", version_str, manifest_path_ext)).unwrap()
        } else {
            DATA_DIR.get_file(format!("templates/nodos-{}/Subsystem.{}", version_str, manifest_path_ext)).unwrap()
        };
        let output_manifest_path = output_dir.join(format!("{}.{}", plugin_name, manifest_path_ext));

        // Read file and replace placeholders
        // <NAME>
        // <DESCRIPTION>
        // <VERSION>
        // <DEPENDENCY_LIST_JSON>
        // <BINARY_NAME>
        let manifest_content = manifest_template_file.contents_utf8().unwrap();
        let manifest_content = manifest_content
            .replace("<NAME>", plugin_name)
            .replace("<DESCRIPTION>", description)
            .replace("<DISPLAY_NAME>", plugin_name)
            .replace("<VERSION>", "0.1.0")
            .replace("<DEPENDENCY_LIST_JSON>", serde_json::to_string(&deps).unwrap().as_str())
            .replace("<BINARY_NAME>", plugin_name);
        fs::write(&output_manifest_path, manifest_content)?;

        // Recursively copy the tool directory
        copy_include_dir_recursive(tool_template_dir, output_dir, Some(&mut |content| {
            Self::replace_tool_placeholders(workspace, content, plugin_name, &deps, lang_tool.tool());
        }))?;

        copy_include_dir_recursive(lang_template_dir, output_dir, Some(&mut |content| {
            Self::replace_lang_placeholders(content, lang_tool.lang());
        }))?;

        println!("{:?} project created at {:?}", plugin_type, output_dir);

        if workspace.ready() {
            workspace.scan_packages_in_folder(output_dir.clone(), ScanPackagesFlags::ForceReplaceInRegistry);
            workspace.save()?;
        }

        Ok(())
    }
}

pub fn get_cli() -> clap::Command {
    clap::Command::new("create")
        .about("Create a Nodos plugin")
        .arg(Arg::new("type")
            .value_parser(clap::builder::PossibleValuesParser::new(["plugin", "subsystem"]))
            .required(true)
        )
        .arg(Arg::new("name")
            .required(true)
        )
        .arg(get_lang_tool_arg())
        .arg(Arg::new("output_dir")
            .help("Path to create the plugin folder in")
            .long("output-dir")
            .short('o')
            .default_value("./Module")
            .required(false)
        )
        .arg(Arg::new("prefix")
            .help("Folder path relative to out_dir. The plugin contents will be under this folder. By default, its '<plugin_name>'.")
            .long("prefix")
            .required(false)
        )
        .arg(Arg::new("yes_to_all")
            .action(ArgAction::SetTrue)
            .long("yes-to-all")
            .help("Do not ask for confirmation & use defaults for missing parameters")
            .num_args(0)
            .short('y')
            .required(false)
        )
        .arg(Arg::new("description")
            .help("Description of the plugin")
            .long("description")
            .default_value("")
            .required(false)
        )
        .arg(Arg::new("dependency")
            .help("Add plugin dependency. Can be specified multiple times. Format: <plugin_name>-<version>")
            .long("dependency")
            .short('d')
            .required(false)
            .action(ArgAction::Append)
            .num_args(1)
        )
        .arg(Arg::new("nodos_version")
            .help("Nodos engine version to use for the plugin. If not specified, the latest version will be used.")
            .long("nodos-version")
            .short('n')
            .required(false)
            .value_name("VERSION")
            .num_args(1)
        )
}

impl Command for CreateCommand {
    fn matched_args<'a>(&self, _workspace: &Workspace, args: &'a ArgMatches) -> Option<&'a ArgMatches> {
        args.subcommand_matches("create")
    }

    fn run(&self, workspace: &mut Workspace, _command_name: Option<&str>, args: &ArgMatches) -> CommandResult {
        let plugin_type = match args.get_one::<String>("type").unwrap().as_str() {
            "plugin" => PluginType::Default,
            "subsystem" => PluginType::SubsystemLegacy,
            _ => panic!("Invalid module type") // Unreachable
        };
        let lang_tool = match args.get_one::<String>("language/tool").unwrap().as_str() {
            "cpp/cmake" => LangTool::CppCMake,
            _ => panic!("Invalid language/tool") // Unreachable
        };
        let module_name = args.get_one::<String>("name").unwrap();
        let mut output_dir = PathBuf::from(args.get_one::<String>("output_dir").unwrap());
        let prefix = args.get_one::<String>("prefix");
        if let Some(p) = prefix {
            output_dir = output_dir.join(p);
        } else {
            output_dir = output_dir.join(module_name.clone());
        }
        let depss: Vec<&String> = args.get_many::<String>("dependency").unwrap_or_default().collect();
        let mut deps: Vec<PackageIdentifier> = Vec::new();
        for dep in depss {
            let parts: Vec<&str> = dep.split('-').collect();
            if parts.len() != 2 {
                return Err(InvalidArgument { message: format!("Invalid dependency format: {}", dep) });
            }
            deps.push(PackageIdentifier {
                name: parts[0].to_string(),
                version: parts[1].to_string(),
            });
        }
        let mut deps_success = false;
        let deps = get_dependency_arguments(args, false, &mut deps_success);
        if !deps_success {
            return Err(InvalidArgument { message: "Invalid dependency format".to_string() });
        }

        let description = args.get_one::<String>("description").unwrap();
        let nodos_version = get_nodos_version_from_args(args)?;
        self.run_create(workspace, module_name, plugin_type, lang_tool, &output_dir, deps, description, nodos_version)
    }

    fn needs_workspace(&self) -> bool {
        false
    }
}
