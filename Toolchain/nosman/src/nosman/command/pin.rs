use clap::{ArgMatches};
use colored::Colorize;
use inquire::{MultiSelect, Select, Text};
use crate::nosman::command::{Command, CommandResult};
use crate::nosman::command::CommandError::{GenericError, InvalidArgumentError};
use crate::nosman::constants;
use crate::nosman::workspace::{Workspace};

pub struct PinCommand {
}

impl PinCommand {

    fn run_pin(&self, node_class_name: &String, pin_name: &String, remove: bool,
               show_as: Option<&String>, can_show_as: Option<&String>, type_name: Option<&String>) -> CommandResult {
        let workspace = Workspace::get()?;
        let mut node_def;
        let node_defs = workspace.get_node_definitions(node_class_name);
        if node_defs.len() == 0 {
            return Err(InvalidArgumentError { message: format!("Node class {} not found", node_class_name) });
        }
        else if node_defs.len() > 1 {
            // Interactive selection
            let selection = Select::new(format!("Multiple node classes found with name {}. Please select one:", node_class_name).as_str(), node_defs)
                .prompt();
            if let Err(e) = selection {
                return Err(GenericError { message: format!("Failed to select node class: {}", e) });
            }
            else {
                node_def = selection.unwrap().clone();
            }
        }
        else {
            node_def = node_defs[0].clone();
        }
        let nodes_json = node_def.node_defs_json.get_mut("nodes").expect(format!("Failed to get 'nodes' field in node class definition: {}", node_class_name).as_str())
            .as_array_mut().expect(format!("Failed to parse 'nodes' field in node class definition: {}", node_class_name).as_str());
        let node_json = nodes_json.get_mut(node_def.index).expect(format!("Failed to get node definition at index {} in node class definition: {}", node_def.index, node_class_name).as_str())
            .as_object_mut().expect(format!("Failed to parse node definition at index {} in node class definition: {}", node_def.index, node_class_name).as_str());
        let pins_json = node_json.get_mut("pins").expect(format!("Failed to get 'pins' field in node definition: {}", node_class_name).as_str())
            .as_array_mut().expect(format!("Failed to parse 'pins' field in node definition: {}", node_class_name).as_str());
        // Remove pin with name
        if remove {
            let mut index = None;
            for (i, pin) in pins_json.iter().enumerate() {
                if pin.get("name").expect("Failed to get 'name' field in pin").as_str().expect("Failed to parse 'name' field in pin") == pin_name {
                    index = Some(i);
                    break;
                }
            }
            if index.is_some() {
                pins_json.remove(index.unwrap());
                serde_json::to_writer_pretty(std::fs::File::create(node_def.defined_in.as_path()).expect("Failed to open node class definition file for writing"), &node_def.node_defs_json)
                    .expect("Failed to write node class definition file");
                println!("{}", format!("Pin '{}' removed from node class '{}'", pin_name, node_class_name).green());
            } else {
                return Err(InvalidArgumentError { message: format!("Pin '{}' not found in node class '{}'", pin_name, node_class_name) });
            }
        } else {
            // Check if a pin with the same name already exists
            for pin in pins_json.iter() {
                if pin.get("name").expect("Failed to get 'name' field in pin").as_str().expect("Failed to parse 'name' field in pin") == pin_name {
                    return Err(InvalidArgumentError { message: format!("Pin '{}' already exists in node class '{}'", pin_name, node_class_name) });
                }
            }
            let mut pin_json = serde_json::Map::new();
            pin_json.insert("name".to_string(), serde_json::Value::String(pin_name.clone()));
            let show_as_out;
            if show_as.is_none() {
                // "input", "output", "property"
                let selection = Select::new("Select pin show-as:", constants::POSSIBLE_SHOW_AS.to_vec())
                    .prompt();
                if let Err(e) = selection {
                    return Err(GenericError { message: format!("Failed to select pin show-as: {}", e) });
                }
                else {
                    show_as_out = selection.unwrap().to_string();
                }
            }
            else {
                show_as_out = show_as.unwrap().clone();
            }
            pin_json.insert("show_as".to_string(), serde_json::Value::String(show_as_out.to_string()));

            let can_show_as_out;
            if can_show_as.is_none() {
                let selection = MultiSelect::new("Select pin can-show-as:", constants::POSSIBLE_SHOW_AS.to_vec())
                    .prompt();
                if let Err(e) = selection {
                    return Err(GenericError { message: format!("Failed to select pin can-show-as: {}", e) });
                }
                else {
                    let mut input = false;
                    let mut output = false;
                    let mut property = false;
                    for pin in selection.unwrap() {
                        match pin {
                            "INPUT_PIN" => input = true,
                            "OUTPUT_PIN" => output = true,
                            "PROPERTY" => property = true,
                            _ => {} // Ignore unknown types
                        }
                    }
                    can_show_as_out = match (input, output, property) {
                        (true, false, false) => "INPUT_PIN_ONLY".to_string(),
                        (false, true, false) => "OUTPUT_PIN_ONLY".to_string(),
                        (false, false, true) => "PROPERTY_ONLY".to_string(),
                        (true, true, false) => "INPUT_OUTPUT".to_string(),
                        (true, true, true) => "INPUT_OUTPUT_PROPERTY".to_string(),
                        (true, false, true) => "INPUT_PIN_OR_PROPERTY".to_string(),
                        (false, true, true) => "OUTPUT_PIN_OR_PROPERTY".to_string(),
                        _ => "INVALID_COMBINATION".to_string(), // Handles empty or invalid combinations
                    }
                }
            }
            else {
                can_show_as_out = can_show_as.unwrap().clone();
            }
            pin_json.insert("can_show_as".to_string(), serde_json::Value::String(can_show_as_out.to_string()));

            let type_name_in;
            if type_name.is_none() {
                type_name_in = Text::new("Enter type name for pin:")
                    .prompt()
                    .expect("Failed to get type name for pin");
            }
            else {
                type_name_in = type_name.unwrap().clone();
            }
            pin_json.insert("type_name".to_string(), serde_json::Value::String(type_name_in.clone()));
            pins_json.push(serde_json::Value::Object(pin_json));

            serde_json::to_writer_pretty(std::fs::File::create(node_def.defined_in.as_path()).expect("Failed to open node class definition file for writing"), &node_def.node_defs_json)
                .expect("Failed to write node class definition file");
            println!("{}", format!("Pin '{}' added to node class '{}'", pin_name, node_class_name).green());
        }
        Ok(true)
    }
}

impl Command for PinCommand {
    fn matched_args<'a>(&self, args : &'a ArgMatches) -> Option<&'a ArgMatches> {
        args.subcommand_matches("pin")
    }

    fn run(&self, args: &ArgMatches) -> CommandResult {
        let node_class_name = args.get_one::<String>("node_class_name").unwrap();
        let pin_name = args.get_one::<String>("pin_name").unwrap();
        let remove = args.get_one::<bool>("remove").unwrap();
        let show_as = args.get_one::<String>("show_as");
        let can_show_as = args.get_one::<String>("can_show_as");
        let type_name = args.get_one::<String>("type_name");
        self.run_pin(node_class_name, pin_name, *remove, show_as, can_show_as, type_name)
    }

    fn needs_workspace(&self) -> bool {
        true
    }
}
