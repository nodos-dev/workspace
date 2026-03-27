mod support;

use nosman::nosman::command::get::GetCommand;
use nosman::nosman::command::sdk_info::SdkInfoCommand;
use support::WorkspaceGen;

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

fn test_sdk_info(test: &mut WorkspaceGen, version: &str, sdk_type: &str) {
    install_nodos_to_workspace(test, version);
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
    test_sdk_info(&mut test, "1.3.0.b4433", "plugin");
}

#[test]
fn sdk_info_process_version_1_3() {
    let mut test = WorkspaceGen::new_random();
    test_sdk_info(&mut test, "1.3.0.b4433", "process");
}

#[test]
fn sdk_info_plugin_version_1_4() {
    let mut test = WorkspaceGen::new_random();
    test_sdk_info(&mut test, "1.4.0.b4709", "plugin");
}

#[test]
fn sdk_info_process_version_1_4() {
    let mut test = WorkspaceGen::new_random();
    test_sdk_info(&mut test, "1.4.0.b4709", "process");
}
