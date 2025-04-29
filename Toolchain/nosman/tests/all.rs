use std::{fs, io};
use std::path::PathBuf;
use log::{info, warn};
use rand::{Rng, SeedableRng};
use rand::rngs::StdRng;
use nosman::nosman::command::install::{InstallCommand, InstallFlags, InstallOp};
use nosman::nosman::index::SemVer;
use nosman::nosman::module::PackageIdentifier;
use nosman::nosman::workspace::Workspace;

#[ctor::ctor]
fn init() {
    env_logger::Builder::from_default_env()
        .filter_level(log::LevelFilter::Info)
        .init();
    info!("Starting nosman tests");
}

#[ctor::dtor]
fn cleanup() {
    if let Err(e) = WorkspaceGen::clear_all() {
        warn!("Failed to clear test workspaces: {}", e);
    } else {
        info!("Test workspaces cleared");
    }
}

// Global random number generator
lazy_static::lazy_static! {
    static ref RNG: std::sync::Mutex<StdRng> = std::sync::Mutex::new(StdRng::from_entropy());
}

pub struct WorkspaceGen {
    pub workspace: Workspace,
}

impl WorkspaceGen {
    fn new(path: &str) -> Self {
        let mut ws = Workspace::from_root(&PathBuf::from(format!("./test_workspaces/{}", path)));
        if !ws.ready() {
            ws.recreate().expect("Failed to create test workspace");
        }
        WorkspaceGen { workspace: ws }
    }
    fn clear_all() -> io::Result<()> {
        fs::remove_dir_all("./test_workspaces")
    }
    pub(crate) fn new_random() -> Self {
        let random_string: String = (0..8)
            .map(|_| RNG.lock().unwrap().gen_range(b'a'..=b'z'))
            .map(char::from)
            .collect();
        WorkspaceGen::new(random_string.as_str())
    }
}

#[test]
fn install_no_deps() {
    let mut test = WorkspaceGen::new_random();
    let package_name = "nos.sys.vulkan";
    let version = String::from("6.2.1.b612");
    InstallCommand{}.run_install(&mut test.workspace, package_name, Some(&version), &PathBuf::from("."), None, InstallFlags::UpdatePackageIndex | InstallFlags::WithoutDependencies)
        .unwrap_or_else(|_| panic!("Failed to install {}", package_name));
    let versions = test.workspace.get_installed_modules(package_name);
    assert_eq!(versions.len(), 1);
}

#[test]
fn install_brings_dependencies() {
    let mut test = WorkspaceGen::new_random();
    let package_name = "nos.sys.vulkan";
    let version = String::from("6.2.1.b612");
    test.workspace.fetch_package_releases(package_name);
    let index_entry = test.workspace.index_cache.get_package(package_name, version.as_str());
    assert!(index_entry.is_some());
    let index_entry = index_entry.unwrap().1;
    let mut requested_modules = index_entry.dependencies.clone().expect("No dependencies found");
    requested_modules.push(PackageIdentifier {
        name: package_name.to_string(),
        version: version.clone(),
    });
    InstallCommand{}.run_install(&mut test.workspace, package_name, Some(&version), &PathBuf::from("."), None, InstallFlags::UpdatePackageIndex)
        .unwrap_or_else(|_| panic!("Failed to install {}", package_name));
    assert_eq!(requested_modules.len(), test.workspace.get_installed_module_count());
    for requested in requested_modules {
        let installed = test.workspace.get_installed_modules(requested.name.as_str());
        assert_eq!(installed.len(), 1);
        let installed = installed[0].info.clone();
        let requested_version = SemVer::parse_from_string(requested.version.as_str()).expect("Failed to parse semantic version");
        let installed_version = SemVer::parse_from_string(installed.id.version.as_str()).expect("Failed to parse semantic version");
        assert_eq!(installed.id.name, requested.name);
        assert!(installed_version.satisfies_requested_version(&requested_version));
    }
}

#[test]
fn install_skips_if_already_installed() {
    let mut test = WorkspaceGen::new_random();
    let package_name = "nos.sys.vulkan";
    let version = String::from("6.2.1.b612");
    let op = InstallCommand{}.run_install(&mut test.workspace, package_name, Some(&version), &PathBuf::from("."), None, InstallFlags::UpdatePackageIndex | InstallFlags::WithoutDependencies)
        .unwrap_or_else(|_| panic!("Failed to install {}", package_name));
    assert_eq!(op, InstallOp::Installed);

    let op_second = InstallCommand{}.run_install(&mut test.workspace, package_name, Some(&version), &PathBuf::from("."), None, InstallFlags::UpdatePackageIndex | InstallFlags::WithoutDependencies)
        .unwrap_or_else(|_| panic!("Failed to install {}", package_name));
    assert_eq!(op_second, InstallOp::Skipped);
}
