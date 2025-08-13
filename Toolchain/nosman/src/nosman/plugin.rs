use std::fmt;
use std::fmt::Display;
use std::path::PathBuf;
use crate::nosman::package::LocalPackageEntry;

#[derive(Debug, Eq, PartialEq, Clone)]
pub struct NodeDefinition {
    pub class_name: String,
    pub defined_in: PathBuf,
    pub index: usize,
    pub json: serde_json::Value,
    pub owner: LocalPackageEntry,
}

impl Display for NodeDefinition {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{} ({})", self.owner.info.id, self.defined_in.display())
    }
}
