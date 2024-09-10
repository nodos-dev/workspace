use std::fs;
use std::path::{Path, PathBuf};
use clap::{ArgMatches};
use crate::nosman::command::{Command, CommandResult};
use crate::nosman::command::CommandError::InvalidArgumentError;
use crate::nosman::index::ModuleType;
use include_dir::{include_dir, Dir};
use crate::nosman::command::sdk_info::get_engine_sdk_infos;
use crate::nosman::constants;
use crate::nosman::module::PackageIdentifier;
use crate::nosman::workspace::Workspace;

pub struct CreateCommand {}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum LangTool {
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

fn get_template_dir_for<'a>(name: &str, module_type: &ModuleType) -> &'a Dir<'a> {
    let template_dir = if *module_type == ModuleType::Plugin {
        DATA_DIR.get_dir(format!("templates/{}/plugin", name)).unwrap()
    } else {
        DATA_DIR.get_dir(format!("templates/{}/subsystem", name)).unwrap()
    };
    template_dir
}

fn copy_dir_recursive(src: &Dir, dest: &Path, modify: &mut dyn FnMut(&mut String)) -> std::io::Result<()> {
    let mut stack: Vec<&Dir> = vec![src];
    while let Some(dir) = stack.pop() {
        let target_dir = dest.join(dir.path().strip_prefix(src.path()).unwrap());
        for entry in dir.entries() {
            if let Some(d) = entry.as_dir() {
                stack.push(d);
                fs::create_dir_all(target_dir.join(entry.path().file_name().unwrap()))?;
            } else {
                let mut content: String = entry.as_file().unwrap().contents_utf8().unwrap().to_string();
                modify(&mut content);
                fs::write(target_dir.join(entry.path().file_name().unwrap()), content)?;
            }
        }
    }
    Ok(())
}

impl CreateCommand {
    fn replace_lang_placeholders(_content: &mut String, lang: &str) {
        if lang == "cpp" {
            // If any placeholders are added in the future
        }
    }

    fn replace_tool_placeholders(content: &mut String, module_name: &str, deps: &Vec<PackageIdentifier>, tool: &str) {
        let mut nos_version = "1.3.0".to_string();
        if let Ok(engines) = get_engine_sdk_infos() {
            if let Some(engine) = engines.first() {
                nos_version = engine.version.to_string();
            }
        }
        if tool == "cmake" {
            *content = content
                .replace("<CMAKE_PROJECT_NAME>", module_name)
                .replace("<CMAKE_LATEST_NOS_VERSION>", nos_version.as_str())
                .replace("<CMAKE_MODULE_DEPENDENCIES>", &deps.iter().map(|dep| {
                    format!("\"{}-{}\"", dep.name, dep.version)
                }).collect::<Vec<_>>().join(" "));
        }
    }

    fn run_create(&self, module_name: &str, module_type: ModuleType, lang_tool: LangTool,
                  output_dir: &PathBuf, deps: Vec<PackageIdentifier>, description: &str) -> CommandResult {
        println!("Creating a new Nodos module project of type {:?}", module_type);

        fs::create_dir_all(&output_dir)?;

        let tool_template_dir = get_template_dir_for(lang_tool.tool(), &module_type);
        let lang_template_dir = get_template_dir_for(lang_tool.lang(), &module_type);

        // Copy .noscfg if plugin or .nossys
        let cfg_template_file = if module_type == ModuleType::Plugin {
            DATA_DIR.get_file(format!("templates/Plugin.{}", constants::PLUGIN_MANIFEST_FILE_EXT)).unwrap()
        } else {
            DATA_DIR.get_file(format!("templates/Subsystem.{}", constants::SUBSYSTEM_MANIFEST_FILE_EXT)).unwrap()
        };
        let output_cfg_path = output_dir.join(format!("{}.{}", module_name, if module_type == ModuleType::Plugin { constants::PLUGIN_MANIFEST_FILE_EXT } else { constants::SUBSYSTEM_MANIFEST_FILE_EXT }));

        // Read file and replace placeholders
        // <NAME>
        // <DESCRIPTION>
        // <VERSION>
        // <DEPENDENCY_LIST_JSON>
        // <BINARY_NAME>
        let cfg_content = cfg_template_file.contents_utf8().unwrap();
        let cfg_content = cfg_content
            .replace("<NAME>", module_name)
            .replace("<DESCRIPTION>", description)
            .replace("<DISPLAY_NAME>", module_name)
            .replace("<VERSION>", "0.1.0")
            .replace("<DEPENDENCY_LIST_JSON>", serde_json::to_string(&deps).unwrap().as_str())
            .replace("<BINARY_NAME>", module_name);
        fs::write(&output_cfg_path, cfg_content)?;

        // Recursively copy the tool directory
        copy_dir_recursive(tool_template_dir, output_dir, &mut |content| {
            Self::replace_tool_placeholders(content, module_name, &deps, lang_tool.tool());
        })?;

        copy_dir_recursive(lang_template_dir, output_dir, &mut |content| {
            Self::replace_lang_placeholders(content, lang_tool.lang());
        })?;

        println!("{:?} project created at {:?}", module_type, output_dir);

        let ws_res = Workspace::get();
        if ws_res.is_ok() {
            let mut ws = ws_res.unwrap();
            ws.scan_modules_in_folder(output_dir.clone(), true);
            ws.save()?;
        }

        Ok(true)
    }
}

impl Command for CreateCommand {
    fn matched_args<'a>(&self, args: &'a ArgMatches) -> Option<&'a ArgMatches> {
        args.subcommand_matches("create")
    }

    fn needs_workspace(&self) -> bool {
        false
    }

    fn run(&self, args: &ArgMatches) -> CommandResult {
        let module_type = match args.get_one::<String>("type").unwrap().as_str() {
            "plugin" => ModuleType::Plugin,
            "subsystem" => ModuleType::Subsystem,
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
                return Err(InvalidArgumentError { message: format!("Invalid dependency format: {}", dep) });
            }
            deps.push(PackageIdentifier {
                name: parts[0].to_string(),
                version: parts[1].to_string(),
            });
        }
        let description = args.get_one::<String>("description").unwrap();
        self.run_create(module_name, module_type, lang_tool, &output_dir, deps, description)
    }
}
