use std::{fs, io};
use std::path::PathBuf;
use log::{info, warn};
use rand::{Rng, SeedableRng};
use rand::rngs::StdRng;
use nosman::nosman::command::install::{InstallCommand, InstallFlags, InstallOp};
use nosman::nosman::index::{ModuleType, SemVer};
use nosman::nosman::module::PackageIdentifier;
use nosman::nosman::workspace::Workspace;
use nosman::nosman::command::create::{CreateCommand, LangTool};
use nosman::nosman::command::get::GetCommand;
use nosman::nosman::constants;

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
        let requested_version = SemVer::parse_from_str(requested.version.as_str()).expect("Failed to parse semantic version");
        let installed_version = SemVer::parse_from_str(installed.id.version.as_str()).expect("Failed to parse semantic version");
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

fn test_cmake_build(test: &WorkspaceGen) {
    let res = std::process::Command::new("cmake")
        .current_dir(&test.workspace.root)
        .arg("-S")
        .arg("Toolchain/CMake")
        .arg("-B")
        .arg("Project")
        .output();
    if let Err(e) = res {
        panic!("Failed to generate project: {}", e);
    }
    let res = res.unwrap();
    assert!(res.status.success(), "Failed to generate project: {}", String::from_utf8_lossy(&res.stdout));
    let res = std::process::Command::new("cmake")
        .current_dir(&test.workspace.root)
        .arg("--build")
        .arg("Project")
        .output();
    if let Err(e) = res {
        panic!("Failed to build project: {}", e);
    }
    let res = res.unwrap();
    assert!(res.status.success(), "Failed to build project: {}", String::from_utf8_lossy(&res.stdout));
}

fn test_create_module(module_name: &str, module_type: ModuleType, description: &str) {
    let mut test = WorkspaceGen::new_random();

    // Install nodos and verify cmake generation and build works correctly.
    let res = GetCommand{}.run_get(&mut test.workspace, &"nodos".to_string(), Some(&"1.4".to_string()), true, true, false);
    if let Err(e) = res {
        panic!("Failed to install nodos: {}", e);
    }

    // Create the module
    let module_dir = test.workspace.root.join("Module").join(module_name);
    CreateCommand{}.run_create(
        &mut test.workspace,
        module_name,
        module_type.clone(),
        LangTool::CppCMake,
        &module_dir,
        Vec::new(), // No dependencies
        description
    ).expect(&format!("Failed to create {:?}", module_type));

    // Verify the module was created correctly
    assert!(module_dir.exists(), "{:?} directory was not created", module_type);

    // Check manifest file exists with correct extension
    let extension = match module_type {
        ModuleType::Subsystem => constants::SUBSYSTEM_MANIFEST_FILE_EXT,
        ModuleType::Plugin => constants::PLUGIN_MANIFEST_FILE_EXT,
    };
    let manifest_path = module_dir.join(format!("{}.{}", module_name, extension));
    assert!(manifest_path.exists(), "{:?} manifest file was not created", module_type);

    // Verify CMake files are present
    let cmake_lists_path = module_dir.join("CMakeLists.txt");
    assert!(cmake_lists_path.exists(), "CMakeLists.txt was not created");

    test_cmake_build(&test);
}

#[test]
fn create_subsystem() {
    test_create_module(
        "test.sys.example", 
        ModuleType::Subsystem, 
        "Test subsystem description"
    );
}

#[test]
fn create_plugin() {
    test_create_module(
        "test.example", 
        ModuleType::Plugin, 
        "Test plugin description"
    );
}
