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
            Some(version.clone()),
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
            Some(version.clone()),
        )
        .expect("Failed to remove pin");
    let json = read_node_def_json(&node_def_path);
    let pins = json["nodes"][0]["node"]["pins"].as_array().unwrap();
    assert!(!pins.iter().any(|p| p["name"] == "myPin"));
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
