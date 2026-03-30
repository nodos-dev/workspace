mod support;

use nosman::nosman::command::create::CreateCommand;
use nosman::nosman::index::{PluginType, SemVer};
use nosman::nosman::lang_tool::LangTool;
use support::test_create_plugin;

#[test]
fn create_plugin_1_3() {
    let mut test = workspace!();
    test_create_plugin(
        &mut test.workspace,
        "test.example",
        PluginType::Default,
        "Test plugin description",
        "1.3",
    );
}

#[test]
fn create_subsystem_1_3() {
    let mut test = workspace!();
    test_create_plugin(
        &mut test.workspace,
        "test.sys.example",
        PluginType::SubsystemLegacy,
        "Test subsystem description",
        "1.3",
    );
}

#[test]
fn create_plugin_1_4() {
    let mut test = workspace!();
    test_create_plugin(
        &mut test.workspace,
        "test.example",
        PluginType::Default,
        "Test plugin description",
        "1.4",
    );
}

#[test]
fn create_subsystem_1_4() {
    let mut test = workspace!();
    let module_name = "test.sys.example";
    let module_dir = test.workspace.root.join("Module").join(module_name);
    let result = CreateCommand {}.run_create(
        &mut test.workspace,
        module_name,
        Some(PluginType::SubsystemLegacy),
        LangTool::CppCMake,
        &module_dir,
        Vec::new(),
        "Test subsystem description",
        Some(SemVer::new(1, Some(4), None, None)),
    );
    assert!(result.is_err(), "Expected subsystem creation to fail for Nodos 1.4");
}
