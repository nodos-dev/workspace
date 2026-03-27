#![allow(dead_code)]

use log::info;
use nosman::nosman::command::create::CreateCommand;
use nosman::nosman::command::get::GetCommand;
use nosman::nosman::index::{PluginType, SemVer};
use nosman::nosman::lang_tool::LangTool;
use nosman::nosman::package::get_plugin_manifest_file_ext;
use nosman::nosman::workspace::{OutputMode, Workspace};
use std::path::PathBuf;
use std::process::Output;

pub struct WorkspaceGen {
    pub workspace: Workspace,
}

impl WorkspaceGen {
    pub fn new(name: &str) -> Self {
        let path = PathBuf::from(format!("./test_workspaces/{}", name));
        if path.exists() {
            std::fs::remove_dir_all(&path).expect("Failed to remove existing test workspace");
        }
        let mut ws = Workspace::from_root(&path);
        ws.recreate().expect("Failed to create test workspace");
        ws.push_output_mode(OutputMode::Silent);
        WorkspaceGen { workspace: ws }
    }
}

/// Returns a fresh, empty directory path for tests that need an uninitialized workspace.
/// The directory is created but no workspace files are written into it.
pub fn uninitialized_workspace_path(name: &str) -> PathBuf {
    let path = PathBuf::from(format!("./test_workspaces/{}", name));
    if path.exists() {
        std::fs::remove_dir_all(&path).expect("Failed to remove existing test workspace");
    }
    std::fs::create_dir_all(&path).expect("Failed to create test directory");
    path
}

/// Creates a [`WorkspaceGen`] whose name is automatically derived from the calling
/// test function — no string literal needed.
///
/// ```rust
/// #[test]
/// fn my_test() {
///     let test = workspace!(); // name = "my_test"
/// }
/// ```
#[macro_export]
macro_rules! workspace {
    () => {{
        fn f() {}
        fn type_name_of<T>(_: T) -> &'static str {
            ::std::any::type_name::<T>()
        }
        let full = type_name_of(f);
        // full ends with "::my_test::f"; strip "::f" then take the last component
        let without_f = &full[..full.len() - 3];
        let name = without_f.rsplit("::").next().unwrap_or(without_f);
        $crate::support::WorkspaceGen::new(name)
    }};
}

/// Same trick as [`workspace!`] but returns a raw directory path for tests that
/// need an uninitialized workspace (no `Workspace::recreate()` called).
#[macro_export]
macro_rules! uninitialized_workspace {
    () => {{
        fn f() {}
        fn type_name_of<T>(_: T) -> &'static str {
            ::std::any::type_name::<T>()
        }
        let full = type_name_of(f);
        let without_f = &full[..full.len() - 3];
        let name = without_f.rsplit("::").next().unwrap_or(without_f);
        $crate::support::uninitialized_workspace_path(name)
    }};
}

#[ctor::ctor]
fn init() {
    env_logger::Builder::from_default_env()
        .filter_level(log::LevelFilter::Info)
        .init();
    info!("Starting nosman tests");
}

// Workspace cleanup is not performed automatically. With nextest, each test
// runs in its own process, so a dtor that removes ./test_workspaces would
// destroy directories still in use by parallel tests. Clean up externally
// (e.g. rm -rf ./test_workspaces) after the test run if needed.

pub fn get_full_output(res: &Output) -> String {
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

pub fn test_cmake_build(test_workspace: &Workspace) {
    let res = std::process::Command::new("cmake")
        .current_dir(&test_workspace.root)
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
        .current_dir(&test_workspace.root)
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

pub fn test_create_plugin(
    test_workspace: &mut Workspace,
    plugin_name: &str,
    plugin_type: PluginType,
    description: &str,
    nodos_version: &str,
) {
    let res = GetCommand {}.run_get(
        test_workspace,
        &"nodos".to_string(),
        &nodos_version.to_string(),
        true,
        true,
        false,
    );
    if let Err(e) = res {
        panic!("Failed to install nodos: {}", e);
    }

    let nosman_path = env!("CARGO_BIN_EXE_nosman");
    let target_executable_name = format!("nodos{}", std::env::consts::EXE_SUFFIX);
    std::fs::copy(
        nosman_path,
        &test_workspace.root.join(target_executable_name),
    )
    .expect("Failed to copy nosman to workspace");

    let module_dir = test_workspace.root.join("Module").join(plugin_name);
    let nodos_version = SemVer::parse_from_str(nodos_version);
    CreateCommand {}
        .run_create(
            test_workspace,
            plugin_name,
            Some(plugin_type.clone()),
            LangTool::CppCMake,
            &module_dir,
            Vec::new(),
            description,
            nodos_version.clone(),
        )
        .expect(&format!("Failed to create {:?}", plugin_type));

    assert!(
        module_dir.exists(),
        "{:?} directory was not created",
        plugin_type
    );

    let extension = get_plugin_manifest_file_ext(Option::from(&nodos_version), &plugin_type);
    let manifest_path = module_dir.join(format!("{}.{}", plugin_name, extension));
    assert!(
        manifest_path.exists(),
        "{:?} manifest file was not created",
        plugin_type
    );

    test_cmake_build(test_workspace);
}
