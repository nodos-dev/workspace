use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use clap::{Arg, ArgAction, ArgMatches};
use colored::Colorize;
use crate::nosman::common::{copy_dir_recursive, copy_include_dir_recursive};
use crate::nosman::command::{get_lang_tool_arg, get_nodos_version_from_args, Command, CommandResult};
use crate::nosman::command::CommandError::InvalidArgument;
use crate::nosman::index::{PluginType, SemVer};
use include_dir::{include_dir, Dir};
use crate::nosman::command::sdk_info::get_engine_sdk_infos;
use crate::nosman::common::{DEFAULT_NODOS_VERSION_INDEX, NODOS_1_4, SUPPORTED_NODOS_VERSIONS};
use crate::nosman::lang_tool::LangTool;
use crate::nosman::module::get_dependency_arguments;
use crate::nosman::package::{get_plugin_manifest_file_ext, PackageIdentifier};
use crate::nosman::workspace::{ScanPackagesFlags, Workspace};

pub struct CreateCommand {}

static DATA_DIR: Dir = include_dir!("$CARGO_MANIFEST_DIR/data");

fn get_legacy_template_dir_for<'a>(name: &str, plugin_type: &PluginType, version: &str) -> Option<&'a Dir<'a>> {
    let template_dir = if *plugin_type == PluginType::Default {
        DATA_DIR.get_dir(format!("templates/nodos-{}/{}/plugin", version, name))
    } else {
        DATA_DIR.get_dir(format!("templates/nodos-{}/{}/subsystem", version, name))
    };
    template_dir
}

fn find_sdk_template_root(workspace: &Workspace, version: &SemVer) -> Result<PathBuf, crate::nosman::command::CommandError> {
    if !workspace.ready() {
        return Err(InvalidArgument { message: "Workspace is not ready; cannot locate SDK plugin templates".to_string() });
    }

    let engines = get_engine_sdk_infos(workspace)?;
    let mut best_match: Option<(SemVer, PathBuf)> = None;
    for engine in engines {
        let engine_semver = match SemVer::parse_from_str(engine.version.as_str()) {
            Some(v) => v,
            None => continue,
        };
        if !engine_semver.satisfies_requested_version(version) {
            continue;
        }
        let sdk_path = PathBuf::from(engine.path);
        if let Some((best_ver, _)) = best_match.as_ref() {
            if engine_semver > *best_ver {
                best_match = Some((engine_semver, sdk_path));
            }
        } else {
            best_match = Some((engine_semver, sdk_path));
        }
    }

    let sdk_path = match best_match {
        Some((_, path)) => path,
        None => {
            return Err(InvalidArgument { message: format!("No Nodos SDK found for version {}", version.to_string()) });
        }
    };

    Ok(sdk_path.join("Plugin").join("Template").join("Plugin"))
}

fn get_sdk_base_template_dir(template_root: &Path) -> PathBuf {
    template_root.join("Base")
}

fn get_sdk_toolchain_template_dir(template_root: &Path, tool: &str) -> PathBuf {
    template_root.join("Toolchain").join(tool)
}

fn get_sdk_language_template_dir(template_root: &Path, lang: &str) -> PathBuf {
    template_root.join("Language").join(lang)
}

impl CreateCommand {
    fn plugin_name_to_cpp_namespace(plugin_name: &str) -> String {
        plugin_name
            .split('.')
            .map(|part| {
                let mut sanitized = String::new();
                for (idx, ch) in part.chars().enumerate() {
                    if ch.is_ascii_alphanumeric() || ch == '_' {
                        if idx == 0 && ch.is_ascii_digit() {
                            sanitized.push('_');
                        }
                        sanitized.push(ch);
                    } else {
                        sanitized.push('_');
                    }
                }
                if sanitized.is_empty() {
                    "_".to_string()
                } else {
                    sanitized
                }
            })
            .collect::<Vec<_>>()
            .join("::")
    }

