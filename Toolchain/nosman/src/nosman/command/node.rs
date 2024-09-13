use clap::{ArgMatches};
use colored::Colorize;
use inquire::{Select};
use crate::nosman::command::{Command, CommandResult};
use crate::nosman::command::CommandError::InvalidArgumentError;
use crate::nosman::index::ModuleType;
use crate::nosman::workspace::{Workspace};

pub struct NodeCommand {}

impl NodeCommand {
    fn run_node(&self, plugin_name: &String, node_class_name: &String,
                remove: bool, display_name: Option<String>, description: Option<String>,
                category: Option<String>, hide_in_context_menu: bool) -> CommandResult {
        let workspace = Workspace::get()?;
        let modules = workspace.get_installed_modules(plugin_name);
        let plugins: Vec<_> = modules.iter().filter(|m| m.module_type == ModuleType::Plugin).collect();
        if plugins.len() == 0 {
            return Err(InvalidArgumentError { message: format!("Plugin {} not found", plugin_name) });
        }
        let plugin;
        if plugins.len() > 1 {
            let selection = Select::new(format!("Multiple plugins found with name {}. Please select one:", plugin_name).as_str(), plugins)
                .prompt();
            if let Err(e) = selection {
                return Err(InvalidArgumentError { message: format!("Failed to select plugin: {}", e) });
            }
            else {
                plugin = selection.unwrap();
            }
        }
        else {
            plugin = plugins[0];
        }
        if remove {
            // Prefix node_class_name if it doesn't have the plugin name
            let node_class_name = if node_class_name.starts_with(plugin_name.as_str()) {
                node_class_name.clone()
            }
            else {
                format!("{}.{}", plugin_name, node_class_name)
            };
            if !plugin.remove_node_definition(&node_class_name) {
                return Err(InvalidArgumentError { message: format!("Node class {} not found in plugin {}", node_class_name, plugin) });
            }
            println!("{}", format!("Node class {} removed from plugin {}", node_class_name, plugin_name).yellow());
        }
        else {
            if let Err (e) = plugin.add_node_definition(&node_class_name, display_name, description, category, hide_in_context_menu) {
                return Err(InvalidArgumentError { message: format!("Failed to add node class: {}", e) });
            }
            println!("{}", format!("Node class {} added to plugin {}", node_class_name, plugin_name).green());
        }
        Ok(true)
    }
}

impl Command for NodeCommand {
    fn matched_args<'a>(&self, args: &'a ArgMatches) -> Option<&'a ArgMatches> {
        args.subcommand_matches("node")
    }

    fn run(&self, args: &ArgMatches) -> CommandResult {
        let plugin_name = args.get_one::<String>("plugin").unwrap();
        let node_class_name = args.get_one::<String>("node_class_name").unwrap();
        let remove = *args.get_one::<bool>("remove").unwrap();
        let display_name = args.get_one::<String>("display_name").cloned();
        let description = args.get_one::<String>("description").cloned();
        let category = args.get_one::<String>("category").cloned();
        let hide_in_context_menu = *args.get_one::<bool>("hide_in_context_menu").unwrap();
        self.run_node(plugin_name, node_class_name, remove, display_name, description, category, hide_in_context_menu)
    }

    fn needs_workspace(&self) -> bool {
        true
    }
}
