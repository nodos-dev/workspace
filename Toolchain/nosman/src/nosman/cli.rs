use std::cell::RefCell;
use std::collections::HashMap;
use std::error::Error;
use std::mem;
use clap::{Arg, ArgAction, ArgMatches, Command};
use clap::builder::StyledStr;
use crate::nosman;
use crate::nosman::command;
use crate::nosman::ui;
use crate::nosman::workspace::Workspace;

/// A double-clicked console binary gets a console window from Windows. Hide
/// and detach it so only the engine dialog is visible. Detaching leaves the
/// std handles stale (prints would error and spawning children with inherited
/// stdio fails), so repoint them at the NUL device — inheritable, so spawned
/// engines get valid handles too.
#[cfg(windows)]
fn detach_console() {
    use winapi::um::fileapi::{CreateFileW, OPEN_EXISTING};
    use winapi::um::handleapi::INVALID_HANDLE_VALUE;
    use winapi::um::minwinbase::SECURITY_ATTRIBUTES;
    use winapi::um::processenv::SetStdHandle;
    use winapi::um::winbase::{STD_ERROR_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE};
    use winapi::um::wincon::{FreeConsole, GetConsoleWindow};
    use winapi::um::winnt::{FILE_SHARE_READ, FILE_SHARE_WRITE, GENERIC_READ, GENERIC_WRITE};
    use winapi::um::winuser::{ShowWindow, SW_HIDE};
    unsafe {
        let window = GetConsoleWindow();
        if !window.is_null() {
            ShowWindow(window, SW_HIDE);
        }
        FreeConsole();
        let mut security = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: std::ptr::null_mut(),
            bInheritHandle: 1,
        };
        let nul: Vec<u16> = "NUL\0".encode_utf16().collect();
        let nul_handle = CreateFileW(
            nul.as_ptr(),
            GENERIC_READ | GENERIC_WRITE,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            &mut security,
            OPEN_EXISTING,
            0,
            std::ptr::null_mut(),
        );
        if nul_handle != INVALID_HANDLE_VALUE {
            SetStdHandle(STD_INPUT_HANDLE, nul_handle);
            SetStdHandle(STD_OUTPUT_HANDLE, nul_handle);
            SetStdHandle(STD_ERROR_HANDLE, nul_handle);
        }
    }
}

#[cfg(not(windows))]
fn detach_console() {}

/// Reads the options that apply to every command and settles how output looks,
/// then answers where the workspace is.
///
/// This runs before the real parse because the workspace is opened, and starts
/// printing, while the command line is still being worked out.
fn apply_global_options(cmd: &Command) -> std::path::PathBuf {
    let mut wcmd = cmd.clone();
    wcmd = wcmd.subcommand(Command::new("help")) // trick because we can't get workspace dir without parsing everything.
        .disable_help_subcommand(true)
        .ignore_errors(true);
    let matches = wcmd.get_matches();
    ui::set_color(find_option(&matches, "color").map(|s| s.as_str()).unwrap_or("auto"));
    if find_flag(&matches, "quiet") {
        ui::set_verbosity(ui::Verbosity::Quiet);
    } else if find_flag(&matches, "verbose") {
        ui::set_verbosity(ui::Verbosity::Verbose);
    }
    // TODO: Try to get --workspace option without having to clone command and parse all args.
    std::path::PathBuf::from(matches.get_one::<String>("workspace").unwrap_or(&".".to_string()))
}

/// The options that shape output are accepted next to any command, so look for
/// them all the way down the chain of subcommands the user typed.
fn find_flag(matches: &ArgMatches, id: &str) -> bool {
    if matches.try_get_one::<bool>(id).ok().flatten() == Some(&true) {
        return true;
    }
    matches.subcommand().is_some_and(|(_, sub)| find_flag(sub, id))
}

fn find_option<'a>(matches: &'a ArgMatches, id: &str) -> Option<&'a String> {
    // The innermost mention wins, so that `nodos install --color never` reads
    // the same as `nodos --color never install`.
    if let Some((_, sub)) = matches.subcommand() {
        if let Some(found) = find_option(sub, id) {
            return Some(found);
        }
    }
    matches.try_get_one::<String>(id).ok().flatten()
}

