use std::path::PathBuf;

use clap::{ArgMatches};
use crate::nosman::command::{Command, CommandResult};
use crate::nosman::command::install::InstallCommand;

// Hashmap of sample names to package names
static SAMPLES: phf::Map<&'static str, &'static str> = phf::phf_map! {
    "dx12_app" => "nos.sample.dxapp",
    "vk_app" => "nos.sample.vkapp",
};

pub struct SampleCommand {
}


impl SampleCommand {
    fn run_get_sample(&self, name: &str, output_dir: &PathBuf) -> CommandResult {
        let opt_pkg_name = SAMPLES.get(name);
        if let Some(pkg_name) = opt_pkg_name {
            InstallCommand{}.run_install(pkg_name, None, true, output_dir, None)
        } else {
            Err (crate::nosman::command::CommandError::GenericError { message: format!("Sample {} not found", name) })
        }
    }
}

impl Command for SampleCommand {
    fn matched_args<'a>(&self, args : &'a ArgMatches) -> Option<&'a ArgMatches> {
        args.subcommand_matches("get-sample")
    }

    fn run(&self, args: &ArgMatches) -> CommandResult {
        let name = args.get_one::<String>("name").unwrap();
        let output_dir = args.get_one::<String>("output_dir").map(|p| PathBuf::from(p)).unwrap_or_else(|| PathBuf::from("."));
        self.run_get_sample(name, &output_dir)
    }
}