    fn split_plugin_name_parts(plugin_name: &str) -> Vec<String> {
        let mut parts = Vec::new();
        let mut current = String::new();
        for ch in plugin_name.chars() {
            if ch.is_ascii_alphanumeric() {
                current.push(ch);
            } else if !current.is_empty() {
                parts.push(current.clone());
                current.clear();
            }
        }
        if !current.is_empty() {
            parts.push(current);
        }
        parts
    }

    fn plugin_name_to_camel_case(plugin_name: &str) -> String {
        let parts = Self::split_plugin_name_parts(plugin_name);
        if parts.is_empty() {
            return String::new();
        }
        let mut out = String::new();
        for (idx, part) in parts.into_iter().enumerate() {
            let lower = part.to_ascii_lowercase();
            let mut chars = lower.chars();
            if let Some(first) = chars.next() {
                if idx == 0 {
                    out.push(first);
                } else {
                    out.push(first.to_ascii_uppercase());
                }
                out.push_str(chars.as_str());
            }
        }
        out
    }

    fn plugin_name_to_screaming_snake_case(plugin_name: &str) -> String {
        let parts = Self::split_plugin_name_parts(plugin_name);
        parts
            .into_iter()
            .map(|part| part.to_ascii_uppercase())
            .collect::<Vec<_>>()
            .join("_")
    }

    fn replace_plugin_placeholders(content: &mut String, plugin_name: &str) {
        let camel_case = Self::plugin_name_to_camel_case(plugin_name);
        let screaming_snake = Self::plugin_name_to_screaming_snake_case(plugin_name);
        *content = content
            .replace("<NAME>", plugin_name)
            .replace("<PLUGIN_NAME_CAMEL_CASE>", &camel_case)
            .replace("<PLUGIN_NAME_SCREAMING_SNAKE_CASE>", &screaming_snake);
    }

