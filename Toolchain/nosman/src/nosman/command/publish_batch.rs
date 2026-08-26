use std::path::PathBuf;
use clap::{Arg, ArgAction, ArgMatches};
use crate::nosman::command::{Command, CommandResult};
use crate::nosman::command::CommandError::{InvalidArgument};
use crate::nosman::command::publish::{PublishCommand, PublishOptions};
use crate::nosman::{constants, git};

use crate::nosman::command::unpublish::UnpublishCommand;
use crate::nosman::index::SemVer;
use crate::nosman::package::{get_package_info_from_manifest, get_package_manifests};
use crate::nosman::platform::{get_host_platform, Platform};
use crate::nosman::workspace::Workspace;
use crate::nosman::ui;

pub struct PublishBatchCommand {
}

impl PublishBatchCommand {
    #[allow(clippy::too_many_arguments)]
    fn run_publish_batch(&self, workspace: &mut Workspace, dry_run: bool, verbose: bool, directory: &PathBuf,
                         version_suffix: &str, release_tags: &Vec<String>, opt_target_platform: Option<&String>,
                         publish_all: bool, packages: Vec<&String>,
                         visibility: nodos_store_client::PackageVisibility,
                         create_tag: bool, push_tag: bool, fetch_tags: bool,
    ) -> CommandResult {
        if !directory.exists() {
            return Err(InvalidArgument { message: format!("Repo {} does not exist", directory.display()) });
        }

		let target_platform = match opt_target_platform {
            Some(platform_str) => Platform::from_str(platform_str).expect("Invalid target platform"),
            None => {
                let current_platform = get_host_platform();
                ui::detail(format!("no target platform given, using this one: {}", current_platform));
                current_platform
            }
        };

        let directory = dunce::canonicalize(directory).unwrap_or_else(|e| panic!("Failed to canonicalize directory {:?}: {}", directory, e));

        // Find all modules in the repo
        let mut to_be_published: Vec<PathBuf> = vec![];
        let package_manifests = get_package_manifests(&directory, false);
        ui::step("Scanned", format!("{} in {}", ui::plural(package_manifests.len(), "package"), directory.display()));
        for (_plugin_type, manifest_file_path) in package_manifests {
            let parent = manifest_file_path.parent().unwrap();
            let relative_path = parent.strip_prefix(&directory).unwrap();
            let (publish_options, found) = PublishOptions::from_file(&parent.join(constants::PUBLISH_OPTIONS_FILE_NAME));
            if !found {
                ui::step_skipped("Skipped", format!("{}, it has no {}", relative_path.display(), constants::PUBLISH_OPTIONS_FILE_NAME));
                continue;
            }
            if let Some(targets) = publish_options.target_platforms {
                if !targets.contains(&target_platform.to_string()) {
                    ui::step_skipped("Skipped", format!("{}, {} does not list target platform {}", relative_path.display(), constants::PUBLISH_OPTIONS_FILE_NAME, target_platform));
					continue;
				}
            }
            let package_info = get_package_info_from_manifest(&manifest_file_path)
                .map_err(|e| InvalidArgument { message: format!("Failed to get package info from manifest at {}: {}", relative_path.display(), e) })?;
            if !packages.is_empty() && !packages.contains(&&package_info.id.name) {
                ui::step_skipped("Skipped", format!("{}, it was not asked for", package_info.id.name));
                continue;
            }
            let mut skip = false;
            if !publish_all {

                workspace.fetch_package_releases(&package_info.id.name);
                let publish_version_str = package_info.id.version + version_suffix;
                let publish_version = SemVer::parse_from_str(&publish_version_str)
                    .ok_or(InvalidArgument { message: format!("Failed to parse version string: {}", publish_version_str) })?;
                let publish_version_excl_build_no = SemVer::new(publish_version.major, publish_version.minor, publish_version.patch, None);
                for existing_release in workspace.index_cache.get_package_releases(&package_info.id.name) {
                    if let Some(existing_platform) = &existing_release.platform {
                        let existing_release_platform = Platform::from_str(existing_platform);
                        if existing_release_platform.is_some() && existing_release_platform.unwrap() != target_platform {
                            continue;
                        }
                        let existing_release_ver = SemVer::parse_from_str(&existing_release.version);
                        if existing_release_ver.is_none() {
                            continue;
                        }
                        let existing_release_ver = existing_release_ver.unwrap();
                        let existing_release_ver_excl_build_no = SemVer::new(existing_release_ver.major, existing_release_ver.minor, existing_release_ver.patch, None);
                        if existing_release_ver_excl_build_no == publish_version_excl_build_no {
                            ui::step_skipped("Skipped", format!("release {:?}, it is already published", existing_release));
                            skip = true;
                            break;
                        }
                    }
                }
            }
            if !skip {
                to_be_published.push(parent.to_path_buf());
            }
        }

        for package_root in &to_be_published {
            ui::step("Selected", format!("{:?}", package_root));
        }

        if to_be_published.is_empty() {
            ui::step_skipped("Nothing", "to publish, every package is already up to date");
            return Ok(());
        }
        // Fetch tags / unshallow once for the whole repo here, so each
        // per-package publish below can skip it (fetch_tags = false).
        if fetch_tags && !dry_run {
            PublishCommand::ensure_tags_fetched(&directory, verbose);
        }
        let mut published = Vec::new();
        let mut rollback = false;
        for package_root in to_be_published {
            // No `--changelog` for batch publishes; `publish` auto-picks each
            // package's CHANGELOG.md or git commit log when present.
            let res = PublishCommand {}.publish(workspace, dry_run, verbose,
                                                &package_root, None, None,
                                                version_suffix, None, release_tags, Some(&target_platform.to_string()),
                                                visibility, None, create_tag, push_tag, false);
            if let Ok(outcome) = res {
                published.push(outcome);
            }
            else {
                ui::error(format!("could not publish the package at {:?}: {}", package_root, res.err().unwrap()));
                rollback = true;
                break;
            }
        }
        if rollback {
            ui::warn("rolling back the packages that were published");
            for outcome in published {
                UnpublishCommand {}.run_unpublish(workspace, dry_run, false, &outcome.id.name, Option::from(&outcome.id.version))?;
                if let Some(tag) = &outcome.created_tag {
                    ui::step("Deleting", format!("git tag {}", tag));
                    git::delete_tag(&directory, tag, push_tag);
                }
            }
            return Err(InvalidArgument { message: "Failed to publish all packages".to_string() });
        }

        Ok(())
    }
}

