use serde::{Deserialize, Serialize};
use std::{fmt, path};
use std::path::PathBuf;
use crate::nosman::index::ModuleType;

#[derive(Serialize, Deserialize, Debug, Eq, PartialEq, Hash, Clone)]
pub struct ModuleIdentifier {
    pub name: String,
    pub version: String,
}

#[derive(Serialize, Deserialize, Debug, Eq, PartialEq, Hash, Clone)]
pub struct ModuleInfo {
    pub id: ModuleIdentifier,
    pub display_name: Option<String>,
    pub description: Option<String>,
    pub dependencies: Option<Vec<ModuleIdentifier>>,
}

#[derive(Serialize, Deserialize, Debug, Eq, PartialEq, Hash, Clone)]
pub struct InstalledModule {
    pub info: ModuleInfo,
    pub config_path: path::PathBuf,
    pub public_include_folder: Option<path::PathBuf>,
    pub type_schema_files: Vec<path::PathBuf>,
    pub module_type: ModuleType,
}

impl InstalledModule {
    pub fn new(path: path::PathBuf) -> InstalledModule {
        InstalledModule {
            info: ModuleInfo {
                id: ModuleIdentifier {
                    name: String::new(),
                    version: String::new(),
                },
                display_name: None,
                description: None,
                dependencies: None,
            },
            config_path: path,
            public_include_folder: None,
            type_schema_files: Vec::new(),
            module_type: ModuleType::Plugin,
        }
    }
    pub fn get_module_dir(&self) -> PathBuf {
        self.config_path.parent().unwrap().to_path_buf()
    }
}

impl fmt::Display for ModuleIdentifier {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}-{}", self.name, self.version)
    }
}
