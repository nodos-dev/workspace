use clap::{Arg, ArgAction, ArgMatches};
use colored::Colorize;
use crate::nosman::command::{Command, CommandResult};
use crate::nosman::command::CommandError::{InvalidArgument, Runtime};
use crate::nosman::index::{ModuleType, SemVer};
use crate::nosman::workspace::{Workspace};

pub struct NodeCommand {}

impl NodeCommand {
    pub fn run_node(&self, workspace: &mut Workspace, plugin_name: &String, node_class_name: &String,
                remove: bool, display_name: Option<String>, description: Option<String>,
                category: Option<String>, hide_in_context_menu: bool, nodos_version: Option<SemVer>) -> CommandResult {
        let module = workspace.get_or_select_installed_module(&plugin_name)?;
        if module.module_type != ModuleType::Plugin {
            return Err(InvalidArgument { message: format!("Selected module {} is not a Nodos plugin. Only plugins can have nodes!", plugin_name) });
        }
        let plugin = module;
        if remove {
            // Prefix node_class_name if it doesn't have the plugin name
            let node_class_name = if node_class_name.starts_with(plugin_name.as_str()) {
                node_class_name.clone()
            }
            else {
                format!("{}.{}", plugin_name, node_class_name)
            };
            plugin.remove_node_definition(&node_class_name, nodos_version).map_err(|e| {
                Runtime { message: e.to_string() }
            })?;
            println!("{}", format!("Node class {} removed from plugin {}", node_class_name, plugin_name).yellow());
        }
        else {
            plugin.add_node_definition(workspace, &node_class_name, display_name, description, category, hide_in_context_menu, nodos_version).map_err(|e| {
                Runtime { message: e.to_string() }
            })?;
            println!("{}", format!("Node class {} added to plugin {}", node_class_name, plugin_name).green());
        }
        Ok(())
    }
}

pub fn get_cli() -> clap::Command {
    clap::Command::new("node")
        .about("Add/remove a node definition in a Nodos plugin")
        .arg(Arg::new("plugin")
            .required(true)
            .help("Name of the plugin to add/remove a node.")
        )
        .arg(Arg::new("node_class_name")
            .required(true)
            .help("Node class name to add/remove.")
        )
        .arg(Arg::new("remove")
            .action(ArgAction::SetTrue)
            .long("remove")
            .help("Remove the node class.")
            .num_args(0)
            .required(false)
        )
        .arg(Arg::new("display_name")
            .long("display-name")
            .help("Display name of the node class.")
            .required(false)
        )
        .arg(Arg::new("description")
            .long("description")
            .help("Description of the node class.")
            .required(false)
        )
        .arg(Arg::new("category")
            .long("category")
            .help("Category of the node class.")
            .required(false)
        )
        .arg(Arg::new("hide_in_context_menu")
            .action(ArgAction::SetTrue)
            .long("hide")
            .help("Should Nodos editors hide it in the editor context menu?")
            .required(false)
            .num_args(0)
        )
        .arg(Arg::new("nodos_version")
            .help("Nodos engine version to use for the plugin. If not specified, the latest version will be used.")
            .long("nodos-version")
            .short('n')
            .required(false)
            .value_name("VERSION")
            .num_args(1)
        )
}

impl Command for NodeCommand {
    fn matched_args<'a>(&self, _workspace: &Workspace, args: &'a ArgMatches) -> Option<&'a ArgMatches> {
        args.subcommand_matches("node")
    }

    fn run(&self, workspace: &mut Workspace, _command_name: Option<&str>, args: &ArgMatches) -> CommandResult {
        let plugin_name = args.get_one::<String>("plugin").unwrap();
        let node_class_name = args.get_one::<String>("node_class_name").unwrap();
        let remove = *args.get_one::<bool>("remove").unwrap();
        let display_name = args.get_one::<String>("display_name").cloned();
        let description = args.get_one::<String>("description").cloned();
        let category = args.get_one::<String>("category").cloned();
        let hide_in_context_menu = *args.get_one::<bool>("hide_in_context_menu").unwrap();
        let nodos_version_str = args.get_one::<String>("nodos_version");
        let nodos_version = if let Some(version) = nodos_version_str {
            match SemVer::parse_from_str(version) {
                Some(v) => Some(v),
                None => return Err(InvalidArgument { message: format!("Invalid Nodos version: {}", version) }),
            }
        } else {
            None
        };
        self.run_node(workspace, plugin_name, node_class_name, remove, display_name, description, category, hide_in_context_menu, nodos_version)
    }

    fn needs_workspace(&self) -> bool {
        true
    }
}