pub fn get_cli() -> clap::Command {
    clap::Command::new("publish-batch")
        .about("Publish all/changed packages under the git repository.")
        .after_help(format!("This command will publish all/changed packages under the git repository to the Nodos Store.\n\
    It will use the {} files to add the files to the release.", constants::PUBLISH_OPTIONS_FILE_NAME))
        .arg(Arg::new("directory")
            .long("directory")
            .alias("repo-path")
            .short('d')
            .help("The directory of the plugins to be published. If not provided, the current directory will be used.")
            .default_value(".")
        )
        .arg(Arg::new("version_suffix")
            .long("version-suffix")
            .help("Suffix to append to the version of the packages to be published.")
            .default_value("")
        )
        .arg(Arg::new("dry_run")
            .action(ArgAction::SetTrue)
            .long("dry-run")
            .help("Do not actually publish the package, just show what would be done.")
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
        .arg(Arg::new("publish_all")
            .action(ArgAction::SetTrue)
            .long("publish-all")
            .help("Triggers publish routine for all packages, even if their version have not changed. Command could fail if version check is not permissive enough.")
            .num_args(0)
            .required(false)
        )
        .arg(Arg::new("packages")
            .long("packages")
            .help("Publish only the specified packages. Can be specified multiple times.")
            .required(false)
            .action(ArgAction::Append)
            .num_args(1..)
        )
        .arg(Arg::new("visibility")
            .long("visibility")
            .value_parser(clap::builder::PossibleValuesParser::new(["public", "private"]))
            .default_value("public")
            .help("Visibility applied to packages created by this run. Existing packages keep their current visibility (manage from the Nodos Store dashboard).")
            .required(false)
        )
        .arg(Arg::new("no_tag")
            .action(ArgAction::SetTrue)
            .long("no-tag")
            .help("Do not create release-<name>-<version>-<target> git tags after successful publishes.")
            .num_args(0)
            .required(false)
        )
        .arg(Arg::new("no_push_tag")
            .action(ArgAction::SetTrue)
            .long("no-push-tag")
            .help("Create release git tags locally but do not push them to the remote.")
            .num_args(0)
            .required(false)
        )
        .arg(Arg::new("no_fetch_tags")
            .action(ArgAction::SetTrue)
            .long("no-fetch-tags")
            .help("Do not fetch tags / unshallow the repo before generating changelogs. By default nosman fetches tags (and unshallows a shallow clone) so changelog generation works under CI's shallow checkouts.")
            .num_args(0)
            .required(false)
        )
}

impl Command for PublishBatchCommand {
    fn matched_args<'a>(&self, _workspace: &Workspace, args : &'a ArgMatches) -> Option<&'a ArgMatches> {
        args.subcommand_matches("publish-batch")
    }

    fn run(&self, workspace: &mut Workspace, _command_name: Option<&str>, args: &ArgMatches) -> CommandResult {
        let dry_run = args.get_one::<bool>("dry_run").unwrap();
        let verbose = ui::is_verbose();
        let directory = PathBuf::from(args.get_one::<String>("directory").unwrap());
        let version_suffix = args.get_one::<String>("version_suffix").unwrap();
        let release_tags_ref: Vec<&String> = args.get_many::<String>("tag").unwrap_or_default().collect();
        let release_tags: Vec<String> = release_tags_ref.iter().map(|s| s.to_string()).collect();
        let target_platform = args.get_one::<String>("target_platform");
        let publish_all = args.get_one::<bool>("publish_all").unwrap();
        let packages: Vec<&String> = args.get_many::<String>("packages").unwrap_or_default().collect();
        let visibility = nodos_store_client::PackageVisibility::from_str(
            args.get_one::<String>("visibility").map(String::as_str).unwrap_or("public"),
        );
        let create_tag = !*args.get_one::<bool>("no_tag").unwrap();
        let push_tag = !*args.get_one::<bool>("no_push_tag").unwrap();
        let fetch_tags = !*args.get_one::<bool>("no_fetch_tags").unwrap();
        self.run_publish_batch(
            workspace,
            *dry_run,
            verbose,
            &directory,
            version_suffix,
            &release_tags,
            target_platform,
            *publish_all,
            packages,
            visibility,
            create_tag,
            push_tag,
            fetch_tags,
        )
    }

    fn needs_workspace(&self) -> bool {
        false
    }
}