    fn replace_placeholders_in_dir(
        dir: &Path,
        workspace: &Workspace,
        plugin_name: &str,
        deps: &Vec<PackageIdentifier>,
        lang_tool: &LangTool,
    ) -> CommandResult {
        let mut stack = vec![dir.to_path_buf()];
        while let Some(current) = stack.pop() {
            let entries = match fs::read_dir(&current) {
                Ok(entries) => entries,
                Err(err) => {
                    return Err(crate::nosman::command::CommandError::IO {
                        file: current.to_string_lossy().to_string(),
                        message: format!("Failed to read directory: {}", err),
                    });
                }
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    stack.push(path);
                    continue;
                }
                let mut content = match fs::read_to_string(&path) {
                    Ok(content) => content,
                    Err(err) if err.kind() == io::ErrorKind::InvalidData => continue,
                    Err(err) => {
                        return Err(crate::nosman::command::CommandError::IO {
                            file: path.to_string_lossy().to_string(),
                            message: format!("Failed to read template file: {}", err),
                        });
                    }
                };
                let original = content.clone();
                Self::replace_plugin_placeholders(&mut content, plugin_name);
                Self::replace_tool_placeholders(workspace, &mut content, plugin_name, deps, lang_tool.tool());
                Self::replace_lang_placeholders(&mut content, lang_tool.lang(), plugin_name);
                if content != original {
                    fs::write(&path, content).map_err(|err| crate::nosman::command::CommandError::IO {
                        file: path.to_string_lossy().to_string(),
                        message: format!("Failed to write template file: {}", err),
                    })?;
                }
            }
        }
        Ok(())
    }

    fn rename_cpp_include_files(output_dir: &Path, plugin_name: &str) -> CommandResult {
        let camel_case = Self::plugin_name_to_camel_case(plugin_name);
        if camel_case.is_empty() {
            return Ok(());
        }
        let include_dir_base = output_dir.join("Include");
        let include_dir = include_dir_base.join("Plugin");
        if include_dir.exists() {
            let include_dir_dst = include_dir_base.join(&camel_case);
            if include_dir != include_dir_dst && !include_dir_dst.exists() {
                fs::rename(&include_dir, &include_dir_dst).map_err(|err| crate::nosman::command::CommandError::IO {
                    file: include_dir.to_string_lossy().to_string(),
                    message: format!("Failed to rename include directory: {}", err),
                })?;
            }
            let src = include_dir_dst.join("Plugin.h");
            if src.exists() {
                let dst = include_dir_dst.join(format!("{}.h", camel_case));
                if src != dst && !dst.exists() {
                    fs::rename(&src, &dst).map_err(|err| crate::nosman::command::CommandError::IO {
                        file: src.to_string_lossy().to_string(),
                        message: format!("Failed to rename header file: {}", err),
                    })?;
                }
            }
        } else {
            let src = include_dir_base.join("Plugin.h");
            if src.exists() {
                let dst = include_dir_base.join(format!("{}.h", camel_case));
                if src != dst && !dst.exists() {
                    fs::rename(&src, &dst).map_err(|err| crate::nosman::command::CommandError::IO {
                        file: src.to_string_lossy().to_string(),
                        message: format!("Failed to rename header file: {}", err),
                    })?;
                }
            }
        }
        Ok(())
    }

    fn replace_lang_placeholders(content: &mut String, lang: &str, module_name: &str) {
        if lang == "cpp" {
            let namespace = Self::plugin_name_to_cpp_namespace(module_name);
            *content = content.replace("<NOS_NAMESPACE>", namespace.as_str());
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

    pub fn run_create(&self, workspace: &mut Workspace, plugin_name: &str, plugin_type: Option<PluginType>, lang_tool: LangTool,
                      output_dir: &PathBuf, deps: Vec<PackageIdentifier>, description: &str, nodos_version: Option<SemVer>) -> CommandResult {
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

        let resolved_version = selected_version
            .clone()
            .unwrap_or_else(|| SUPPORTED_NODOS_VERSIONS[DEFAULT_NODOS_VERSION_INDEX].clone());

        let plugin_type = if let Some(plugin_type) = plugin_type {
            plugin_type
        } else if resolved_version >= NODOS_1_4 {
            PluginType::Default
        } else {
            return Err(InvalidArgument { message: "Plugin type is required for Nodos versions before 1.4. Use 'plugin' or 'subsystem'.".to_string() });
        };

        if resolved_version >= NODOS_1_4 {
            println!("{}", format!("Creating a new Nodos plugin project").green());
        }
        else {
            println!("{}", format!("Creating a new Nodos {:?} project", plugin_type).green());
        }

        if plugin_type == PluginType::SubsystemLegacy && resolved_version >= NODOS_1_4 {
            return Err(InvalidArgument { message: "Subsystems are not supported for Nodos 1.4 or later".to_string() });
        }

        let version_str = resolved_version.to_string();
        let is_sdk_template = resolved_version >= NODOS_1_4;

        fs::create_dir_all(&output_dir)?;

        let legacy_tool_template_dir = if is_sdk_template {
            None
        } else {
            get_legacy_template_dir_for(lang_tool.tool(), &plugin_type, &version_str)
        };
        let legacy_lang_template_dir = if is_sdk_template {
            None
        } else {
            get_legacy_template_dir_for(lang_tool.lang(), &plugin_type, &version_str)
        };

        if !is_sdk_template && (legacy_tool_template_dir.is_none() || legacy_lang_template_dir.is_none()) {
            return Err(InvalidArgument { message: format!("No template found for plugin type {:?} and version {}", plugin_type, version_str) });
        }

        let manifest_path_ext = get_plugin_manifest_file_ext(Some(&resolved_version), &plugin_type);

        // Copy .noscfg if plugin or .nossys
        let manifest_template_content = if is_sdk_template {
            let template_root = find_sdk_template_root(workspace, &resolved_version)?;
            let manifest_path = get_sdk_base_template_dir(&template_root)
                .join(format!("Plugin.{}.template", manifest_path_ext));
            fs::read_to_string(&manifest_path).map_err(|e| crate::nosman::command::CommandError::IO {
                file: manifest_path.to_string_lossy().to_string(),
                message: format!("Failed to read template manifest: {}", e),
            })?
        } else if plugin_type == PluginType::Default {
            DATA_DIR.get_file(format!("templates/nodos-{}/Plugin.{}", version_str, manifest_path_ext)).unwrap()
                .contents_utf8().unwrap().to_string()
        } else {
            DATA_DIR.get_file(format!("templates/nodos-{}/Subsystem.{}", version_str, manifest_path_ext)).unwrap()
                .contents_utf8().unwrap().to_string()
        };
        let output_manifest_path = output_dir.join(format!("{}.{}", plugin_name, manifest_path_ext));

        // Read file and replace placeholders
        // <NAME>
        // <DESCRIPTION>
        // <VERSION>
        // <DEPENDENCY_LIST_JSON>
        // <BINARY_NAME>
        let manifest_content = manifest_template_content
            .replace("<NAME>", plugin_name)
            .replace("<DESCRIPTION>", description)
            .replace("<DISPLAY_NAME>", plugin_name)
            .replace("<VERSION>", "0.1.0")
            .replace("<DEPENDENCY_LIST_JSON>", serde_json::to_string(&deps).unwrap().as_str())
            .replace("<BINARY_NAME>", plugin_name);
        fs::write(&output_manifest_path, manifest_content)?;

        if is_sdk_template {
            let template_root = find_sdk_template_root(workspace, &resolved_version)?;
            let base_template_dir = get_sdk_base_template_dir(&template_root);
            let tool_template_dir = get_sdk_toolchain_template_dir(&template_root, lang_tool.tool());
            let lang_template_dir = get_sdk_language_template_dir(&template_root, lang_tool.lang());

            if !base_template_dir.exists() || !lang_template_dir.exists() {
                return Err(InvalidArgument { message: format!("No SDK template found for version {}", version_str) });
            }

            let manifest_filename = format!("Plugin.{}.template", manifest_path_ext);
            copy_dir_recursive(
                &base_template_dir,
                output_dir,
                None,
                Some(&|path| path.file_name().and_then(|n| n.to_str()) == Some(manifest_filename.as_str())),
            )?;

            if tool_template_dir.exists() {
                copy_dir_recursive(&tool_template_dir, output_dir, None, None)?;
            }

            copy_dir_recursive(&lang_template_dir, output_dir, None, None)?;
                
            if lang_tool.lang() == "cpp" {
                Self::rename_cpp_include_files(output_dir, plugin_name)?;
            }
            Self::replace_placeholders_in_dir(output_dir, workspace, plugin_name, &deps, &lang_tool)?;
        } else {
            let legacy_tool_template_dir = legacy_tool_template_dir.unwrap();
            let legacy_lang_template_dir = legacy_lang_template_dir.unwrap();
            // Recursively copy the tool directory
            copy_include_dir_recursive(&legacy_tool_template_dir, output_dir, Some(&mut |content| {
                Self::replace_tool_placeholders(workspace, content, plugin_name, &deps, lang_tool.tool());
            }))?;

            copy_include_dir_recursive(&legacy_lang_template_dir, output_dir, Some(&mut |content| {
                Self::replace_lang_placeholders(content, lang_tool.lang(), plugin_name);
            }))?;
        }

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
        .arg(Arg::new("name")
            .required(true)
        )
        .arg(Arg::new("type")
            .value_parser(clap::builder::PossibleValuesParser::new(["plugin", "subsystem"]))
            .required(false)
            .help("Plugin type (required for Nodos versions before 1.4)")
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
        let plugin_type = match args.get_one::<String>("type").map(|s| s.as_str()) {
            Some("plugin") => Some(PluginType::Default),
            Some("subsystem") => Some(PluginType::SubsystemLegacy),
            Some(_) => panic!("Invalid module type"), // Unreachable
            None => None,
        };
        let lang_tool_str = args.get_one::<String>("language/tool").unwrap();
        let lang_tool = LangTool::from_str(lang_tool_str.as_str())
            .ok_or(InvalidArgument { message: format!("Unsupported language/tool: {}", lang_tool_str) })?;
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
