use std::{fs, io};
use std::path::PathBuf;
use log::{info, warn};
use rand::{Rng, SeedableRng};
use rand::rngs::StdRng;
use nosman::nosman::command::install::{InstallCommand, InstallFlags, InstallOp};
use nosman::nosman::index::{ModuleType, SemVer};
use nosman::nosman::module::{get_manifest_file_ext, PackageIdentifier};
use nosman::nosman::workspace::Workspace;
use nosman::nosman::command::create::{CreateCommand, LangTool};
use nosman::nosman::command::get::GetCommand;
use nosman::nosman::command::node::NodeCommand;
use nosman::nosman::command::pin::PinCommand;
use nosman::nosman::common::NODOS_1_4;

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
    if !res.status.success() {
        print!("Output:\n{}", String::from_utf8_lossy(&res.stderr));
        panic!("Failed to generate project");
    }
    let res = std::process::Command::new("cmake")
        .current_dir(&test.workspace.root)
        .arg("--build")
        .arg("Project")
        .output();
    if let Err(e) = res {
        panic!("Failed to build project: {}", e);
    }
    let res = res.unwrap();
    if !res.status.success() {
        print!("Output:\n{}", String::from_utf8_lossy(&res.stdout));
        panic!("Failed to build project");
    }
}

fn test_create_module(module_name: &str, module_type: ModuleType, description: &str, nodos_version: &str) {
    let mut test = WorkspaceGen::new_random();

    // Install nodos and verify cmake generation and build works correctly.
    let res = GetCommand{}.run_get(&mut test.workspace, &"nodos".to_string(), Some(&nodos_version.to_string()), true, true, false);
    if let Err(e) = res {
        panic!("Failed to install nodos: {}", e);
    }

    // Copy self to the workspace
    let nosman_path = std::env::current_exe().expect("Failed to get current executable path");
    // Set the target executable name
    let target_executable_name = format!("nodos{}", std::env::consts::EXE_SUFFIX);
    std::fs::copy(
        nosman_path,
        &test.workspace.root.join(target_executable_name)
    ).expect("Failed to copy nosman to workspace");

    // Create the module
    let module_dir = test.workspace.root.join("Module").join(module_name);
    let nodos_version = SemVer::parse_from_str(nodos_version);
    CreateCommand{}.run_create(
        &mut test.workspace,
        module_name,
        module_type.clone(),
        LangTool::CppCMake,
        &module_dir,
        Vec::new(), // No dependencies
        description,
        nodos_version.clone(),
    ).expect(&format!("Failed to create {:?}", module_type));

    // Verify the module was created correctly
    assert!(module_dir.exists(), "{:?} directory was not created", module_type);

    // Check manifest file exists with correct extension
    let extension = get_manifest_file_ext(Option::from(&nodos_version), &module_type);
    let manifest_path = module_dir.join(format!("{}.{}", module_name, extension));
    assert!(manifest_path.exists(), "{:?} manifest file was not created", module_type);

    // Verify CMake files are present
    let cmake_lists_path = module_dir.join("CMakeLists.txt");
    assert!(cmake_lists_path.exists(), "CMakeLists.txt was not created");

    test_cmake_build(&test);
}

#[test]
fn create_plugin_1_3() {
    test_create_module(
        "test.example",
        ModuleType::Plugin,
        "Test plugin description",
        "1.3"
    );
}

#[test]
fn create_subsystem_1_3() {
    test_create_module(
        "test.sys.example",
        ModuleType::Subsystem,
        "Test subsystem description",
        "1.3"
    );
}

#[test]
fn create_plugin_1_4() {
    test_create_module(
        "test.example",
        ModuleType::Plugin,
        "Test plugin description",
        "1.4"
    );
}

#[test]
fn create_subsystem_1_4() {
    test_create_module(
        "test.sys.example",
        ModuleType::Subsystem,
        "Test subsystem description",
        "1.4"
    );
}

// Helper to read node definition JSON
fn read_node_def_json(node_def_path: &PathBuf) -> serde_json::Value {
    let content = std::fs::read_to_string(node_def_path).expect("Failed to read node definition file");
    serde_json::from_str(&content).expect("Failed to parse node definition JSON")
}

