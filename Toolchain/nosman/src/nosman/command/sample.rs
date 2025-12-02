use std::path::PathBuf;

use clap::{Arg, ArgMatches};
use crate::nosman::command::{sample, Command, CommandResult};
use crate::nosman::command::install::{InstallCommand, InstallFlags};
use crate::nosman::workspace::Workspace;

// Hashmap of sample names to package names
pub (crate) static SAMPLES: phf::Map<&'static str, &'static str> = phf::phf_map! {
    "dx12_app" => "nos.sample.dxapp",
    "vk_app" => "nos.sample.vkapp",
};

pub struct SampleCommand {
}


impl SampleCommand {
    fn run_get_sample(&self, workspace: &mut Workspace, name: &str, output_dir: &PathBuf) -> CommandResult {
        let opt_pkg_name = SAMPLES.get(name);
        if let Some(pkg_name) = opt_pkg_name {
            InstallCommand{}.run_install(workspace, pkg_name, None, &Some(output_dir.clone()), None, InstallFlags::UpdatePackageIndex | InstallFlags::InstallExactVersion)?;
            Ok(())
        } else {
            Err (crate::nosman::command::CommandError::Runtime { message: format!("Sample {} not found", name) })
        }
    }
}

pub fn get_cli() -> clap::Command {
    clap::Command::new("get-sample")
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
}

impl Command for SampleCommand {
    fn matched_args<'a>(&self, _workspace: &Workspace, args : &'a ArgMatches) -> Option<&'a ArgMatches> {
        args.subcommand_matches("get-sample")
    }

    fn run(&self, workspace: &mut Workspace, _command_name: Option<&str>, args: &ArgMatches) -> CommandResult {
        let name = args.get_one::<String>("name").unwrap();
        let output_dir = args.get_one::<String>("output_dir").map(|p| PathBuf::from(p)).unwrap_or_else(|| PathBuf::from("."));
        self.run_get_sample(workspace, name, &output_dir)
    }
}
