use std::ffi::CStr;
use std::os::raw::{c_char};
use serde::{Deserialize, Serialize};
use std::ptr;
use clap::{ArgAction};
use colored::Colorize;
use libloading::Library;
use crate::nosman::workspace::Workspace;

#[repr(C)]
#[derive(Debug, Serialize, Deserialize, Hash, Clone, Eq, PartialEq)]
pub enum NosArgAction {
    Set,
    SetTrue,
    SetFalse,
}

#[repr(C)]
#[derive(Debug)]
pub struct CNosArgDesc {
    pub name: *const c_char,
    pub description: *const c_char,
    pub required: bool,
    pub action: NosArgAction,
}

#[repr(C)]
#[derive(Debug)]
pub struct CNosCommandDesc {
    pub name: *const c_char,
    pub description: *const c_char,
    pub args_count: usize,
    pub args: *mut CNosArgDesc,
    pub sub_commands_count: usize,
    pub sub_commands: *mut CNosCommandDesc,
}

#[repr(C)]
#[derive(Debug)]
pub struct CNosArg {
    pub name: *const c_char,
    pub value: *const c_char,
}

#[repr(C)]
#[derive(Debug)]
pub struct CNosCommand {
    pub name: *const c_char,
    pub args_count: usize,
    pub args: *mut CNosArg,
    pub sub_command: *mut CNosCommand,
}

#[repr(C)]
#[derive(Debug)]
pub struct CNosRunCommandParams {
    pub command: *mut CNosCommand,
    pub workspace_dir: *const c_char
}

// Rust representation for CNosArgDesc
#[derive(Debug, Serialize, Deserialize, Hash, Clone, Eq, PartialEq)]
pub struct NosArgDesc {
    pub name: String,
    pub description: String,
    pub required: bool,
    pub action: NosArgAction,
}

// Rust representation for CNosCommandDesc
#[derive(Debug, Serialize, Deserialize, Hash, Clone, Eq, PartialEq)]
pub struct NosCommandDesc {
    pub name: String,
    pub description: String,
    pub args: Vec<NosArgDesc>,
    pub sub_commands: Vec<NosCommandDesc>,
}

// Rust representation for CNosArg
#[derive(Debug, Serialize, Deserialize, Hash, Clone, Eq, PartialEq)]
pub struct NosArg {
    pub name: String,
    pub value: String,
}

// Rust representation for CNosCommand
#[derive(Debug, Serialize, Deserialize, Hash, Clone, Eq, PartialEq)]
pub struct NosCommand {
    pub name: String,
    pub args: Vec<NosArg>,
    pub sub_command: Option<Box<NosCommand>>,
}

fn c_str_to_string(c_string: *const c_char) -> String {
    // Check null pointer
    if c_string.is_null() {
        return String::new();
    }
    unsafe {
        CStr::from_ptr(c_string).to_string_lossy().into_owned()
    }
}

// Conversion implementations
impl From<&CNosArgDesc> for NosArgDesc {
    fn from(c_arg_desc: &CNosArgDesc) -> Self {
        let name = c_str_to_string(c_arg_desc.name);
        let description = c_str_to_string(c_arg_desc.description);
        NosArgDesc { name, description, required: c_arg_desc.required, action: c_arg_desc.action.clone() }
    }
}

impl From<&CNosCommandDesc> for NosCommandDesc {
    fn from(c_command_desc: &CNosCommandDesc) -> Self {
        let name = c_str_to_string(c_command_desc.name);
        let description = c_str_to_string(c_command_desc.description);

        // Convert arguments
        let args = if c_command_desc.args_count > 0 {
            unsafe {
                std::slice::from_raw_parts(c_command_desc.args, c_command_desc.args_count)
                    .iter()
                    .map(|arg_desc| NosArgDesc::from(arg_desc))
                    .collect()
            }
        } else {
            Vec::new()
        };

        // Convert subcommands
        let sub_commands = if c_command_desc.sub_commands_count > 0 {
            unsafe {
                std::slice::from_raw_parts(c_command_desc.sub_commands, c_command_desc.sub_commands_count)
                    .iter()
                    .map(|sub_command_desc| NosCommandDesc::from(sub_command_desc))
                    .collect()
            }
        } else {
            Vec::new()
        };

        NosCommandDesc { name, description, args, sub_commands }
    }
}

impl From<&CNosArg> for NosArg {
    fn from(c_arg: &CNosArg) -> Self {
        let name = c_str_to_string(c_arg.name);
        let value = c_str_to_string(c_arg.value);
        NosArg { name, value }
    }
}

impl From<&CNosCommand> for NosCommand {
    fn from(c_command: &CNosCommand) -> Self {
        let name = c_str_to_string(c_command.name);

        // Convert arguments
        let args = if c_command.args_count > 0 {
            unsafe {
                std::slice::from_raw_parts(c_command.args, c_command.args_count)
                    .iter()
                    .map(|arg| NosArg::from(arg))
                    .collect()
            }
        } else {
            Vec::new()
        };

        // Convert optional subcommand
        let sub_command = if !c_command.sub_command.is_null() {
            Some(Box::new(NosCommand::from(unsafe { &*c_command.sub_command })))
        } else {
            None
        };

        NosCommand {
            name,
            args,
            sub_command,
        }
    }
}

