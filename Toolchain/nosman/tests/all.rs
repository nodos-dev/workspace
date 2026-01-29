use log::{info, warn};
use nosman::nosman::command::create::{CreateCommand};
use nosman::nosman::command::dev::DevInitCommand;
use nosman::nosman::command::get::GetCommand;
use nosman::nosman::command::info::InfoCommand;
use nosman::nosman::command::install::{InstallCommand, InstallFlags, InstallOp};
use nosman::nosman::command::node::NodeCommand;
use nosman::nosman::command::pin::PinCommand;
use nosman::nosman::command::sdk_info::SdkInfoCommand;
use nosman::nosman::common::{set_prompt_handler, NODOS_1_4};
use nosman::nosman::index::{PluginType, SemVer};
use nosman::nosman::workspace::{OutputMode, Workspace};
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use std::path::PathBuf;
use std::{fs, io};
use std::process::Output;
use std::sync::{Arc, Mutex};
use nosman::nosman::lang_tool::LangTool;
use nosman::nosman::package::{get_plugin_manifest_file_ext, PackageIdentifier};

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
    static ref RNG: std::sync::Mutex<StdRng> = std::sync::Mutex::new(StdRng::from_os_rng());
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
        ws.push_output_mode(OutputMode::Silent);
        WorkspaceGen { workspace: ws }
    }
    fn clear_all() -> io::Result<()> {
        fs::remove_dir_all("./test_workspaces")
    }
    pub(crate) fn new_random() -> Self {
        let random_string: String = (0..8)
            .map(|_| RNG.lock().unwrap().random_range(b'a'..=b'z'))
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
    InstallCommand {}
        .run_install(
            &mut test.workspace,
            package_name,
            Some(&version),
            &Some(PathBuf::from(".")),
            None,
            InstallFlags::UpdatePackageIndex | InstallFlags::WithoutDependencies,
        )
        .unwrap_or_else(|_| panic!("Failed to install {}", package_name));
    let versions = test.workspace.get_packages(package_name);
    assert_eq!(versions.len(), 1);
}

#[test]
fn install_brings_dependencies() {
    let mut test = WorkspaceGen::new_random();
    let package_name = "nos.sys.vulkan";
    let version = String::from("6.2.1.b612");
    test.workspace.fetch_package_releases(package_name);
    let index_entry = test
        .workspace
        .index_cache
        .get_package(package_name, version.as_str());
    assert!(index_entry.is_some());
    let index_entry = index_entry.unwrap().1;
    let mut requested_modules = index_entry
        .dependencies
        .clone()
        .expect("No dependencies found");
    requested_modules.push(PackageIdentifier {
        name: package_name.to_string(),
        version: version.clone(),
    });
    InstallCommand {}
        .run_install(
            &mut test.workspace,
            package_name,
            Some(&version),
            &Some(PathBuf::from(".")),
            None,
            InstallFlags::UpdatePackageIndex,
        )
        .unwrap_or_else(|_| panic!("Failed to install {}", package_name));
    assert_eq!(
        requested_modules.len(),
        test.workspace.get_local_package_count()
    );
    for requested in requested_modules {
        let installed = test
            .workspace
            .get_packages(requested.name.as_str());
        assert_eq!(installed.len(), 1);
        let installed = installed[0].info.clone();
        let requested_version = SemVer::parse_from_str(requested.version.as_str())
            .expect("Failed to parse semantic version");
        let installed_version = SemVer::parse_from_str(installed.id.version.as_str())
            .expect("Failed to parse semantic version");
        assert_eq!(installed.id.name, requested.name);
        assert!(installed_version.satisfies_requested_version(&requested_version));
    }
}

