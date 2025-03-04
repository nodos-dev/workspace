pub mod init;
pub mod remote;
mod install;
mod info;
mod remove;
mod rescan;
mod deinit;
mod create;
mod sdk_info;
mod list;
mod publish;
mod publish_batch;
mod get;
pub mod sample;
mod unpublish;
mod pin;
mod node;
mod dev;
pub(crate) mod launch;
mod extension;
mod depend;

use std::io;

use clap::ArgMatches;
use thiserror::Error;
use crate::nosman::workspace::Workspace;

#[derive(Error, Debug)]
pub enum CommandError {
    #[error("I/O (file {}): {}", file, message)]
    IO { file: String, message: String },
    #[error("Invalid argument: {}", message)]
    InvalidArgument { message: String },
    #[error("Zip: {}", message)]
    Zip { message: String },
    #[error("{}", message)]
    Runtime { message: String },
}

impl From<io::Error> for CommandError {
    fn from(err: io::Error) -> Self {
        CommandError::IO { file: "Unknown".to_string(), message: format!("{}", err) }
    }
}

pub(crate) type CommandResult = Result<(), CommandError>;

pub trait Command {
    fn matched_args<'a, 'b>(&self, workspace: &'a Workspace, args : &'b ArgMatches) -> Option<&'b ArgMatches>;
    fn run(&self, workspace: &mut Workspace, command_name: Option<&str>, args: &ArgMatches) -> CommandResult;
    fn needs_workspace(&self) -> bool {
        true
    }
}

pub fn commands() -> Vec<Box<dyn Command>> {
    vec![
        Box::new(init::InitCommand {}),
        Box::new(remote::RemoteAddCommand {}),
        Box::new(remote::RemoteListCommand {}),
        Box::new(install::InstallCommand {}),
        Box::new(info::InfoCommand {}),
        Box::new(remove::RemoveCommand {}),
        Box::new(rescan::RescanCommand {}),
        Box::new(deinit::DeinitCommand {}),
        Box::new(create::CreateCommand {}),
        Box::new(sdk_info::SdkInfoCommand {}),
        Box::new(list::ListCommand {}),
        Box::new(publish::PublishCommand {}),
        Box::new(publish_batch::PublishBatchCommand {}),
        Box::new(get::GetCommand {}),
        Box::new(sample::SampleCommand {}),
        Box::new(unpublish::UnpublishCommand {}),
        Box::new(pin::PinCommand {}),
        Box::new(node::NodeCommand {}),
        Box::new(dev::DevPullCommand {}),
        Box::new(dev::DevGenCommand {}),
        Box::new(dev::DevStatusCommand {}),
        Box::new(launch::LaunchCommand {}),
        Box::new(extension::Extension {}),
        Box::new(depend::DependsCommands{}),
    ]
}