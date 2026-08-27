mod support;

use nosman::nosman::command::create::CreateCommand;
use nosman::nosman::command::node::NodeCommand;
use nosman::nosman::command::pin::PinCommand;
use nosman::nosman::common::NODOS_1_4;
use nosman::nosman::index::{PluginType, SemVer};
use nosman::nosman::lang_tool::LangTool;
use std::path::PathBuf;
use support::{test_create_plugin, WorkspaceGen};

fn read_node_def_json(node_def_path: &PathBuf) -> serde_json::Value {
    let content =
        std::fs::read_to_string(node_def_path).expect("Failed to read node definition file");
    serde_json::from_str(&content).expect("Failed to parse node definition JSON")
}

fn test_node_add_remove(mut test: WorkspaceGen, version: SemVer) {
    let module_name = format!("test{}.plugin", version.major);
    if version < NODOS_1_4 {
        let module_dir = test.workspace.root.join("Module").join(&module_name);
        CreateCommand {}
            .run_create(
                &mut test.workspace,
                &module_name,
                Some(PluginType::Default),
                LangTool::CppCMake,
                &module_dir,
                Vec::new(),
                "Node test plugin",
                Some(version.clone()),
            )
            .expect("Failed to create plugin");
    } else {
        test_create_plugin(
            &mut test.workspace,
            &module_name,
            PluginType::Default,
            "Node test plugin",
            &version.to_string(),
        )
    };

    let node_class = "MyNode";
    // No version passed on purpose: the plugin manifest has to be enough, like it is on the CLI.
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
            None,
        )
        .expect("Failed to add node");
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
        let defs = test
            .workspace
            .get_node_definitions(&format!("{}.MyNode", module_name).to_string());
        assert!(
            !defs.is_empty(),
            "No node definitions found for {}.{}",
            module_name,
            node_class
        );
        node_def_path = defs[0].defined_in.clone();
    }
    let expected_ext = if version < NODOS_1_4 { "nosdef" } else { "nosnode" };
    assert_eq!(
        node_def_path.extension().and_then(|e| e.to_str()),
        Some(expected_ext),
        "Unexpected node definition file for Nodos {}: {}",
        version,
        node_def_path.display()
    );

    assert!(node_def_path.exists());
    let json = read_node_def_json(&node_def_path);
    let nodes = json["nodes"].as_array().unwrap();
    assert_eq!(
        nodes[0]["class_name"],
        format!("{}.{}", module_name, node_class)
    );
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
            None,
        )
        .expect("Failed to remove node");
    assert!(!node_def_path.exists());
}

fn test_pin_add_remove(mut test: WorkspaceGen, version: SemVer) {
    let module_name = format!("test{}.plugin", version.major);
    if version < NODOS_1_4 {
        let module_dir = test.workspace.root.join("Module").join(&module_name);
        CreateCommand {}
            .run_create(
                &mut test.workspace,
                &module_name,
                Some(PluginType::Default),
                LangTool::CppCMake,
                &module_dir,
                Vec::new(),
                "Pin test plugin",
                Some(version.clone()),
            )
            .expect("Failed to create plugin");
    } else {
        test_create_plugin(
            &mut test.workspace,
            &module_name,
            PluginType::Default,
            "Pin test plugin",
            &version.to_string(),
        )
    };
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
            None,
        )
        .expect("Failed to add node");
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
        let defs = test
            .workspace
            .get_node_definitions(&format!("{}.{}", module_name, node_class).to_string());
        assert!(
            !defs.is_empty(),
            "No node definitions found for {}.{}",
            module_name,
            node_class
        );
        node_def_path = defs[0].defined_in.clone();
    }
    PinCommand {}
        .run_pin(
            &test.workspace,
            &format!("{}.{}", module_name, node_class),
            &"myPin".to_string(),
            false,
            Some(&"INPUT_PIN".to_string()),
            Some(&"INPUT_PIN_ONLY".to_string()),
            Some(&"float".to_string()),
        )
        .expect("Failed to add pin");
    let json = read_node_def_json(&node_def_path);
    let pins = json["nodes"][0]["node"]["pins"].as_array().unwrap();
    assert!(pins.iter().any(|p| p["name"] == "myPin"));
    PinCommand {}
        .run_pin(
            &test.workspace,
            &format!("{}.{}", module_name, node_class),
            &"myPin".to_string(),
            true,
            None,
            None,
            None,
        )
        .expect("Failed to remove pin");
    let json = read_node_def_json(&node_def_path);
    let pins = json["nodes"][0]["node"]["pins"].as_array().unwrap();
    assert!(!pins.iter().any(|p| p["name"] == "myPin"));
}

// Nodos reads hide_in_context_menu next to class_name. Written inside menu_info it is
// silently dropped, so --hide looks like it worked and the node still shows up.
#[test]
fn node_hide_flag_sits_next_to_class_name() {
    let mut test = workspace!();
    let version = SemVer::new(1, Some(3), None, None);
    let module_name = "test1.plugin".to_string();
    let module_dir = test.workspace.root.join("Module").join(&module_name);
    CreateCommand {}
        .run_create(
            &mut test.workspace,
            &module_name,
            Some(PluginType::Default),
            LangTool::CppCMake,
            &module_dir,
            Vec::new(),
            "Hidden node test plugin",
            Some(version),
        )
        .expect("Failed to create plugin");

    NodeCommand {}
        .run_node(
            &mut test.workspace,
            &module_name,
            &"HiddenNode".to_string(),
            false,
            Some("HiddenNode".to_string()),
            Some("A hidden test node".to_string()),
            Some("TestCategory".to_string()),
            true,
            None,
        )
        .expect("Failed to add node");

    let plugin = test.workspace.get_or_select_package(&module_name).unwrap();
    let manifest = plugin.read_manifest();
    let node_defs = manifest["node_definitions"]
        .as_array()
        .expect("No node_definitions");
    let node_def_path = plugin.get_package_root().join(node_defs[0].as_str().unwrap());
    let json = read_node_def_json(&node_def_path);
    let node = &json["nodes"][0];
    assert_eq!(node["hide_in_context_menu"], true);
    assert!(
        node["menu_info"]["hide_in_context_menu"].is_null(),
        "hide_in_context_menu must not be written inside menu_info"
    );
}

#[test]
fn node_add_remove_1_3() {
    test_node_add_remove(workspace!(), SemVer::new(1, Some(3), None, None));
}

#[test]
fn node_add_remove_1_4() {
    test_node_add_remove(workspace!(), SemVer::new(1, Some(4), None, None));
}

#[test]
fn pin_add_remove_1_3() {
    test_pin_add_remove(workspace!(), SemVer::new(1, Some(3), None, None));
}

#[test]
fn pin_add_remove_1_4() {
    test_pin_add_remove(workspace!(), SemVer::new(1, Some(4), None, None));
}

#[test]
fn node_rejects_unsupported_nodos_version() {
    let mut test = workspace!();
    let err = NodeCommand {}
        .run_node(
            &mut test.workspace,
            &"some.plugin".to_string(),
            &"MyNode".to_string(),
            false,
            Some("MyNode".to_string()),
            Some("A test node".to_string()),
            Some("TestCategory".to_string()),
            false,
            Some(SemVer::new(1, Some(2), None, None)),
        )
        .expect_err("Nodos 1.2 should not be accepted");
    assert!(
        err.to_string().contains("Unsupported Nodos version"),
        "Unexpected error: {}",
        err
    );
}
