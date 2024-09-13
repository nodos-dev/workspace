use serde::{Deserialize, Serialize};
use std::{fmt, fs};
use std::fmt::Display;
use std::path::PathBuf;
use std::time::Duration;
use colored::Colorize;
use indicatif::{ProgressBar};
use crate::nosman::constants;
use crate::nosman::index::{ModuleType};
use crate::nosman::path::{get_plugin_manifest_file, get_subsystem_manifest_file};

#[derive(Serialize, Deserialize, Debug, Eq, PartialEq, Hash, Clone)]
pub struct PackageIdentifier {
    pub name: String,
    pub version: String,
}

#[derive(Serialize, Deserialize, Debug, Eq, PartialEq, Hash, Clone)]
pub struct ModuleInfo {
    pub id: PackageIdentifier,
    pub display_name: Option<String>,
    pub description: Option<String>,
    pub dependencies: Option<Vec<PackageIdentifier>>,
    pub category: Option<String>,
    pub tags: Option<Vec<String>>,
}

#[derive(Serialize, Deserialize, Debug, Eq, PartialEq, Hash, Clone)]
pub struct InstalledModule {
    pub info: ModuleInfo,
    #[serde(alias = "config_path")]
    pub manifest_path: PathBuf,
    pub public_include_folder: Option<PathBuf>,
    pub type_schema_files: Vec<PathBuf>,
    pub module_type: ModuleType,
}

impl Display for InstalledModule {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{} ({})", self.info.id, self.manifest_path.display())
    }
}

#[derive(Debug, Eq, PartialEq, Clone)]
pub struct NodeDefinition {
    pub class_name: String,
    pub defined_in: PathBuf,
    pub index: usize,
    pub node_defs_json: serde_json::Value,
    pub owner: InstalledModule,
}

impl Display for NodeDefinition {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{} ({})", self.owner.info.id, self.defined_in.display())
    }
}

impl InstalledModule {
    pub fn new(path: PathBuf) -> InstalledModule {
        InstalledModule {
            info: ModuleInfo {
                id: PackageIdentifier {
                    name: String::new(),
                    version: String::new(),
                },
                display_name: None,
                description: None,
                dependencies: None,
                category: None,
                tags: None,
            },
            manifest_path: path,
            public_include_folder: None,
            type_schema_files: Vec::new(),
            module_type: ModuleType::Plugin,
        }
    }
    pub fn get_module_dir(&self) -> PathBuf {
        self.manifest_path.parent().unwrap().to_path_buf()
    }
    pub fn get_node_definition(&self, class_name: &str) -> Option<NodeDefinition> {
        // Read module manifest file as JSON, and read node definition files
        let manifest_file = fs::File::open(&self.manifest_path);
        if let Err(e) = manifest_file {
            eprintln!("{}", format!("Failed to open module manifest file ({}): {}", self.manifest_path.display(), e).red());
            return None;
        }
        let manifest_file = manifest_file.unwrap();
        let config: serde_json::Value = serde_json::from_reader(manifest_file).expect("Failed to parse config file");
        let node_defs_rel_paths = config["node_definitions"].as_array();
        if node_defs_rel_paths.is_none() {
            return None;
        }
        for node_defs_rel_path in node_defs_rel_paths.unwrap() {
            let node_defs_path = self.get_module_dir().join(node_defs_rel_path.as_str().unwrap());
            let node_defs_file_content = fs::read_to_string(&node_defs_path);
            if let Err(e) = node_defs_file_content {
                eprintln!("{}", format!("Failed to read node definitions file ({}): {}", node_defs_path.display(), e).red());
                continue;
            }
            let node_defs_file_content = node_defs_file_content.unwrap();
            // Remove BOM
            let node_defs_file_content = node_defs_file_content.trim_start_matches('\u{FEFF}');
            let node_defs: serde_json::Value = serde_json::from_str(&node_defs_file_content).expect(format!("Failed to parse node definitions file: {}", node_defs_path.display()).as_str());
            let nodes_json_array = node_defs.get("nodes").expect("Missing 'nodes' field in node definitions file").as_array().expect("'nodes' field is not an array");
            let mut index = 0;
            for node_json in nodes_json_array {
                let mut curr_class_name = node_json["class_name"].as_str().expect(format!("Missing 'class_name' field in node definition in {}", node_defs_path.display()).as_str()).to_string();
                // If class name is not prefixed with module name, prefix it
                if !curr_class_name.starts_with(self.info.id.name.as_str()) {
                    curr_class_name = format!("{}.{}", self.info.id.name, curr_class_name);
                }
                if curr_class_name == *class_name {
                    return Some (NodeDefinition {
                        class_name: curr_class_name.to_string(),
                        defined_in: node_defs_path.clone(),
                        index,
                        node_defs_json: node_defs.clone(),
                        owner: self.clone(),
                    });
                }
                index += 1;
            }
        }
        None
    }
}

impl fmt::Display for PackageIdentifier {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}-{}", self.name, self.version)
    }
}

pub fn get_module_manifest_file_in_folder(folder: &PathBuf) -> Result<Option<(ModuleType, PathBuf)>, String> {
    let res = get_plugin_manifest_file(folder);
    if res.is_err() {
        return Err(res.err().unwrap());
    }
    let plugin_manifest_file = res.unwrap();
    let res = get_subsystem_manifest_file(folder);
    if res.is_err() {
        return Err(res.err().unwrap());
    }
    let subsystem_manifest_file = res.unwrap();
    if plugin_manifest_file.is_some() && subsystem_manifest_file.is_some() {
        return Err(format!("Multiple module manifest files found in {}", folder.display()));
    }
    if plugin_manifest_file.is_none() && subsystem_manifest_file.is_none() {
        return Ok(None);
    }
    if plugin_manifest_file.is_some() {
        return Ok(Some((ModuleType::Plugin, plugin_manifest_file.unwrap())));
    }
    Ok(Some((ModuleType::Subsystem, subsystem_manifest_file.unwrap())))
}

pub fn get_module_manifests(folder: &PathBuf) -> Vec<(ModuleType, PathBuf)> {
    let pb = ProgressBar::new_spinner();
    pb.enable_steady_tick(Duration::from_millis(100));

    pb.set_message(format!("Looking for Nodos modules in {}", folder.to_str().expect("Non-UTF-8 path")).to_string());
    let res = get_module_manifest_file_in_folder(&folder);
    if res.is_ok() {
        if let Some((ty, mpath)) = res.unwrap() {
            return vec![(ty, mpath)];
        }
    }

    let patterns = &[format!("*.{{{},{}}}", constants::SUBSYSTEM_MANIFEST_FILE_EXT, constants::PLUGIN_MANIFEST_FILE_EXT)];
    let walker = globwalk::GlobWalkerBuilder::from_patterns(folder, patterns)
        .file_type(globwalk::FileType::FILE)
        .build()
        .expect(format!("Failed to glob dirs: {:?}", patterns).as_str());
    let mut module_manifest_files = vec![];
    for entry in walker {
        match entry {
            Ok(entry) => {
                let path = entry.path().to_path_buf();
                // If multiple manifest files are found in the same folder, we will skip this folder
                let parent = path.parent().expect("No parent found").to_path_buf();
                let res = get_module_manifest_file_in_folder(&parent);
                if let Ok(res) = res {
                    if let Some((ty, mpath)) = res {
                        module_manifest_files.push((ty, mpath));
                    }
                }
            }
            Err(e) => {
                pb.println(format!("Error while walking: {}", e).to_string());
            }
        }
    }

    pb.finish_and_clear();
    module_manifest_files
}