pub fn get_commands(lib: Library) -> Option<Vec<NosCommandDesc>> {
    let fn_name = b"nosGetCommands\0";
    let res = unsafe { lib.get::<unsafe extern "C" fn(*mut usize, *mut *mut CNosCommandDesc)>(fn_name) };
    match res {
        Ok(fn_get_commands) => {
            let mut count: usize = 0;
            let mut commands: *mut CNosCommandDesc = ptr::null_mut();
            unsafe {
                fn_get_commands(&mut count, &mut commands);
            }

            if count > 0 {
                let command_descs = unsafe {
                    std::slice::from_raw_parts(commands, count)
                        .iter()
                        .map(|command_desc| NosCommandDesc::from(command_desc))
                        .collect()
                };
                Some(command_descs)
            } else {
                None
            }
        }
        Err(_) => None,
    }
}

pub fn add_extensions(workspace: &Workspace, mut cmd: clap::Command) -> clap::Command {
    let modules = workspace.get_latest_installed_modules();
    for module in modules {
        for command in &module.commands {
            let mut new_cmd = clap::Command::new(command.name.clone())
                .about(format!("{} {}", format!("{}", module.info.id.name).italic().green(), command.description.as_str()));
            for arg in &command.args {
                let mut new_arg = clap::Arg::new(arg.name.clone())
                    .long(arg.name.clone())
                    .action(match arg.action {
                        NosArgAction::Set => ArgAction::Set,
                        NosArgAction::SetTrue => ArgAction::SetTrue,
                        NosArgAction::SetFalse => ArgAction::SetFalse,
                    })
                    .help(arg.description.clone());
                if arg.required {
                    new_arg = new_arg.required(true);
                }
                new_cmd = new_cmd.arg(new_arg);
            }
            cmd = cmd.subcommand(new_cmd);
        }
    }
    cmd
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CString;
    use std::ptr;

    #[test]
    fn test_cnos_arg_desc_to_rust() {
        // Create CString instances for argument name and description
        let name = CString::new("arg_name").unwrap();
        let desc = CString::new("Argument description").unwrap();

        // Create CNosArgDesc with valid pointers
        let c_arg_desc = CNosArgDesc {
            name: name.as_ptr(),
            description: desc.as_ptr(),
            required: false,
            action: NosArgAction::Set,
        };

        // Convert to Rust type and check fields
        let rust_arg_desc = NosArgDesc::from(&c_arg_desc);

        assert_eq!(rust_arg_desc.name, "arg_name");
        assert_eq!(rust_arg_desc.description, "Argument description");
        assert_eq!(rust_arg_desc.required, false);
    }

    #[test]
    fn test_cnos_command_desc_to_rust() {
        // Create CString instances for command name and description
        let name = CString::new("command").unwrap();
        let desc = CString::new("Command description").unwrap();

        // Create CNosArgDesc array for arguments
        let c_args = vec![CNosArgDesc {
            name: ptr::null(),
            description: ptr::null(),
            required: false,
            action: NosArgAction::Set,
        }];

        // Create CNosCommandDesc array for subcommands
        let c_sub_commands = vec![CNosCommandDesc {
            name: ptr::null(),
            description: ptr::null(),
            args_count: 0,
            args: ptr::null_mut(),
            sub_commands_count: 0,
            sub_commands: ptr::null_mut(),
        }];

        // Create CNosCommandDesc with valid pointers
        let c_command_desc = CNosCommandDesc {
            name: name.as_ptr(),
            description: desc.as_ptr(),
            args_count: c_args.len(),
            args: c_args.as_ptr() as *mut CNosArgDesc,
            sub_commands_count: c_sub_commands.len(),
            sub_commands: c_sub_commands.as_ptr() as *mut CNosCommandDesc,
        };

        // Convert to Rust type and check fields
        let rust_command_desc = NosCommandDesc::from(&c_command_desc);

        assert_eq!(rust_command_desc.name, "command");
        assert_eq!(rust_command_desc.description, "Command description");
        assert_eq!(rust_command_desc.args.len(), 1);
        assert_eq!(rust_command_desc.sub_commands.len(), 1);
    }

    #[test]
    fn test_cnos_arg_to_rust() {
        // Create CString instances for argument name and value
        let name = CString::new("arg_name").unwrap();
        let value = CString::new("arg_value").unwrap();

        // Create CNosArg with valid pointers
        let c_arg = CNosArg {
            name: name.as_ptr(),
            value: value.as_ptr(),
        };

        // Convert to Rust type and check fields
        let rust_arg = NosArg::from(&c_arg);

        assert_eq!(rust_arg.name, "arg_name");
        assert_eq!(rust_arg.value, "arg_value");
    }

    #[test]
    fn test_cnos_command_to_rust() {
        // Create CString instances for command name
        let name = CString::new("command").unwrap();

        // Create CNosArg array for arguments
        let c_args = vec![CNosArg {
            name: ptr::null(),
            value: ptr::null(),
        }];

        // Create CNosCommand with valid pointers
        let c_command = CNosCommand {
            name: name.as_ptr(),
            args_count: c_args.len(),
            args: c_args.as_ptr() as *mut CNosArg,
            sub_command: ptr::null_mut(),
        };

        // Convert to Rust type and check fields
        let rust_command = NosCommand::from(&c_command);

        assert_eq!(rust_command.name, "command");
        assert_eq!(rust_command.args.len(), 1);
        assert!(rust_command.sub_command.is_none());
    }
}
