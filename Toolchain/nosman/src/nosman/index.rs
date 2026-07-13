use crate::nosman::package::PackageIdentifier;
use crate::nosman::platform::get_host_platform;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Serialize, Deserialize, Debug, Eq, PartialEq, Hash, Clone)]
pub enum PackageType {
    #[serde(alias = "plugin", alias = "PLUGIN")]
    Plugin,
    #[serde(alias = "subsystem", alias = "SUBSYSTEM")]
    Subsystem,
    #[serde(alias = "nodos", alias = "NODOS")]
    Nodos,
    #[serde(alias = "engine", alias = "ENGINE")]
    Engine,
    #[serde(alias = "generic", alias = "GENERIC")]
    Generic,
}

impl PackageType {
    pub fn is_plugin(&self) -> bool {
        matches!(self, PackageType::Plugin | PackageType::Subsystem)
    }

    #[allow(clippy::should_implement_trait)]
    pub fn from_str(value: &str) -> PackageType {
        match value {
            "Plugin" => PackageType::Plugin,
            "Subsystem" => PackageType::Subsystem,
            "Nodos" => PackageType::Nodos,
            "Engine" => PackageType::Engine,
            _ => PackageType::Generic,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            PackageType::Plugin => "Plugin",
            PackageType::Subsystem => "Subsystem",
            PackageType::Nodos => "Nodos",
            PackageType::Engine => "Engine",
            PackageType::Generic => "Generic",
        }
    }
}

impl std::fmt::Display for PackageType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

#[derive(Serialize, Deserialize, Debug, Eq, PartialEq, Hash, Clone)]
pub enum PluginType {
    Default,
    SubsystemLegacy,
}

#[derive(Serialize, Deserialize, Debug, Eq, PartialEq, Hash, Clone, Default)]
pub struct SemVer {
    #[serde(alias = "major", alias = "MAJOR", alias = "Major")]
    pub major: u32,
    #[serde(
        alias = "minor",
        alias = "MINOR",
        alias = "Minor",
        skip_serializing_if = "Option::is_none"
    )]
    pub minor: Option<u32>,
    #[serde(
        alias = "patch",
        alias = "PATCH",
        alias = "Patch",
        skip_serializing_if = "Option::is_none"
    )]
    pub patch: Option<u32>,
    #[serde(
        alias = "build",
        alias = "BUILD",
        alias = "Build",
        skip_serializing_if = "Option::is_none"
    )]
    pub build_number: Option<u32>,
}

// Implement ordering for SemVer
impl std::cmp::Ord for SemVer {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        if self.major < other.major {
            return std::cmp::Ordering::Less;
        }
        if self.major > other.major {
            return std::cmp::Ordering::Greater;
        }
        if self.minor < other.minor {
            return std::cmp::Ordering::Less;
        }
        if self.minor > other.minor {
            return std::cmp::Ordering::Greater;
        }
        if self.patch < other.patch {
            return std::cmp::Ordering::Less;
        }
        if self.patch > other.patch {
            return std::cmp::Ordering::Greater;
        }
        // A plain release (no build number) outranks a build/pre-release of the
        // same major.minor.patch (e.g. "2.0.0" > "2.0.0.b1179"), matching semver
        // convention. Without this, PartialEq/Eq (derived, field-by-field) call
        // these unequal while Ord called them Equal -- an inconsistency that let
        // a stale downloaded pre-release tie with, and sometimes beat, a real
        // local release in "give me the latest" lookups.
        match (self.build_number, other.build_number) {
            (Some(a), Some(b)) => {
                if a < b {
                    return std::cmp::Ordering::Less;
                }
                if a > b {
                    return std::cmp::Ordering::Greater;
                }
            }
            (None, Some(_)) => return std::cmp::Ordering::Greater,
            (Some(_), None) => return std::cmp::Ordering::Less,
            (None, None) => {}
        }
        std::cmp::Ordering::Equal
    }
}

impl PartialOrd for SemVer {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl SemVer {
    pub fn new(
        major: u32,
        minor: Option<u32>,
        patch: Option<u32>,
        build_number: Option<u32>,
    ) -> SemVer {
        SemVer {
            major,
            minor,
            patch,
            build_number,
        }
    }
    pub fn parse_from_str(s: &str) -> Option<SemVer> {
        // Parse 1.2.3.b4 -> (1, 2, 3, Some(4))
        // Parse 1.2.3.4 -> (1, 2, 3, Some(4))
        // Parse 1.2.3 -> (1, 2, 3, None)
        // Parse 1.2 -> (1, 2, 0, None)
        // Parse 1 -> (1, 0, 0, None)
        let parts: Vec<&str> = s.split('.').collect();
        let opt_major = parts.first().and_then(|s| s.parse::<u32>().ok());
        let opt_minor = parts.get(1).and_then(|s| s.parse::<u32>().ok());
        let opt_patch = parts.get(2).and_then(|s| s.parse::<u32>().ok());
        let opt_build_number = parts.get(3).and_then(|s| {
            if s.starts_with("b") {
                s.get(1..).and_then(|s| s.parse::<u32>().ok())
            } else {
                s.parse::<u32>().ok()
            }
        });
        let major = opt_major?;
        Some(SemVer {
            major,
            minor: opt_minor,
            patch: opt_patch,
            build_number: opt_build_number,
        })
    }
    pub fn matches_prefix(&self, prefix: &SemVer) -> bool {
        // Check if this version matches the prefix
        // For prefix "6" (major only): matches 6.x.x
        // For prefix "6.30" (major.minor): matches 6.30.x
        // For prefix "6.30.1" (major.minor.patch): matches 6.30.1.x
        // For prefix "6.30.1.b709" (full): matches exact 6.30.1.b709

        // Major version must match
        if self.major != prefix.major {
            return false;
        }

        // If prefix specifies minor, check it
        if let Some(prefix_minor) = prefix.minor {
            match self.minor {
                Some(self_minor) if self_minor != prefix_minor => return false,
                None => return false,
                _ => {}
            }
        } else {
            // Prefix is major-only, so any minor matches
            return true;
        }

        // If prefix specifies patch, check it
        if let Some(prefix_patch) = prefix.patch {
            match self.patch {
                Some(self_patch) if self_patch != prefix_patch => return false,
                None => return false,
                _ => {}
            }
        } else {
            // Prefix is major.minor, so any patch matches
            return true;
        }

        // If prefix specifies build_number, check it
        if let Some(prefix_build) = prefix.build_number {
            match self.build_number {
                Some(self_build) if self_build != prefix_build => return false,
                None => return false,
                _ => {}
            }
        } else {
            // Prefix is major.minor.patch, so any build matches
            return true;
        }

        // All specified fields match
        true
    }