#[test]
fn install_skips_if_already_installed() {
    let mut test = WorkspaceGen::new_random();
    let package_name = "nos.sys.vulkan";
    let version = String::from("6.2.1.b612");
    let op = InstallCommand {}
        .run_install(
            &mut test.workspace,
            package_name,
            Some(&version),
            &Some(PathBuf::from(".")),
            None,
            InstallFlags::UpdatePackageIndex | InstallFlags::WithoutDependencies,
        )
        .unwrap_or_else(|_| panic!("Failed to install {}", package_name));
    assert_eq!(op, InstallOp::Installed);

    let op_second = InstallCommand {}
        .run_install(
            &mut test.workspace,
            package_name,
            Some(&version),
            &Some(PathBuf::from(".")),
            None,
            InstallFlags::UpdatePackageIndex | InstallFlags::WithoutDependencies,
        )
        .unwrap_or_else(|_| panic!("Failed to install {}", package_name));
    assert_eq!(op_second, InstallOp::Skipped);
}

fn get_full_output(res: &Output) -> String {
    let stdout = String::from_utf8_lossy(&res.stdout);
    let stderr = String::from_utf8_lossy(&res.stderr);
    let mut output = String::new();
    if !stdout.is_empty() {
        output.push_str(&format!("{}", stdout));
    }
    if !stderr.is_empty() {
        output.push_str(&format!("\nError:\n{}", stderr));
    }
    output
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
        print!("Output:\n{}", get_full_output(&res));
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
        print!("Output:\n{}", get_full_output(&res));
        panic!("Failed to build project");
    }
}

fn test_create_module(
    module_name: &str,
    plugin_type: PluginType,
    description: &str,
    nodos_version: &str,
) {
    let mut test = WorkspaceGen::new_random();

    // Install nodos and verify cmake generation and build works correctly.
    let res = GetCommand {}.run_get(
        &mut test.workspace,
        &"nodos".to_string(),
        &nodos_version.to_string(),
        true,
        true,
        false,
    );
    if let Err(e) = res {
        panic!("Failed to install nodos: {}", e);
    }

    // Copy self to the workspace
    let nosman_path = env!("CARGO_BIN_EXE_nosman");
    // Set the target executable name
    let target_executable_name = format!("nodos{}", std::env::consts::EXE_SUFFIX);
    std::fs::copy(
        nosman_path,
        &test.workspace.root.join(target_executable_name),
    )
    .expect("Failed to copy nosman to workspace");

    // Create the module
    let module_dir = test.workspace.root.join("Module").join(module_name);
    let nodos_version = SemVer::parse_from_str(nodos_version);
    CreateCommand {}
        .run_create(
            &mut test.workspace,
            module_name,
            plugin_type.clone(),
            LangTool::CppCMake,
            &module_dir,
            Vec::new(), // No dependencies
            description,
            nodos_version.clone(),
        )
        .expect(&format!("Failed to create {:?}", plugin_type));

    // Verify the module was created correctly
    assert!(
        module_dir.exists(),
        "{:?} directory was not created",
        plugin_type
    );

    // Check manifest file exists with correct extension
    let extension = get_plugin_manifest_file_ext(Option::from(&nodos_version), &plugin_type);
    let manifest_path = module_dir.join(format!("{}.{}", module_name, extension));
    assert!(
        manifest_path.exists(),
        "{:?} manifest file was not created",
        plugin_type
    );

    test_cmake_build(&test);
}

#[test]
fn create_plugin_1_3() {
    test_create_module(
        "test.example",
        PluginType::Default,
        "Test plugin description",
        "1.3",
    );
}

#[test]
fn create_subsystem_1_3() {
    test_create_module(
        "test.sys.example",
        PluginType::SubsystemLegacy,
        "Test subsystem description",
        "1.3",
    );
}

#[test]
fn create_plugin_1_4() {
    test_create_module(
        "test.example",
        PluginType::Default,
        "Test plugin description",
        "1.4",
    );
}

#[test]
fn create_subsystem_1_4() {
    let mut test = WorkspaceGen::new_random();
    let module_name = "test.sys.example";
    let module_dir = test.workspace.root.join("Module").join(module_name);
    let result = CreateCommand {}.run_create(
        &mut test.workspace,
        module_name,
        PluginType::SubsystemLegacy,
        LangTool::CppCMake,
        &module_dir,
        Vec::new(),
        "Test subsystem description",
        Some(SemVer::new(1, Some(4), None, None)),
    );
    assert!(result.is_err(), "Expected subsystem creation to fail for Nodos 1.4");
}

