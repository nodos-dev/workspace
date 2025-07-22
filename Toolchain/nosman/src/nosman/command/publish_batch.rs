use std::path::PathBuf;
use clap::{Arg, ArgAction, ArgMatches};
use colored::Colorize;
use glob_match::glob_match;

use crate::nosman::command::{get_version_check_arg, Command, CommandResult};
use crate::nosman::command::CommandError::{InvalidArgument};
use crate::nosman::command::publish::{PublishCommand, PublishOptions};
use crate::nosman::constants;
use crate::nosman::module::get_module_manifests;

use path_slash::PathExt as _;
use crate::nosman::command::unpublish::UnpublishCommand;
use crate::nosman::index::VersionCheckStrategy;
use crate::nosman::platform::{get_host_platform, Platform};
use crate::nosman::workspace::Workspace;

pub struct PublishBatchCommand {
}

impl PublishBatchCommand {
    fn run_publish_batch(&self, workspace: &Workspace, dry_run: bool, verbose: bool, remote_name: &String, repo_path: &PathBuf, compare_with: Option<&String>,
                        version_suffix: &String, version_check_strategy: &VersionCheckStrategy, vendor: Option<&String>, publisher_name: Option<&String>,
                        publisher_email: Option<&String>, release_tags: &Vec<String>, opt_target_platform: Option<&String>, release_notes: Option<&String>) -> CommandResult {
        if !repo_path.exists() {
            return Err(InvalidArgument { message: format!("Repo {} does not exist", repo_path.display()) });
        }

		let target_platform = if opt_target_platform.is_none() {
            let current_platform = get_host_platform();
            println!("{}", format!("Target platform is not provided. Using the current platform: {}", current_platform).yellow());
            current_platform
        } else {
            Platform::from_str(opt_target_platform.unwrap()).expect("Invalid target platform")
        };

        let repo_path = dunce::canonicalize(repo_path).unwrap_or_else(|e| panic!("Failed to canonicalize repo path {:?}: {}", repo_path, e));

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
                return Err(InvalidArgument { message: format!("Failed to execute git diff: {}", String::from_utf8_lossy(&output.stderr)) });
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
        let module_manifests = get_module_manifests(&repo_path, false);
        println!("Found {} modules in {}", module_manifests.len(), repo_path.display());
        for (_module_type, manifest_file_path) in module_manifests {
            let parent = manifest_file_path.parent().unwrap();
            let relative_path = parent.strip_prefix(&repo_path).unwrap();
            let (publish_options, found) = PublishOptions::from_file(&parent.join(constants::PUBLISH_OPTIONS_FILE_NAME));
            if !found {
                println!("{}", format!("Module at {} does not contain a {} file, skipping release", relative_path.display(), constants::PUBLISH_OPTIONS_FILE_NAME).dimmed());
                continue;
            }
			if let Some(targets) = publish_options.target_platforms {
                if !targets.contains(&target_platform.to_string()) {
                    println!("{}", format!("Target platform {} is not in the list of target platforms in {} for Module at {}", target_platform.to_string(), constants::PUBLISH_OPTIONS_FILE_NAME, relative_path.display()));
					continue;
				}
            }
            // If nospub.globs contain any of the changed files, add parent to to_be_published
            if changed_files_opt.is_some() {
                let changed_files = changed_files_opt.as_ref().unwrap();
                let mut found = false;
                let mut watch_globs = Vec::new();
                watch_globs.extend(publish_options.release_globs.iter());
                if let Some(triggers) = &publish_options.additional_publish_triggering_globs {
                    watch_globs.extend(triggers.iter());
                }
                let nospub_file = ".nospub".to_string();
                watch_globs.push(&nospub_file);
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
            return Ok(());
        }
        let mut published = Vec::new();
        let mut rollback = false;
        for module_root in to_be_published {
            let res = PublishCommand {}.publish(workspace, dry_run, verbose, 
                                                    &module_root, None, None, 
                                                    version_suffix, &version_check_strategy, None, 
                                                    remote_name, vendor, publisher_name, 
                                                    publisher_email, release_tags, Some(&target_platform.to_string()),
                                                    release_notes);
            if let Ok(id) = res {
                published.push(id);
            }
            else {
                println!("{}", format!("Failed to publish module at {:?}: {}", module_root, res.err().unwrap()).red());
                rollback = true;
                break;
            }
        }
        if rollback {
            println!("{}", "Rolling back published modules".red());
            for id in published {
                UnpublishCommand {}.run_unpublish(&workspace, dry_run, verbose, remote_name, &id.name, Option::from(&id.version))?
            }
            return Err(InvalidArgument { message: "Failed to publish all modules".to_string() });
        }

        Ok(())
    }
}

pub fn get_cli() -> clap::Command {
    clap::Command::new("publish-batch")
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
        .arg(Arg::new("target_platform")
            .long("target-platform")
            .help("Target architecture and operating system of the module to be published. If not provided, the current platform will be used.")
            .required(false)
        )
        .arg(Arg::new("release_notes")
            .long("release-notes")
            .help("Release notes for the release.")
            .required(false)
        )
        .arg(get_version_check_arg())
}

impl Command for PublishBatchCommand {
    fn matched_args<'a>(&self, _workspace: &Workspace, args : &'a ArgMatches) -> Option<&'a ArgMatches> {
        args.subcommand_matches("publish-batch")
    }

    fn run(&self, workspace: &mut Workspace, _command_name: Option<&str>, args: &ArgMatches) -> CommandResult {
        let dry_run = args.get_one::<bool>("dry_run").unwrap();
        let verbose = args.get_one::<bool>("verbose").unwrap();
        let remote_name = args.get_one::<String>("remote").unwrap();
        let repo_path = PathBuf::from(args.get_one::<String>("repo_path").unwrap());
        let mut opt_compare_with = args.get_one::<String>("compare_with");
        let version_suffix = args.get_one::<String>("version_suffix").unwrap();
        let vendor = args.get_one::<String>("vendor");
        let publisher_name = args.get_one::<String>("publisher_name");
        let publisher_email = args.get_one::<String>("publisher_email");
        if let Some(compare_with) = opt_compare_with {
            if compare_with.is_empty() {
                opt_compare_with = None;
            }
        }
        let release_tags_ref: Vec<&String> = args.get_many::<String>("tag").unwrap_or_default().collect();
        let release_tags: Vec<String> = release_tags_ref.iter().map(|s| s.to_string()).collect();
        let target_platform = args.get_one::<String>("target_platform");
        let release_notes = args.get_one::<String>("release_notes");
        let version_check_strategy = VersionCheckStrategy::from_str(args.get_one::<String>("version_check").unwrap().as_str());
        self.run_publish_batch(workspace, *dry_run, *verbose, &remote_name, &repo_path, opt_compare_with, &version_suffix, &version_check_strategy, 
                               vendor, publisher_name, publisher_email, &release_tags, target_platform, release_notes)
    }

    fn needs_workspace(&self) -> bool {
        true
    }
}