/// Adds the options that shape output to a command and to every command under
/// it. clap's own global arguments only reach commands it has not built yet,
/// and nosman renders subcommand help up front, which builds them.
fn add_output_options(cmd: Command) -> Command {
    let takes = |cmd: &Command, long: &str, short: Option<char>| {
        cmd.get_arguments()
            .any(|arg| arg.get_long() == Some(long) || (short.is_some() && arg.get_short() == short))
    };
    let mut cmd = cmd;
    if !takes(&cmd, "quiet", Some('q')) {
        cmd = cmd.arg(Arg::new("quiet")
            .help("Only print warnings and errors")
            .short('q')
            .long("quiet")
            .action(ArgAction::SetTrue)
            .num_args(0)
        );
    }
    if !takes(&cmd, "verbose", None) {
        cmd = cmd.arg(Arg::new("verbose")
            .help("Print more information about what is being done")
            .long("verbose")
            .action(ArgAction::SetTrue)
            .num_args(0)
        );
    }
    if !takes(&cmd, "color", None) {
        cmd = cmd.arg(Arg::new("color")
            .help("When to colour output: auto, always or never")
            .long("color")
            .num_args(1)
            .value_parser(["auto", "always", "never"])
        );
    }
    let subcommand_names: Vec<String> = cmd.get_subcommands().map(|s| s.get_name().to_string()).collect();
    for name in subcommand_names {
        cmd = cmd.mut_subcommand(name, add_output_options);
    }
    cmd
}

pub fn run_cli() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() == 1 {
        detach_console();
        let workspace_dir = std::env::current_exe().expect("Unable to access current executable path.")
            .parent().expect("Unable to access parent directory of executable.").to_path_buf();
        command::launch::launch_nodos(&workspace_dir, false, None, true, &[])?;
        return Ok(());
    }

    let exe_path = std::env::current_exe().expect("Unable to get current executable path");
    let stem = exe_path.file_stem().expect("Unable to get executable name").to_str().expect("Unable to convert executable name to string");
    let mut cli = Command::new(stem.to_string())
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
            .num_args(0..=1)
            .help("Prints help information about a command")
        );
    cli = command::register_cli(cli);
    cli = add_output_options(cli);

    let workspace_dir = apply_global_options(&cli);
    let workspace = Workspace::from_root(&workspace_dir);

    let mut subcommand_helps: HashMap<String, StyledStr> = HashMap::new();
    for subcommand in cli.get_subcommands_mut() {
        let moved = mem::take(subcommand);
        // Re-enable help flag for subcommands
        *subcommand = moved.disable_help_flag(false);
        subcommand_helps.insert(subcommand.get_name().to_string(), subcommand.render_help());
    }

    // Add commands from extensions
    if workspace.ready() {
        cli = nosman::extensions::add_extensions(&workspace, cli);
        cli = add_output_options(cli);
    }

    let help_str = cli.render_help();
    let matches = cli.get_matches();

    // If contains --silently-agree-eula, agree to EULAs
    if let Some(agree_eula) = matches.get_one::<bool>("silently_agree_eula") {
        if *agree_eula {
            workspace.ensure_ready_if_required(true)?;
            nosman::eula::silently_agree_eulas(&workspace.root);
            return Ok(());
        }
    }

    // If -h comes first, print help and exit
    if matches.contains_id("help") {
        // If help is called without a subcommand, print the help string
        let subcommand_name = matches.get_one::<String>("help");
        if subcommand_name.is_none() {
            println!("{}", help_str.ansi());
            return Ok(());
        }
        // If help is called with a subcommand, print the help string for that subcommand
        let subcommand_name = subcommand_name.unwrap();
        if let Some(help) = subcommand_helps.get(subcommand_name) {
            println!("{}", help.ansi());
            return Ok(());
        }
    }

    let workspace_ref = RefCell::new(workspace);
    for command in nosman::command::commands().iter() {
        let match_res = command.matched_args(&workspace_ref.borrow(), &matches);
        match match_res {
            Some(matched_args) => {
                let needs_workspace = (*command).needs_workspace();
                workspace_ref.borrow().ensure_ready_if_required(needs_workspace)?;

                // Auto-rescan workspace if needed when workspace is required
                if needs_workspace && workspace_ref.borrow().ready() {
                    workspace_ref.borrow_mut().auto_rescan_if_needed()?;
                }

                (*command).run(&mut workspace_ref.borrow_mut(), matches.subcommand_name(), matched_args)?;
                return Ok(());
            }
            None => continue,
        };
    }

    eprintln!("{}", help_str.ansi());
    Err("No matching command found".into())
}
