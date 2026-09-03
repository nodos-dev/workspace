mod auth;
pub mod bundle;
pub mod create;
mod deinit;
mod depend;
pub mod dev;
pub mod engine;
mod extension;
pub mod get;
pub mod info;
pub mod init;
pub mod install;
pub(crate) mod launch;
mod list;
pub mod node;
pub mod pin;
pub mod publish;
pub mod publish_batch;
pub mod publish_interrupt;
mod remove;
mod rescan;
pub mod sample;
pub mod sdk_info;
pub mod test;
mod unpublish;

use std::io;

use crate::nosman::workspace::Workspace;
use clap::{Arg, ArgMatches};
use thiserror::Error;
use crate::nosman::command::CommandError::InvalidArgument;
use crate::nosman::index::SemVer;
use crate::nosman::lang_tool::LangTool;

/// Errors are printed behind an `error:` label, so the text after it reads as
/// the sentence that follows and does not repeat the label.
#[derive(Error, Debug)]
pub enum CommandError {
    #[error("{}{}", message, blamed_file(file))]
    IO { file: String, message: String },
    #[error("{}", message)]
    InvalidArgument { message: String },
    #[error("{}", message)]
    Zip { message: String },
    #[error("{}", message)]
    Runtime { message: String },
}

/// Names the file an error is about, unless nothing useful is known about it.
fn blamed_file(file: &str) -> String {
    if file.is_empty() || file == UNKNOWN_FILE {
        String::new()
    } else {
        format!(" ({})", file)
    }
}

const UNKNOWN_FILE: &str = "Unknown";

impl From<io::Error> for CommandError {
    fn from(err: io::Error) -> Self {
        CommandError::IO {
            file: UNKNOWN_FILE.to_string(),
            message: format!("{}", err),
        }
    }
}

pub(crate) type CommandResult = Result<(), CommandError>;

pub trait Command {
    fn matched_args<'b>(
        &self,
        workspace: &Workspace,
        args: &'b ArgMatches,
    ) -> Option<&'b ArgMatches>;
    fn run(
        &self,
        workspace: &mut Workspace,
        command_name: Option<&str>,
        args: &ArgMatches,
    ) -> CommandResult;
    fn needs_workspace(&self) -> bool {
        true
    }
}

pub fn commands() -> Vec<Box<dyn Command>> {
    vec![
        Box::new(init::InitCommand {}),
        Box::new(bundle::BundleCommand {}),
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
        Box::new(dev::DevPushCommand {}),
        Box::new(dev::DevGenCommand {}),
        Box::new(dev::DevStatusCommand {}),
        Box::new(dev::DevBuildCommand {}),
        Box::new(dev::DevInitCommand {}),
        Box::new(dev::DevSetupCommand {}),
        Box::new(launch::LaunchCommand {}),
        Box::new(engine::EngineLaunchCommand {}),
        Box::new(engine::EngineListCommand {}),
        Box::new(engine::EngineStopCommand {}),
        Box::new(engine::EngineStatusCommand {}),
        Box::new(engine::EngineRestartCommand {}),
        Box::new(extension::Extension {}),
        Box::new(depend::DependCommand {}),
        Box::new(test::TestCommand {}),
        Box::new(auth::AuthCommand {}),
    ]
}

pub fn get_lang_tool_arg() -> Arg {
    Arg::new("language/tool")
        .long("language-tool")
        .short('l')
        .help("Language and tool to use")
        .value_parser(clap::builder::PossibleValuesParser::new(LangTool::POSSIBLE_VALUES))
        .default_value("cpp/cmake")
}

pub fn register_cli(app: clap::Command) -> clap::Command {
    app.subcommand(init::get_cli())
        .subcommand(bundle::get_cli())
        .subcommand(deinit::get_cli())
        .subcommand(install::get_cli())
        .subcommand(remove::get_cli())
        .subcommand(rescan::get_cli())
        .subcommand(list::get_cli())
        .subcommand(info::get_cli())
        .subcommand(sdk_info::get_cli())
        .subcommand(create::get_cli())
        .subcommand(sample::get_cli())
        .subcommand(get::get_cli())
        .subcommand(publish::get_cli())
        .subcommand(publish_batch::get_cli())
        .subcommand(unpublish::get_cli())
        .subcommand(pin::get_cli())
        .subcommand(node::get_cli())
        .subcommand(depend::get_cli())
        .subcommand(launch::get_cli())
        .subcommand(engine::get_cli())
        .subcommand(dev::get_cli())
        .subcommand(test::get_cli())
        .subcommand(auth::get_cli())
}

pub fn get_nodos_version_from_args(args: &ArgMatches) -> Result<Option<SemVer>, CommandError> {
    let nodos_version_str = args.get_one::<String>("nodos_version");
    let nodos_version = if let Some(version) = nodos_version_str {
        match SemVer::parse_from_str(version) {
            Some(v) => Some(v),
            None => return Err(InvalidArgument { message: format!("Invalid Nodos version: {}", version) }),
        }
    } else {
        None
    };
    Ok(nodos_version)
}
