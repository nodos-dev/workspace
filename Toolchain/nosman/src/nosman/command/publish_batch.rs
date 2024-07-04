use std::path::PathBuf;
use clap::{ArgMatches};
use colored::Colorize;
use glob_match::glob_match;

use crate::nosman::command::{Command, CommandResult};
use crate::nosman::command::CommandError::{InvalidArgumentError};
use crate::nosman::command::publish::{PublishCommand, PublishOptions};
use crate::nosman::constants;
use crate::nosman::module::get_module_manifests;

use path_slash::PathExt as _;

pub struct PublishBatchCommand {
}

impl PublishBatchCommand {
    fn run_publish_batch(&self, dry_run: bool, verbose: bool, remote_name: &String, repo_path: &PathBuf, compare_with: Option<&String>,
                        version_suffix: &String, vendor: Option<&String>, publisher_name: Option<&String>,
                        publisher_email: Option<&String>) -> CommandResult {
        if !repo_path.exists() {
            return Err(InvalidArgumentError { message: format!("Repo {} does not exist", repo_path.display()) });
        }

        let repo_path = dunce::canonicalize(repo_path).expect(format!("Failed to canonicalize repo path: {}", repo_path.display()).as_str());

        let mut changed_files_opt: Option<Vec<PathBuf>> = None;
        if let Some(reference) = compare_with {
            println!("Checking for changes between {} and HEAD", reference);
            let mut changed_files = vec![];
            let output = std::process::Command::new("git")
                .arg("diff")
                .arg("--name-only")
                .arg(format!("{}..{}", reference, "HEAD"))
                .current_dir(&repo_path)
                .output()
                .expect("Failed to execute git diff");
            if !output.status.success() {
                return Err(InvalidArgumentError { message: format!("Failed to execute git diff: {}", String::from_utf8_lossy(&output.stderr)) });
            }
            let output = String::from_utf8_lossy(&output.stdout);
            for line in output.lines() {
                println!("{}", format!("Changed file: {}", line).dimmed());
                changed_files.push(PathBuf::from(line.to_string()));
            }
            changed_files_opt = Some(changed_files);
        }
        else {
            println!("All modules under {} will be published", repo_path.display());
        }

        // Find all modules in the repo
        let mut to_be_published: Vec<PathBuf> = vec![];
        let module_manifests = get_module_manifests(&repo_path);
        println!("Found {} modules in {}", module_manifests.len(), repo_path.display());
        for (_module_type, manifest_file_path) in module_manifests {
            let parent = manifest_file_path.parent().unwrap();
            let relative_path = parent.strip_prefix(&repo_path).unwrap();
            let (nospub, found) = PublishOptions::from_file(&parent.join(constants::PUBLISH_OPTIONS_FILE_NAME));
            if !found {
                println!("{}", format!("Module at {} does not contain a {} file, skipping release", relative_path.display(), constants::PUBLISH_OPTIONS_FILE_NAME).dimmed());
                continue;
            }
            // If nospub.globs contain any of the changed files, add parent to to_be_published
            if changed_files_opt.is_some() {
                let changed_files = changed_files_opt.as_ref().unwrap();
                let mut found = false;
                let mut watch_globs = Vec::new();
                watch_globs.extend(nospub.release_globs.iter());
                if let Some(triggers) = &nospub.additional_publish_triggering_globs {
                    watch_globs.extend(triggers.iter());
                }
                for glob in &watch_globs {
                    // Prepend the parent path to the glob
                    let local = relative_path.join(glob);
                    let glob_str = local.to_slash_lossy().to_string();
                    for changed_file in changed_files {
                        if glob_match(glob_str.as_str(), changed_file.to_str().unwrap()) {
                            found = true;
                            break;
                        }
                    }
                    if found {
                        break;
                    }
                }
                if !found {
                    continue;
                }
            }
            to_be_published.push(parent.to_path_buf());
        }

        for module_root in &to_be_published {
            println!("{}", format!("Will publish module at {:?}", module_root).green());
        }

        if to_be_published.is_empty() {
            println!("{}", "No modules need publishing".yellow());
            return Ok(true);
        }
        for module_root in to_be_published {
            PublishCommand {}.run_publish(dry_run, verbose, &module_root, None, None, version_suffix, None, remote_name, vendor, publisher_name, publisher_email)?;
        }

        return Ok(true);
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
        let verbose = args.get_one::<bool>("verbose").unwrap();
        let remote_name = args.get_one::<String>("remote").unwrap();
        let repo_path = PathBuf::from(args.get_one::<String>("repo_path").unwrap());
        let compare_with = args.get_one::<String>("compare_with");
        let version_suffix = args.get_one::<String>("version_suffix").unwrap();
        let vendor = args.get_one::<String>("vendor");
        let publisher_name = args.get_one::<String>("publisher_name");
        let publisher_email = args.get_one::<String>("publisher_email");
        self.run_publish_batch(*dry_run, *verbose, &remote_name, &repo_path, compare_with, &version_suffix, vendor, publisher_name, publisher_email)
    }
}