    pub fn satisfies_requested_version(&self, requested: &SemVer) -> bool {
        if self.major != requested.major {
            return false;
        }
        self.minor >= requested.minor
    }
    pub fn is_equal_excl_build_no(&self, other: &SemVer) -> bool {
        self.major == other.major && self.minor == other.minor && self.patch == other.patch
    }
}

impl std::fmt::Display for SemVer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.major)?;
        if let Some(minor) = self.minor {
            write!(f, ".{}", minor)?;
        }
        if let Some(patch) = self.patch {
            write!(f, ".{}", patch)?;
        }
        if let Some(build_number) = self.build_number {
            write!(f, ".b{}", build_number)?;
        }
        Ok(())
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct PackageReleaseEntry {
    pub(crate) version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) artifact_id: Option<i64>,
    pub(crate) url: String,
    // TODO: Replace plugin_api_version & subsystem_api_version with these
    // plugin_type: String,
    // api_version: SemVer,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) plugin_api_version: Option<SemVer>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) subsystem_api_version: Option<SemVer>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) release_date: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dependencies: Option<Vec<PackageIdentifier>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub module_tags: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub release_tags: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub platform: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node_names: Option<Vec<String>>,
}
#[derive(Serialize, Deserialize, Debug, Default)]
pub struct Index {
    pub packages: HashMap<String, (PackageType, Vec<PackageReleaseEntry>)>, // name -> version -> ModuleReleaseEntry
}

fn sort_version_list(versions: &mut Vec<&PackageReleaseEntry>) {
    versions.sort_by(|a, b| {
        let semver_a = SemVer::parse_from_str(&a.version);
        let semver_b = SemVer::parse_from_str(&b.version);
        if semver_a.is_none() || semver_b.is_none() {
            return std::cmp::Ordering::Equal;
        }
        let semver_a = semver_a.unwrap();
        let semver_b = semver_b.unwrap();
        semver_a.cmp(&semver_b)
    });
}

impl Index {
    pub fn add_package(
        &mut self,
        name: &str,
        package_type: PackageType,
        package: PackageReleaseEntry,
    ) {
        let type_versions = self
            .packages
            .entry(name.to_owned())
            .or_insert((package_type, Vec::new()));
        type_versions.1.push(package);
    }
    pub fn get_package(
        &self,
        name: &str,
        version: &str,
    ) -> Option<(&PackageType, &PackageReleaseEntry)> {
        let res = self.packages.get(name);
        let (package_type, version_list) = res?;
        let platform = get_host_platform().to_string();
        for module in version_list {
            if module.version == version
                && (module.platform.is_none() || module.platform.as_ref()? == &platform)
            {
                return Some((package_type, module));
            }
        }
        None
    }
    pub fn get_package_releases(&self, name: &str) -> Vec<&PackageReleaseEntry> {
        let res = self.packages.get(name);
        if res.is_none() {
            return Vec::new();
        }
        let (_, version_list) = res.unwrap();
        version_list.iter().collect()
    }
    pub fn get_package_cpy(
        &self,
        name: &str,
        version: &str,
    ) -> Option<(PackageType, PackageReleaseEntry)> {
        let (package_type, pkg_release) = self.get_package(name, version)?;
        Some((package_type.clone(), pkg_release.clone()))
    }
    pub fn get_latest_release(&self, name: &str) -> Option<(&PackageType, &PackageReleaseEntry)> {
        let res = self.packages.get(name);
        let (package_type, version_list) = res?;
        let mut versions: Vec<&PackageReleaseEntry> = version_list.iter().collect();
        sort_version_list(&mut versions);
        versions.reverse();
        if versions.is_empty() {
            return None;
        }
        let platform = get_host_platform().to_string();
        for module in versions {
            if module.platform.is_none() || module.platform.as_ref()? == &platform {
                return Some((package_type, module));
            }
        }
        None
    }
    pub fn get_latest_compatible_release(
        &mut self,
        name: &str,
        version_prefix: &SemVer,
    ) -> Option<(&PackageType, &PackageReleaseEntry)> {
        let res = self.packages.get(name);
        let (package_type, version_list) = res?;
        let mut versions: Vec<&PackageReleaseEntry> = version_list.iter().collect();
        sort_version_list(&mut versions);
        versions.reverse();
        let platform = get_host_platform().to_string();
        for module in versions {
            let semver = SemVer::parse_from_str(&module.version);
            if semver.is_none() {
                continue;
            }
            let semver = semver?;
            if semver.matches_prefix(version_prefix)
                && (module.platform.is_none() || module.platform.as_ref()? == &platform)
            {
                return Some((package_type, module));
            }
        }
        None
    }
}