fn test_node_add_remove(version: SemVer) {
    let mut test = WorkspaceGen::new_random();
    let module_name = format!("test{}.plugin", version.major);
    let module_dir = test.workspace.root.join("Module").join(&module_name);
    // Create plugin
    CreateCommand{}.run_create(
        &mut test.workspace,
        &module_name,
        ModuleType::Plugin,
        LangTool::CppCMake,
        &module_dir,
        Vec::new(),
        "Node test plugin",
        Some(version.clone()),
    ).expect("Failed to create plugin");
    // Add node
    let node_class = "MyNode";
    NodeCommand{}.run_node(
        &mut test.workspace,
        &module_name,
        &node_class.to_string(),
        false,
        Some("MyNode".to_string()),
        Some("A test node".to_string()),
        Some("TestCategory".to_string()),
        false,
        Some(version.clone()),
    ).expect("Failed to add node");
    // Find node definition file
    let plugin = test.workspace.get_or_select_installed_module(&module_name).unwrap();
    let manifest = plugin.read_manifest();
    let node_def_path;
    if version < NODOS_1_4 {
        let node_defs = manifest["node_definitions"].as_array().expect("No node_definitions");
        assert!(!node_defs.is_empty());
        node_def_path = plugin.get_module_dir().join(node_defs[0].as_str().unwrap());
    } else {
        // For Nodos 1.4 and later, node definitions are stored in a dedicated directory by default
        let defs = test.workspace.get_node_definitions(&format!("{}.MyNode", module_name).to_string(), &Some(version.clone()));
        assert!(!defs.is_empty(), "No node definitions found for {}.{}", module_name, node_class);
        node_def_path = defs[0].defined_in.clone();
    }

    assert!(node_def_path.exists());
    let json = read_node_def_json(&node_def_path);
    let nodes = json["nodes"].as_array().unwrap();
    assert_eq!(nodes[0]["class_name"], format!("{}.{}", module_name, node_class));
    // Remove node
    NodeCommand{}.run_node(
        &mut test.workspace,
        &module_name,
        &node_class.to_string(),
        true,
        None,
        None,
        None,
        false,
        Some(version.clone()),
    ).expect("Failed to remove node");
    // Node file should be gone
    assert!(!node_def_path.exists());
}

#[test]
fn node_add_remove_1_3() {
    test_node_add_remove(SemVer::new(1, Some(3), None, None));
}

#[test]
fn node_add_remove_1_4() {
    test_node_add_remove(SemVer::new(1, Some(4), None, None));
}

fn test_pin_add_remove(version: SemVer) {
    let mut test = WorkspaceGen::new_random();
    let module_name = format!("test{}.plugin", version.major);
    let module_dir = test.workspace.root.join("Module").join(&module_name);
    // Create plugin
    CreateCommand{}.run_create(
        &mut test.workspace,
        &module_name,
        ModuleType::Plugin,
        LangTool::CppCMake,
        &module_dir,
        Vec::new(),
        "Pin test plugin",
        Some(version.clone()),
    ).expect("Failed to create plugin");
    // Add node
    let node_class = "SomeNode";
    NodeCommand{}.run_node(
        &mut test.workspace,
        &module_name,
        &node_class.to_string(),
        false,
        Some(node_class.to_string()),
        Some("A node for pin test".to_string()),
        Some("PinCategory".to_string()),
        false,
        Some(version.clone()),
    ).expect("Failed to add node");
    // Find node definition file
    let plugin = test.workspace.get_or_select_installed_module(&module_name).unwrap();
    let manifest = plugin.read_manifest();
    let node_def_path;
    if version < NODOS_1_4 {
        let node_defs = manifest["node_definitions"].as_array().expect("No node_definitions");
        assert!(!node_defs.is_empty());
        node_def_path = plugin.get_module_dir().join(node_defs[0].as_str().unwrap());
    } else {
        // For Nodos 1.4 and later, node definitions are stored in a dedicated directory by default
        let defs = test.workspace.get_node_definitions(&format!("{}.{}", module_name, node_class).to_string(), &Some(version.clone()));
        assert!(!defs.is_empty(), "No node definitions found for {}.{}", module_name, node_class);
        node_def_path = defs[0].defined_in.clone();
    }
    // Add pin
    PinCommand{}.run_pin(
        &test.workspace,
        &format!("{}.{}", module_name, node_class),
        &"myPin".to_string(),
        false,
        Some(&"INPUT_PIN".to_string()),
        Some(&"INPUT_PIN_ONLY".to_string()),
        Some(&"float".to_string()),
        Some(version.clone()),
    ).expect("Failed to add pin");
    // Verify pin exists
    let json = read_node_def_json(&node_def_path);
    let pins = json["nodes"][0]["node"]["pins"].as_array().unwrap();
    assert!(pins.iter().any(|p| p["name"] == "myPin"));
    // Remove pin
    PinCommand{}.run_pin(
        &test.workspace,
        &format!("{}.{}", module_name, node_class),
        &"myPin".to_string(),
        true,
        None,
        None,
        None,
        Some(version.clone()),
    ).expect("Failed to remove pin");
    // Verify pin is gone
    let json = read_node_def_json(&node_def_path);
    let pins = json["nodes"][0]["node"]["pins"].as_array().unwrap();
    assert!(!pins.iter().any(|p| p["name"] == "myPin"));
}

#[test]
fn pin_add_remove_1_3() {
    test_pin_add_remove(SemVer::new(1, Some(3), None, None));
}

#[test]
fn pin_add_remove_1_4() {
    test_pin_add_remove(SemVer::new(1, Some(4), None, None));
}
