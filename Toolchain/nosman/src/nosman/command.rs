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
mod sample;
mod unpublish;
mod pin;

use std::io;

use clap::ArgMatches;
use err_derive::Error;

#[derive(Debug, Error)]
pub enum CommandError {
    #[error(display = "I/O (file {}): {}", file, message)]
    IOError{ file: String, message: String },
    #[error(display = "Invalid argument: {}", message)]
    InvalidArgumentError { message: String },
    #[error(display = "Zip: {}", message)]
    ZipError { message: String },
    #[error(display = "{}", message)]
    GenericError { message: String },
}

impl From<io::Error> for CommandError {
    fn from(err: io::Error) -> Self {
        CommandError::IOError { file: "Unknown".to_string(), message: format!("{}", err) }
    }
}

pub(crate) type CommandResult = Result<bool, CommandError>;

pub trait Command {
    fn matched_args<'a>(&self, args : &'a ArgMatches) -> Option<&'a ArgMatches>;
    fn run(&self, args: &ArgMatches) -> CommandResult;
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
    ]
}