// Helper to read node definition JSON
fn read_node_def_json(node_def_path: &PathBuf) -> serde_json::Value {
    let content =
        std::fs::read_to_string(node_def_path).expect("Failed to read node definition file");
    serde_json::from_str(&content).expect("Failed to parse node definition JSON")
}

fn test_node_add_remove(version: SemVer) {
    let mut test = WorkspaceGen::new_random();
    let module_name = format!("test{}.plugin", version.major);
    let module_dir = test.workspace.root.join("Module").join(&module_name);
    // Create plugin
    CreateCommand {}
        .run_create(
            &mut test.workspace,
            &module_name,
            PluginType::Default,
            LangTool::CppCMake,
            &module_dir,
            Vec::new(),
            "Node test plugin",
            Some(version.clone()),
        )
        .expect("Failed to create plugin");
    // Add node
    let node_class = "MyNode";
    NodeCommand {}
        .run_node(
            &mut test.workspace,
            &module_name,
            &node_class.to_string(),
            false,
            Some("MyNode".to_string()),
            Some("A test node".to_string()),
            Some("TestCategory".to_string()),
            false,
            Some(version.clone()),
        )
        .expect("Failed to add node");
    // Find node definition file
    let plugin = test
        .workspace
        .get_or_select_package(&module_name)
        .unwrap();
    let manifest = plugin.read_manifest();
    let node_def_path;
    if version < NODOS_1_4 {
        let node_defs = manifest["node_definitions"]
            .as_array()
            .expect("No node_definitions");
        assert!(!node_defs.is_empty());
        node_def_path = plugin.get_package_root().join(node_defs[0].as_str().unwrap());
    } else {
        // For Nodos 1.4 and later, node definitions are stored in a dedicated directory by default
        let defs = test.workspace.get_node_definitions(
            &format!("{}.MyNode", module_name).to_string(),
            &Some(version.clone()),
        );
        assert!(
            !defs.is_empty(),
            "No node definitions found for {}.{}",
            module_name,
            node_class
        );
        node_def_path = defs[0].defined_in.clone();
    }

    assert!(node_def_path.exists());
    let json = read_node_def_json(&node_def_path);
    let nodes = json["nodes"].as_array().unwrap();
    assert_eq!(
        nodes[0]["class_name"],
        format!("{}.{}", module_name, node_class)
    );
    // Remove node
    NodeCommand {}
        .run_node(
            &mut test.workspace,
            &module_name,
            &node_class.to_string(),
            true,
            None,
            None,
            None,
            false,
            Some(version.clone()),
        )
        .expect("Failed to remove node");
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
    CreateCommand {}
        .run_create(
            &mut test.workspace,
            &module_name,
            PluginType::Default,
            LangTool::CppCMake,
            &module_dir,
            Vec::new(),
            "Pin test plugin",
            Some(version.clone()),
        )
        .expect("Failed to create plugin");
    // Add node
    let node_class = "SomeNode";
    NodeCommand {}
        .run_node(
            &mut test.workspace,
            &module_name,
            &node_class.to_string(),
            false,
            Some(node_class.to_string()),
            Some("A node for pin test".to_string()),
            Some("PinCategory".to_string()),
            false,
            Some(version.clone()),
        )
        .expect("Failed to add node");
    // Find node definition file
    let plugin = test
        .workspace
        .get_or_select_package(&module_name)
        .unwrap();
    let manifest = plugin.read_manifest();
    let node_def_path;
    if version < NODOS_1_4 {
        let node_defs = manifest["node_definitions"]
            .as_array()
            .expect("No node_definitions");
        assert!(!node_defs.is_empty());
        node_def_path = plugin.get_package_root().join(node_defs[0].as_str().unwrap());
    } else {
        // For Nodos 1.4 and later, node definitions are stored in a dedicated directory by default
        let defs = test.workspace.get_node_definitions(
            &format!("{}.{}", module_name, node_class).to_string(),
            &Some(version.clone()),
        );
        assert!(
            !defs.is_empty(),
            "No node definitions found for {}.{}",
            module_name,
            node_class
        );
        node_def_path = defs[0].defined_in.clone();
    }
    // Add pin
    PinCommand {}
        .run_pin(
            &test.workspace,
            &format!("{}.{}", module_name, node_class),
            &"myPin".to_string(),
            false,
            Some(&"INPUT_PIN".to_string()),
            Some(&"INPUT_PIN_ONLY".to_string()),
            Some(&"float".to_string()),
            Some(version.clone()),
        )
        .expect("Failed to add pin");
    // Verify pin exists
    let json = read_node_def_json(&node_def_path);
    let pins = json["nodes"][0]["node"]["pins"].as_array().unwrap();
    assert!(pins.iter().any(|p| p["name"] == "myPin"));
    // Remove pin
    PinCommand {}
        .run_pin(
            &test.workspace,
            &format!("{}.{}", module_name, node_class),
            &"myPin".to_string(),
            true,
            None,
            None,
            None,
            Some(version.clone()),
        )
        .expect("Failed to remove pin");
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

fn install_nodos_to_workspace(test: &mut WorkspaceGen, nodos_version: &str) {
    let res = GetCommand {}.run_get(
        &mut test.workspace,
        &"nodos".to_string(),
        &nodos_version.to_string(),
        true,
        true,
        false,
    );
    if let Err(e) = res {
        panic!("Failed to install nodos {}: {}", nodos_version, e);
    }
}

#[test]
fn get_preserves_modules_when_clean_modules_false() {
    let mut test = WorkspaceGen::new_random();
    let module_dir = test.workspace.root.join("Module").join("keep.module");
    fs::create_dir_all(&module_dir).expect("Failed to create module dir");
    let keep_file = module_dir.join("keep.txt");
    fs::write(&keep_file, "keep").expect("Failed to write module file");

    let res = GetCommand {}.run_get(
        &mut test.workspace,
        &"nodos".to_string(),
        &"1.4".to_string(),
        true,
        true,
        false,
    );
    assert!(res.is_ok(), "Failed to install nodos for module preservation test");
    assert!(module_dir.exists(), "Module dir should remain when clean_modules is false");
    assert!(keep_file.exists(), "Module file should remain when clean_modules is false");
}

#[test]
fn get_prompts_when_deletions_exist() {
    struct PromptGuard;
    impl Drop for PromptGuard {
        fn drop(&mut self) {
            set_prompt_handler(None);
        }
    }

    let mut test = WorkspaceGen::new_random();
    let stale_dir = test.workspace.root.join("Stale");
    fs::create_dir_all(&stale_dir).expect("Failed to create stale dir");
    fs::write(stale_dir.join("stale.txt"), "stale").expect("Failed to write stale file");

    let seen = Arc::new(Mutex::new(Vec::<String>::new()));
    let seen_clone = Arc::clone(&seen);
    set_prompt_handler(Some(Box::new(move |question, _default, _dont_ask| {
        seen_clone.lock().unwrap().push(question.to_string());
        false
    })));
    let _guard = PromptGuard;

    let res = GetCommand {}.run_get(
        &mut test.workspace,
        &"nodos".to_string(),
        &"1.4".to_string(),
        true,
        false,
        false,
    );
    assert!(res.is_err(), "Expected get to abort when prompt is declined");
    let seen = seen.lock().unwrap();
    assert!(
        seen.iter().any(|q| q.contains("will delete")),
        "Expected deletion prompt to be shown"
    );
}

fn test_sdk_info(test: &mut WorkspaceGen, version: &str, sdk_type: &str) {
    install_nodos_to_workspace(test, version);
    // Get the actual SDK version from the installed nodos
    let engines = nosman::nosman::command::sdk_info::get_engine_sdk_infos(&test.workspace)
        .expect("Failed to get engine SDK infos");
    assert!(!engines.is_empty(), "No engines found");

    let sdk_version = match sdk_type {
        "plugin" => &engines[0].plugin_sdk_version,
        "process" => &engines[0].process_sdk_version,
        _ => panic!("Invalid SDK type: {}", sdk_type),
    };
    
    let major_minor = sdk_version
        .split('.')
        .take(2)
        .collect::<Vec<_>>()
        .join(".");

    let result = SdkInfoCommand {}.run_get_sdk_info(&test.workspace, &major_minor, sdk_type);
    assert!(result.is_ok(), "Failed to get {} SDK info", sdk_type);
}

#[test]
fn sdk_info_plugin_version_1_3() {
    let mut test = WorkspaceGen::new_random();
    test_sdk_info(&mut test, "1.3.0.b4429", "plugin");
}

#[test]
fn sdk_info_process_version_1_3() {
    let mut test = WorkspaceGen::new_random();
    test_sdk_info(&mut test, "1.3.0.b4429", "process");
}

#[test]
fn sdk_info_plugin_version_1_4() {
    let mut test = WorkspaceGen::new_random();
    test_sdk_info(&mut test, "1.4.0.b4431", "plugin");
}

#[test]
fn sdk_info_process_version_1_4() {
    let mut test = WorkspaceGen::new_random();
    test_sdk_info(&mut test, "1.4.0.b4431", "process");
}

#[test]
fn auto_rescan_if_needed_workspace_not_ready() {
    use nosman::nosman::workspace::AutoRescanResult;
    
    // Create a workspace from a path that doesn't have an index file
    let random_string: String = (0..8)
        .map(|_| RNG.lock().unwrap().random_range(b'a'..=b'z'))
        .map(char::from)
        .collect();
    let test_path = PathBuf::from(format!("./test_workspaces/{}", random_string));
    
    // Create the directory but don't initialize it with recreate()
    fs::create_dir_all(&test_path).expect("Failed to create test directory");
    
    let mut workspace = Workspace::from_root(&test_path);
    assert!(!workspace.ready(), "Workspace should not be ready initially");
    
    // auto_rescan_if_needed should do nothing when workspace is not ready
    let result = workspace.auto_rescan_if_needed();
    assert!(result.is_ok(), "auto_rescan_if_needed should succeed when workspace is not ready");
    
    match result.unwrap() {
        AutoRescanResult::NoActionNeeded => {
            // This is expected - no action should be taken when workspace is not ready
        },
        other => panic!("Expected NoActionNeeded, got {:?}", other),
    }
    
    // Workspace should still not be ready since no rescan was performed
    assert!(!workspace.ready(), "Workspace should still not be ready after auto_rescan_if_needed");
}

#[test]
fn auto_rescan_if_needed_no_index_file() {
    use nosman::nosman::workspace::AutoRescanResult;
    
    let mut test = WorkspaceGen::new_random();
    
    // Remove the index file to force a full rescan
    let index_path = test.workspace.get_nosman_index_filepath();
    if index_path.exists() {
        fs::remove_file(&index_path).expect("Failed to remove index file");
    }
    
    let result = test.workspace.auto_rescan_if_needed();
    assert!(result.is_ok(), "auto_rescan_if_needed should succeed when index file is missing");
    
    match result.unwrap() {
        AutoRescanResult::FullRescanMissingIndex => {
            // This is expected
        },
        other => panic!("Expected FullRescanMissingIndex, got {:?}", other),
    }
    
    assert!(index_path.exists(), "Index file should be recreated after auto_rescan_if_needed");
}

#[test]
fn auto_rescan_if_needed_no_changes() {
    use nosman::nosman::workspace::AutoRescanResult;
    
    let mut test = WorkspaceGen::new_random();
    
    // Install a package to have some content in the workspace
    let package_name = "nos.sys.vulkan";
    let version = String::from("6.2.1.b612");
    InstallCommand {}
        .run_install(
            &mut test.workspace,
            package_name,
            Some(&version),
            &Some(PathBuf::from(".")),
            None,
            InstallFlags::UpdatePackageIndex | InstallFlags::WithoutDependencies,
        )
        .expect("Failed to install package");
    
    // Save workspace to establish baseline
    test.workspace.save().expect("Failed to save workspace");
    
    // auto_rescan_if_needed should do nothing when no changes detected
    let result = test.workspace.auto_rescan_if_needed();
    assert!(result.is_ok(), "auto_rescan_if_needed should succeed when no changes");
    
    match result.unwrap() {
        AutoRescanResult::NoActionNeeded => {
            // This is expected
        },
        other => panic!("Expected NoActionNeeded, got {:?}", other),
    }
    
    // Verify package is still there
    let versions = test.workspace.get_packages(package_name);
    assert_eq!(versions.len(), 1, "Package should still be present after auto_rescan_if_needed");
}

#[test]
fn auto_rescan_if_needed_missing_manifest() {
    use nosman::nosman::workspace::AutoRescanResult;
    
    let mut test = WorkspaceGen::new_random();
    
    // Install a package to have some content
    let package_name = "nos.sys.vulkan";
    let version = String::from("6.2.1.b612");
    InstallCommand {}
        .run_install(
            &mut test.workspace,
            package_name,
            Some(&version),
            &Some(PathBuf::from(".")),
            None,
            InstallFlags::UpdatePackageIndex | InstallFlags::WithoutDependencies,
        )
        .expect("Failed to install package");
    
    // Save workspace
    test.workspace.save().expect("Failed to save workspace");
    
    // Find and remove a manifest file
    let mut manifest_path = None;
    for (_name, versions) in &test.workspace.packages {
        for (_version, package) in versions {
            let full_manifest_path = test.workspace.root.join(&package.manifest_path);
            if full_manifest_path.exists() {
                manifest_path = Some(full_manifest_path);
                break;
            }
        }
        if manifest_path.is_some() {
            break;
        }
    }
    
    if let Some(path) = manifest_path {
        fs::remove_file(&path).expect("Failed to remove manifest file");
        
        // auto_rescan_if_needed should perform full rescan when manifest is missing
        let result = test.workspace.auto_rescan_if_needed();
        assert!(result.is_ok(), "auto_rescan_if_needed should succeed when manifest is missing");
        
        match result.unwrap() {
            AutoRescanResult::FullRescanMissingManifests => {
                // This is expected
            },
            other => panic!("Expected FullRescanMissingManifests, got {:?}", other),
        }
    }
}

#[test]
fn auto_rescan_if_needed_updated_manifest() {
    use std::time::Duration;
    use nosman::nosman::workspace::AutoRescanResult;
    
    let mut test = WorkspaceGen::new_random();
    
    // Install a package to have some content
    let package_name = "nos.sys.vulkan";
    let version = String::from("6.2.1.b612");
    InstallCommand {}
        .run_install(
            &mut test.workspace,
            package_name,
            Some(&version),
            &Some(PathBuf::from(".")),
            None,
            InstallFlags::UpdatePackageIndex | InstallFlags::WithoutDependencies,
        )
        .expect("Failed to install package");
    
    // Save workspace to establish baseline
    test.workspace.save().expect("Failed to save workspace");
    
    // Sleep briefly to ensure time difference
    std::thread::sleep(Duration::from_millis(100));
    
    // Find and touch a manifest file to update its modification time
    let mut manifest_path = None;
    for (_name, versions) in &test.workspace.packages {
        for (_version, package) in versions {
            let full_manifest_path = test.workspace.root.join(&package.manifest_path);
            if full_manifest_path.exists() {
                manifest_path = Some(full_manifest_path);
                break;
            }
        }
        if manifest_path.is_some() {
            break;
        }
    }
    
    if let Some(path) = manifest_path {
        // Update the file's modification time by reading and writing it
        let content = fs::read_to_string(&path).expect("Failed to read manifest file");
        fs::write(&path, content).expect("Failed to write manifest file");
        
        // auto_rescan_if_needed should handle updated manifest
        let result = test.workspace.auto_rescan_if_needed();
        assert!(result.is_ok(), "auto_rescan_if_needed should succeed when manifest is updated");
        
        match result.unwrap() {
            AutoRescanResult::PartialRescanUpdatedManifests(folders) => {
                assert!(!folders.is_empty(), "Should have at least one updated folder");
            },
            other => panic!("Expected PartialRescanUpdatedManifests, got {:?}", other),
        }
    }
}

#[test]
fn install_with_only_major_version() {
    let mut test = WorkspaceGen::new_random();
    let package_name = "nos.sys.vulkan";
    let version = String::from("6");
    InstallCommand {}
        .run_install(
            &mut test.workspace,
            package_name,
            Some(&version),
            &Some(PathBuf::from(".")),
            None,
            InstallFlags::UpdatePackageIndex | InstallFlags::WithoutDependencies,
        )
        .unwrap_or_else(|_| panic!("Failed to install {}", package_name));
    let versions = test.workspace.get_packages(package_name);
    assert_eq!(versions.len(), 1);
}

#[test]
fn test_matches_prefix() {
    // Test major only prefix (6 matches 6.x.x)
    let prefix = SemVer::parse_from_str("6").unwrap();
    assert!(SemVer::parse_from_str("6.0.0").unwrap().matches_prefix(&prefix));
    assert!(SemVer::parse_from_str("6.30.1").unwrap().matches_prefix(&prefix));
    assert!(SemVer::parse_from_str("6.99.99").unwrap().matches_prefix(&prefix));
    assert!(!SemVer::parse_from_str("5.99.99").unwrap().matches_prefix(&prefix));
    assert!(!SemVer::parse_from_str("7.0.0").unwrap().matches_prefix(&prefix));

    // Test major.minor prefix (6.30 matches 6.30.x)
    let prefix = SemVer::parse_from_str("6.30").unwrap();
    assert!(SemVer::parse_from_str("6.30.0").unwrap().matches_prefix(&prefix));
    assert!(SemVer::parse_from_str("6.30.1").unwrap().matches_prefix(&prefix));
    assert!(SemVer::parse_from_str("6.30.99").unwrap().matches_prefix(&prefix));
    assert!(!SemVer::parse_from_str("6.29.99").unwrap().matches_prefix(&prefix));
    assert!(!SemVer::parse_from_str("6.31.0").unwrap().matches_prefix(&prefix));

    // Test major.minor.patch prefix (6.30.1 matches 6.30.1.x)
    let prefix = SemVer::parse_from_str("6.30.1").unwrap();
    assert!(SemVer::parse_from_str("6.30.1").unwrap().matches_prefix(&prefix));
    assert!(SemVer::parse_from_str("6.30.1.b709").unwrap().matches_prefix(&prefix));
    assert!(SemVer::parse_from_str("6.30.1.b999").unwrap().matches_prefix(&prefix));
    assert!(!SemVer::parse_from_str("6.30.0").unwrap().matches_prefix(&prefix));
    assert!(!SemVer::parse_from_str("6.30.2").unwrap().matches_prefix(&prefix));

    // Test full version prefix (6.30.1.b709 matches exactly 6.30.1.b709)
    let prefix = SemVer::parse_from_str("6.30.1.b709").unwrap();
    assert!(SemVer::parse_from_str("6.30.1.b709").unwrap().matches_prefix(&prefix));
    assert!(!SemVer::parse_from_str("6.30.1.b708").unwrap().matches_prefix(&prefix));
    assert!(!SemVer::parse_from_str("6.30.1.b710").unwrap().matches_prefix(&prefix));
}

#[test]
fn info_by_package_and_version() {
    let mut test = WorkspaceGen::new_random();
    let package_name = "nos.sys.vulkan";
    let version = String::from("6.2.1.b612");
    
    // Install package
    InstallCommand {}
        .run_install(
            &mut test.workspace,
            package_name,
            Some(&version),
            &Some(PathBuf::from(".")),
            None,
            InstallFlags::UpdatePackageIndex | InstallFlags::WithoutDependencies,
        )
        .expect("Failed to install package");
    
    // Get info by package name and version
    let query = nosman::nosman::command::info::PackageQuery {
        name: package_name,
        version_prefix: &version,
    };
    let result = InfoCommand {}.run_get_info(
        &mut test.workspace,
        Some(&query),
        false,
        None,
    );
    
    assert!(result.is_ok(), "Failed to get package info by name and version");
}

#[test]
fn info_by_manifest_path() {
    let mut test = WorkspaceGen::new_random();
    let package_name = "nos.sys.vulkan";
    let version = String::from("6.2.1.b612");
    
    // Install package
    InstallCommand {}
        .run_install(
            &mut test.workspace,
            package_name,
            Some(&version),
            &Some(PathBuf::from(".")),
            None,
            InstallFlags::UpdatePackageIndex | InstallFlags::WithoutDependencies,
        )
        .expect("Failed to install package");
    
    // Get the manifest path
    let package = test.workspace.get_package(package_name, &version)
        .expect("Package not found");
    let manifest_path = test.workspace.root.join(&package.manifest_path);
    
    // Get info by manifest path
    let result = InfoCommand {}.run_get_info(
        &mut test.workspace,
        None,
        false,
        Some(manifest_path.to_str().unwrap()),
    );
    
    assert!(result.is_ok(), "Failed to get package info by manifest path");
}

#[test]
fn info_error_both_package_and_manifest() {
    let mut test = WorkspaceGen::new_random();
    let package_name = "nos.sys.vulkan";
    let version = String::from("6.2.1.b612");
    
    // Install package
    InstallCommand {}
        .run_install(
            &mut test.workspace,
            package_name,
            Some(&version),
            &Some(PathBuf::from(".")),
            None,
            InstallFlags::UpdatePackageIndex | InstallFlags::WithoutDependencies,
        )
        .expect("Failed to install package");
    
    // Get the manifest path
    let package = test.workspace.get_package(package_name, &version)
        .expect("Package not found");
    let manifest_path = test.workspace.root.join(&package.manifest_path);
    
    // Try to use both package name and manifest path (should fail)
    let query = nosman::nosman::command::info::PackageQuery {
        name: package_name,
        version_prefix: &version,
    };
    let result = InfoCommand {}.run_get_info(
        &mut test.workspace,
        Some(&query),
        false,
        Some(manifest_path.to_str().unwrap()),
    );
    
    assert!(result.is_err(), "Should fail when both package and manifest are provided");
}

#[test]
fn info_error_no_arguments() {
    let mut test = WorkspaceGen::new_random();
    
    // Try to get info without any arguments (should fail)
    let result = InfoCommand {}.run_get_info(
        &mut test.workspace,
        None,
        false,
        None,
    );
    
    assert!(result.is_err(), "Should fail when no arguments are provided");
}

#[test]
fn dev_init_cmake_copies_toolchain() {
    let test = WorkspaceGen::new_random();

    DevInitCommand {}
        .run_init(&test.workspace, "cmake")
        .expect("dev init cmake failed");

    let cmake_root = test.workspace.root.join("Toolchain").join("CMake");
    assert!(cmake_root.join("CMakeLists.txt").exists());
    assert!(cmake_root.join("Scripts").join("Projects.cmake").exists());
}

#[test]
fn dev_init_cmake_overwrites_when_exists() {
    let test = WorkspaceGen::new_random();

    DevInitCommand {}
        .run_init(&test.workspace, "cmake")
        .expect("dev init cmake failed");

    let cmake_root = test.workspace.root.join("Toolchain").join("CMake");
    let cmake_list_path = cmake_root.join("CMakeLists.txt");
    fs::write(&cmake_list_path, "modified").expect("Failed to modify CMakeLists.txt");

    DevInitCommand {}
        .run_init(&test.workspace, "cmake")
        .expect("dev init cmake reinit failed");

    let expected = fs::read_to_string("../CMake/CMakeLists.txt")
        .expect("Failed to read expected CMakeLists.txt");
    let actual = fs::read_to_string(&cmake_list_path)
        .expect("Failed to read CMakeLists.txt after reinit");
    assert_eq!(actual, expected);
}
