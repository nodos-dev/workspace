use std::io::{Read, Write};
use std::path;
use std::path::PathBuf;
use clap::{ArgMatches};

use crate::nosman::command::{Command, CommandResult};
use crate::nosman::command::CommandError::GenericError;
use crate::nosman::index::{PackageReleaseEntry, PackageType, SemVer};

pub struct PublishBatchCommand {
}

impl PublishBatchCommand {
    fn run_publish_batch(&self, dry_run: &bool, remote_name: &String, repo_path: &PathBuf, compare_with: Option<&String>,
                        version_suffix: &String, vendor: Option<&String>, publisher_name: Option<&String>,
                        publisher_email: Option<&String>) -> CommandResult {
        Err(GenericError { message: "Not implemented".to_string()})
    }
}

impl Command for PublishBatchCommand {
    fn matched_args<'a>(&self, args : &'a ArgMatches) -> Option<&'a ArgMatches> {
        return args.subcommand_matches("publish-batch");
    }

    fn needs_workspace(&self) -> bool {
        true
    }

    fn run(&self, args: &ArgMatches) -> CommandResult {
        let dry_run = args.get_one::<bool>("dry_run").unwrap();
        let remote_name = args.get_one::<String>("remote").unwrap();
        let repo_path = args.get_one::<PathBuf>("repo_path").unwrap();
        let compare_with = args.get_one::<String>("compare_with");
        let version_suffix = args.get_one::<String>("version_suffix").unwrap();
        let vendor = args.get_one::<String>("vendor");
        let publisher_name = args.get_one::<String>("publisher_name");
        let publisher_email = args.get_one::<String>("publisher_email");
        self.run_publish_batch(&dry_run, &remote_name, &repo_path, compare_with, &version_suffix, vendor, publisher_name, publisher_email)
    }
}
