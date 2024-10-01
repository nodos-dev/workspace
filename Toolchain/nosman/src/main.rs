extern crate clap;

use std::collections::HashMap;
use clap::{Arg, ArgAction, Command};

use std::error::Error;
use std::mem;
use clap::builder::StyledStr;
use colored::Colorize;
use sysinfo::System;
use crate::nosman::{constants, workspace};
use crate::nosman::command::sample;

mod nosman;

fn print_error(e: &dyn Error) {
    eprintln!("{}", format!("Error: {}", e).as_str().red());
    let mut cause = e.source();
    while let Some(e) = cause {
        eprintln!("{}", format!("Caused by: {}", e).as_str().red());
        cause = e.source();
    }
}

fn launched_from_file_explorer() -> bool {
    let mut sys = System::new_all();
    sys.refresh_all();
    if let Ok(pid) = sysinfo::get_current_pid() {
        if let Some(process) = sys.process(pid) {
            if let Some(parent_pid) = process.parent() {
                if let Some(parent_process) = sys.process(parent_pid) {
                    #[cfg(target_os = "windows")]
                    return parent_process.name().to_lowercase() == "explorer.exe";
                    #[cfg(target_os = "macos")]
                    return parent_process.name() == "Finder";
                    #[cfg(target_os = "linux")]
                    return ["nautilus", "dolphin", "nemo", "thunar"]
                        .iter()
                        .any(|&name| parent_process.name().to_lowercase() == name);
                }
            }
        }
    }
    false
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() == 1 {
        // Get parent process name. If it is a file explorer, open Nodos
        if launched_from_file_explorer() {
            let workspace_dir = std::env::current_dir().expect("Unable to access current working directory.");
            nosman::command::launch::launch_nodos(&workspace_dir, false);
            return;
        }
    }

    let lang_tool_arg = Arg::new("language/tool")
             .long("language-tool")
             .short('l')
             .help("Language and tool to use")
             .value_parser(clap::builder::PossibleValuesParser::new(["cpp/cmake"]))
             .default_value("cpp/cmake");

    let exe_path = std::env::current_exe().expect("Unable to get current executable path");
    let stem = exe_path.file_stem().expect("Unable to get executable name").to_str().expect("Unable to convert executable name to string");
    let boxed_name = Box::new(stem.to_string());
    let name: &'static str = Box::leak(boxed_name); // Will live throughout the program lifetime. Command::new wants 'static str.
    let mut cmd = Command::new(name)
        .disable_help_flag(true)
        .version(env!("VERGEN_BUILD_SEMVER"))
        .about("Nodos Package Manager")
        .arg(Arg::new("workspace")
            .help("Directory to the workspace")
            .short('w')
            .long("workspace")
            .default_value(".")
        )
        .arg(Arg::new("silently_agree_eula")
            .help("Agrees to Nodos EULA. If multiple engines are installed, it will agree to all of their EULAs.")
            .long("silently-agree-eula")
            .action(ArgAction::SetTrue)
            .num_args(0)
            .required(false)
        )
        .arg(Arg::new("help")
            .short('h')
            .long("help")
            .help("Prints help information about a command")
        )
        .subcommand(Command::new("init")
            .about("Initialize a directory as a Nodos workspace.")
        )
        .subcommand(Command::new("deinit")
            .about("Deinitialize a Nodos workspace.")
        )
        .subcommand(Command::new("install")
            .about("Install a module")
            .arg(Arg::new("module").required(true))
            .arg(Arg::new("version").required(false))
            .arg(Arg::new("exact")
                .action(ArgAction::SetTrue)
                .help("If not set, version parameter will be interpreted as minimum required version within that minor/patch version.\n\
                If no version 'x' such that 'a.b <= x < a.(b+1)' is found among installed modules, latest such version will be installed.\n\
                If version is set to 'latest' or has no minor component, it will fail.")
                .long("exact")
                .num_args(0)
                .required(false)
            )
            .arg(Arg::new("prefix")
                .help("Folder path relative to out_dir. The module contents will be under this folder. By default, its '<module_name>-<version>'.")
                .long("prefix")
                .required(false)
            )
            .arg(Arg::new("out_dir")
                .help("The directory where the module will be installed")
                .default_value("./Module")
                .long("out-dir")
                .required(false)
            )
        )
        .subcommand(Command::new("remove")
            .about("Remove a module")
            .arg(Arg::new("module").required(true))
            .arg(Arg::new("version").required(true))
        )
        .subcommand(Command::new("rescan")
            .about("Rescan modules and update caches")
            .arg(Arg::new("fetch_index")
                .action(ArgAction::SetTrue)
                .help("Fetch remote module indices before scanning")
                .long("fetch-index")
                .num_args(0)
                .required(false)
            )
        )
        .subcommand(Command::new("list")
            .about("List installed modules")
        )
        .subcommand(Command::new("info")
            .about("Returns information about an installed module in JSON format.\n\
            If no such module is installed, it will return an error.")
            .arg(Arg::new("module").required(true))
            .arg(Arg::new("version").required(true))
            .arg(Arg::new("relaxed")
                .action(ArgAction::SetTrue)
                .help("If set, version parameter will be interpreted as minimum required version within that minor/patch version.\n\
                It will return information about a version 'x' found among installed modules such that 'a.b <= x < a.(b+1)'.")
                .long("relaxed")
                .num_args(0)
                .required(false)
            )
        )
        .subcommand(Command::new("sdk-info")
            .about("Returns information about an installed Nodos SDK under workspace.\n\
            If no such version is found, it will return an error.")
            .arg(Arg::new("version").required(true))
			.arg(Arg::new("sdk-type").required(false)
				.help("Type of the SDK to get information about.")
				.default_value("engine")
				.value_parser(clap::builder::PossibleValuesParser::new(["engine", "plugin", "subsystem", "process"])))
        )
        .subcommand(Command::new("remote")
            .about("Manage remotes.")
            .subcommand(Command::new("add")
                .about("Add a remote")
                .arg(Arg::new("url").required(true))
            )
            .subcommand(Command::new("list")
                .about("List remotes")
            )
            .subcommand(Command::new("remove")
                .about("Remove a remote")
                .arg(Arg::new("url").required(true))
            )
        )
        .subcommand(Command::new("create")
            .about("Create a Nodos plugin or subsystem module")
            .arg(Arg::new("type")
                .value_parser(clap::builder::PossibleValuesParser::new(["plugin", "subsystem"]))
                .required(true)
            )
            .arg(Arg::new("name")
                .required(true)
            )
            .arg(lang_tool_arg.clone())
            .arg(Arg::new("output_dir")
                .help("Path to create the module folder in")
                .long("output-dir")
                .short('o')
                .default_value("./Module")
                .required(false)
            )
            .arg(Arg::new("prefix")
                .help("Folder path relative to out_dir. The module contents will be under this folder. By default, its '<module_name>'.")
                .long("prefix")
                .required(false)
            )
            .arg(Arg::new("yes_to_all")
                .action(ArgAction::SetTrue)
                .long("yes-to-all")
                .help("Do not ask for confirmation & use defaults for missing parameters")
                .num_args(0)
                .short('y')
                .required(false)
            )
            .arg(Arg::new("description")
                .help("Description of the module")
                .long("description")
                .default_value("")
                .required(false)
            )
            .arg(Arg::new("dependency")
                .help("Add module dependency. Can be specified multiple times. Format: <module_name>-<version>")
                .long("dependency")
                .short('d')
                .required(false)
                .action(ArgAction::Append)
                .num_args(1)
            )
        )
        .subcommand(Command::new("get-sample")
            .alias("sample")
            .about("Get a sample plugin, subsystem or a process implementation for Nodos")
            .arg(Arg::new("name")
                .value_parser(clap::builder::PossibleValuesParser::new(sample::SAMPLES.keys().copied().collect::<Vec<&str>>().as_slice()))
                .required(true)
            )
            .arg(Arg::new("output_dir")
                .help("Path to bring the sample to")
                .long("output-dir")
                .short('o')
                .required(true)
            )
        )
        .subcommand(Command::new("get").visible_alias("update")
            .about("Brings a Nodos release under workspace (with --workspace option).\n\
            If there is an existing Nodos release, updates it (note that this will remove all installed Nodos engines!)")
            .arg(Arg::new("name")
                .help("Name of the Nodos release to bring. Can be 'nodos' or some bundled version.")
                .long("name")
                .default_value("nodos")
            )
            .arg(Arg::new("version")
                .help("Version of the Nodos release to bring. If not provided, the latest version will be installed.")
                .long("version")
                .short('v')
                .required(false)
            )
            .arg(Arg::new("yes_to_all")
                .help("Do not ask for confirmation. Execute default behaviour.")
                .short('y')
                .action(ArgAction::SetTrue)
                .num_args(0)
                .required(false)
            )
            .arg(Arg::new("clean_modules")
                .help("Remove Nodos modules before installing the new release.")
                .action(ArgAction::SetTrue)
                .num_args(0)
                .required(false)
                .long("clean-modules")
            )
        )
        .subcommand(Command::new("publish")
            .about("Publish a package")
            .after_help("This command will publish a package to the specified remote.\n\
            Currently, only the git repositories hosted on GitHub can be used to publish.")
            .arg(Arg::new("path")
                .long("path")
                .short('p')
                .help(format!("Path to the root folder of the package (or a file) to be published.\n\
                If not provided, the current directory will be used.\n\
                If the path is a folder and it does not contain a {} file, it will add all files to the release.", constants::PUBLISH_OPTIONS_FILE_NAME))
                .default_value(".")
            )
            .arg(Arg::new("name")
                .long("name")
                .short('n')
                .help("Name of the package. It will be overridden by the module manifest files under <path> if present.\n\
                If the <path> does not contain a module manifest file, this parameter is required."))
            .arg(Arg::new("version")
                .long("version")
                .help("Version of the package. It will be overridden by the module manifest files under <path> if present.\n\
                If the <path> does not contain a module manifest file, this parameter is required.")
            )
            .arg(Arg::new("version_suffix")
                .long("version-suffix")
                .help("Suffix to append to the version of the package.")
                .default_value("")
            )
            .arg(Arg::new("remote")
                .help("Name of the remote to publish to.")
                .long("remote")
                .default_value("default")
            )
            .arg(Arg::new("type")
                .long("type")
                .short('t')
                .value_parser(clap::builder::PossibleValuesParser::new(["plugin", "subsystem", "nodos", "engine", "generic"]))
                .help("Type of the package. It will be overridden by the module manifest files under <path> if present.\n\
                If the <path> does not contain a module manifest file, this parameter is required.")
            )
            .arg(Arg::new("vendor")
                .help("Who is publishing the package?\n\
                Required if the module to be published was not added to the index before.")
                .long("vendor")
            )
            .arg(Arg::new("publisher_name")
                .help("Git name of the publishing agent. If not provided, the name of the current git user will be used.")
                .long("publisher-name")
                .required(false)
            )
            .arg(Arg::new("publisher_email")
                .help("Git email of the publishing agent. If not provided, the email of the current git user will be used.")
                .long("publisher-email")
                .required(false)
            )
            .arg(Arg::new("dry_run")
                .action(ArgAction::SetTrue)
                .long("dry-run")
                .help("Do not actually publish the package, just show what would be done.")
                .num_args(0)
                .required(false)
            )
            .arg(Arg::new("verbose")
                .action(ArgAction::SetTrue)
                .long("verbose")
                .help("Print more information about the process.")
                .num_args(0)
                .required(false)
            )
            .arg(Arg::new("tag")
                .action(ArgAction::Append)
                .long("tag")
                .help("Add a tag to the release. Can be specified multiple times.")
                .required(false)
                .num_args(1)
            )
        )
        .subcommand(Command::new("publish-batch")
            .about("Publish all/changed modules under the git repository.")
            .after_help(format!("This command will publish all/changed modules under the git repository to the specified remote.\n\
            It will use the {} files to compare file changes & adding files to the release. In the {} file, 'trigger_publish_globs' field will be used check file changes. \
            The 'release_globs' field however, will both be used for including files to the release as well as checking file changes.", constants::PUBLISH_OPTIONS_FILE_NAME, constants::PUBLISH_OPTIONS_FILE_NAME))
            .arg(Arg::new("remote")
                .help("Name of the remote to publish to.")
                .default_value("default")
            )
            .arg(Arg::new("repo_path")
                .long("repo-path")
                .short('r')
                .help("Path to the root folder of the repository. If not provided, the current directory will be used.")
                .default_value(".")
            )
            .arg(Arg::new("compare_with")
                .long("compare-with")
                .short('c')
                .help("Compare current with the given branch, tag or ref.\n\
                If not provided or empty, it will publish all modules found under the provided repo.")
            )
            .arg(Arg::new("version_suffix")
                .long("version-suffix")
                .help("Suffix to append to the version of the modules to be published.")
                .default_value("")
            )
            .arg(Arg::new("vendor")
                .help("Who is publishing the package?\n\
                Required if the module to be published was not added to the index before.")
                .long("vendor")
            )
            .arg(Arg::new("publisher_name")
                .help("Git name of the publishing agent. If not provided, the name of the current git user for the remote will be used.")
                .long("publisher-name")
                .required(false)
            )
            .arg(Arg::new("publisher_email")
                .help("Git email of the publishing agent. If not provided, the email of the current git user for the remote will be used.")
                .long("publisher-email")
                .required(false)
            )
            .arg(Arg::new("dry_run")
                .action(ArgAction::SetTrue)
                .long("dry-run")
                .help("Do not actually publish the package, just show what would be done.")
                .num_args(0)
                .required(false)
            )
            .arg(Arg::new("verbose")
                .action(ArgAction::SetTrue)
                .long("verbose")
                .help("Print more information about the process.")
                .num_args(0)
                .required(false)
            )
            .arg(Arg::new("tag")
                .action(ArgAction::Append)
                .long("tag")
                .help("Add a tag to the release. Can be specified multiple times.")
                .required(false)
                .num_args(1)
            )
        )
        .subcommand(Command::new("unpublish")
            .alias("yank")
            .about("Unpublish a package from the index.")
            .arg(Arg::new("package_name").required(true))
            .arg(Arg::new("remote")
                .help("Name of the remote to edit.")
                .long("remote")
                .default_value("default")
            )
            .arg(Arg::new("version")
                .help("Version of the package to unpublish. If not provided, all versions will be unpublished."))
            .arg(Arg::new("dry_run")
                .action(ArgAction::SetTrue)
                .long("dry-run")
                .help("Do not actually publish the package, just show what would be done.")
                .num_args(0)
                .required(false)
            )
            .arg(Arg::new("verbose")
                .action(ArgAction::SetTrue)
                .long("verbose")
                .help("Print more information about the process.")
                .num_args(0)
                .required(false)
            )
        )
        .subcommand(Command::new("pin")
            .about("Add/remove a pin to/from a node definition")
            .arg(Arg::new("node_class_name")
                .required(true)
                .help("Node class name to add/remove pin.")
            )
            .arg(Arg::new("pin_name")
                .required(true)
                .help("Name of the pin to add/remove.")
            )
            .arg(Arg::new("remove")
                .action(ArgAction::SetTrue)
                .long("remove")
                .help("Remove the pin.")
                .num_args(0)
                .required(false)
            )
            .arg(Arg::new("show_as")
                .long("show-as")
                .help("Determine whether the pin is input, property or output pin.")
                .value_parser(clap::builder::PossibleValuesParser::new(constants::POSSIBLE_SHOW_AS))
            )
            .arg(Arg::new("can_show_as")
                .long("can-show-as")
                .help("Determine the kind of the pin.")
                .required(false)
                .value_parser(clap::builder::PossibleValuesParser::new(constants::POSSIBLE_CAN_SHOW_AS))
            )
            .arg(Arg::new("type_name")
                .long("type-name")
                .help("Data type name of the pin")
                .required(false)
            )
        )
        .subcommand(Command::new("node")
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
        )
        .subcommand(Command::new("launch")
            .about("Launch Nodos")
        )
        .subcommand(Command::new("dev")
            .about("Helper commands for Nodos module development")
            .subcommand(Command::new("pull")
                .about("Scans for git repositories and pulls their current branches")
                .arg(Arg::new("dir")
                    .long("directory")
                    .short('m')
                    .help("Path to the directory to scan for git repositories")
                    .action(ArgAction::Append)
                    .num_args(1)
                    .default_values(&[".", "Engine", "Module"])
                )
            )
            .subcommand(Command::new("gen")
                .about("Generates project files for Nodos module development")
                .arg(lang_tool_arg.clone())
                .arg(Arg::new("extra_args")
                    .last(true)
                    .help("Arguments to pass to the underlying tool when generating project files")
                )
            )
        );

    let help_str = cmd.render_help();
    let mut subcommand_helps: HashMap<String, StyledStr> = HashMap::new();
    for subcommand in cmd.get_subcommands_mut() {
        let moved = mem::take(subcommand);
        // Re-enable help flag for subcommands
        *subcommand = moved.disable_help_flag(false);
        subcommand_helps.insert(subcommand.get_name().to_string(), subcommand.render_help());
    }

    let matches = cmd.get_matches();

    let workspace_dir = std::path::PathBuf::from(matches.get_one::<String>("workspace").unwrap());

    // If contains --silently-agree-eula, agree to EULAs
    if matches.contains_id("silently_agree_eula") {
        workspace::set_workspace_root(workspace_dir, true);
        nosman::eula::silently_agree_eulas();
        return;
    }

    // If -h comes first, print help and exit
    if matches.contains_id("help") {
        // If help is called without a subcommand, print the help string
        let subcommand_name = matches.get_one::<String>("help");
        if subcommand_name.is_none() {
            println!("{}", help_str.ansi());
            std::process::exit(0);
        }
        // If help is called with a subcommand, print the help string for that subcommand
        let subcommand_name = subcommand_name.unwrap();
        if let Some(help) = subcommand_helps.get(subcommand_name) {
            println!("{}", help.ansi());
            std::process::exit(0);
        }
    }

    let mut matched = false;
    for command in nosman::command::commands().iter() {
        match command.matched_args(&matches) {
            Some(command_args) => {
                workspace::set_workspace_root(workspace_dir, (*command).needs_workspace());
                match (*command).run(command_args) {
                    Ok(_) => {
                        // nothing
                    },
                    Err(e) => {
                        print_error(&e);
                        std::process::exit(1);
                    }
                };
                matched = true;
                break;
            },
            None => continue,
        };
    }

    if !matched {

        println!("{}", help_str.ansi());
        std::process::exit(1);
    }
}

