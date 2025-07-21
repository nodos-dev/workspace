
pub static DEFAULT_PACKAGE_INDEX_REPO: &str = "https://github.com/nodos-dev/index";

pub static PACKAGE_INDEX_ROOT_FILE: &str = "index";

pub static LEGACY_PLUGIN_MANIFEST_FILE_EXT: &str = "noscfg";
pub static PLUGIN_MANIFEST_FILE_EXT: &str = "nosplugin";
pub static LEGACY_SUBSYSTEM_MANIFEST_FILE_EXT: &str = "nossys";
pub static LEGACY_NODE_DEFINITION_FILE_EXT: &str = "nosdef";
pub static NODE_DEFINITION_FILE_EXT: &str = "nosnode";

pub static PUBLISH_OPTIONS_FILE_NAME: &str = ".nospub";

pub static POSSIBLE_CAN_SHOW_AS: [&str; 7] = ["PROPERTY_ONLY", "INPUT_PIN_ONLY", "INPUT_PIN_OR_PROPERTY", "OUTPUT_PIN_OR_PROPERTY", "OUTPUT_PIN_ONLY", "INPUT_OUTPUT", "INPUT_OUTPUT_PROPERTY"];
pub static POSSIBLE_SHOW_AS: [&str; 3] = ["INPUT_PIN", "OUTPUT_PIN", "PROPERTY"];

pub static POSSIBLE_VERSION_CHECK_STRATEGY: [&str; 3] = ["none", "strict", "loose"];