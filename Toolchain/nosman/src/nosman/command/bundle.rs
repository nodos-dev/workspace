use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use clap::{Arg, ArgAction, ArgMatches};
use colored::Colorize;

use crate::nosman::command::get::GetCommand;
use crate::nosman::command::install::{InstallCommand, InstallFlags};
use crate::nosman::command::CommandError::{InvalidArgument, IO};
use crate::nosman::command::{Command, CommandError, CommandResult};
use crate::nosman::common;
use crate::nosman::index::PackageType;
use crate::nosman::package::PackageIdentifier;
use crate::nosman::path::get_default_engines_dir;
use crate::nosman::platform::get_host_platform;
use crate::nosman::workspace::{RescanFlags, Workspace};

pub struct BundleCommand {}

/// A zip entry has no room for the executable bit, which the engine binaries need, so
/// everywhere but Windows gets a tarball.
#[cfg(windows)]
const ARCHIVE_EXTENSION: &str = ".zip";
#[cfg(not(windows))]
const ARCHIVE_EXTENSION: &str = ".tar.gz";

/// The engine listed the modules it loads on startup under `loaded_modules` until the
/// "1.4-v1" profile schema, and under `loaded_plugins` after it.
const PROFILE_PACKAGE_LIST_KEYS: [&str; 2] = ["loaded_plugins", "loaded_modules"];

/// Reads a package to bundle, as given on the command line.
fn parse_package(arg: &str) -> Result<PackageIdentifier, CommandError> {
    match arg.rsplit_once(':') {
        Some((name, version)) if !name.is_empty() && !version.is_empty() => Ok(PackageIdentifier {
            name: name.to_string(),
            version: version.to_string(),
        }),
        _ => Err(InvalidArgument {
            message: format!("Expected a package as <name>:<version>, got '{}'", arg),
        }),
    }
}

/// Picks the Nodos release out of the requested packages. A bundle is built around exactly
/// one of them; the rest are installed into it.
fn split_release(
    packages: Vec<(PackageIdentifier, PackageType)>,
) -> Result<(PackageIdentifier, Vec<PackageIdentifier>), CommandError> {
    let mut releases = Vec::new();
    let mut rest = Vec::new();
    for (package, package_type) in packages {
        if package_type == PackageType::Nodos {
            releases.push(package);
        } else {
            rest.push(package);
        }
    }
    if releases.len() > 1 {
        let names: Vec<String> = releases.iter().map(|r| format!("{}:{}", r.name, r.version)).collect();
        return Err(InvalidArgument {
            message: format!("A bundle holds one Nodos release, got {}: {}", releases.len(), names.join(", ")),
        });
    }
    let release = releases.pop().ok_or_else(|| InvalidArgument {
        message: "None of the packages is a Nodos release. Add one, for example --package nodos:1.4.".to_string(),
    })?;
    Ok((release, rest))
}

/// The output folder name plus an extension. Not `Path::with_extension`, which would read
/// the ".2" of "Nodos-1.4.2" as an extension and replace it.
fn get_archive_path(out_dir: &Path) -> Result<PathBuf, CommandError> {
    let folder_name = out_dir.file_name().ok_or_else(|| InvalidArgument {
        message: format!(
            "Cannot name an archive after '{}'. Give --out a folder name.",
            out_dir.display()
        ),
    })?;
    let mut archive_name = folder_name.to_os_string();
    archive_name.push(ARCHIVE_EXTENSION);
    Ok(out_dir.with_file_name(archive_name))
}

/// The engine keeps the modules it loads on startup in Config/Profile.json. Writing the
/// bundled packages there is what makes them load, and it doubles as the record of what
/// the bundle holds.
fn find_profile_file(root: &Path) -> Option<PathBuf> {
    let entries = fs::read_dir(get_default_engines_dir(&root.to_path_buf())).ok()?;
    let mut profiles: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| entry.path().join("Config").join("Profile.json"))
        .filter(|profile| profile.exists())
        .collect();
    // Sorted, so a folder that ended up with more than one engine bundles the same way twice.
    profiles.sort();
    profiles.into_iter().next()
}

fn add_to_profile(profile_file: &Path, packages: &Vec<PackageIdentifier>) -> CommandResult {
    let contents = fs::read_to_string(profile_file).map_err(|e| IO {
        file: profile_file.display().to_string(),
        message: e.to_string(),
    })?;
    let mut profile: serde_json::Value = serde_json::from_str(&contents).map_err(|e| IO {
        file: profile_file.display().to_string(),
        message: e.to_string(),
    })?;

    let list_key = *PROFILE_PACKAGE_LIST_KEYS
        .iter()
        .find(|key| profile.get(*key).map(|list| list.is_array()).unwrap_or(false))
        .ok_or_else(|| IO {
            file: profile_file.display().to_string(),
            message: format!(
                "The profile has no {} array",
                PROFILE_PACKAGE_LIST_KEYS.join(" or ")
            ),
        })?;
    let loaded = profile
        .get_mut(list_key)
        .and_then(|list| list.as_array_mut())
        .unwrap();

    for package in packages {
        // The engine release brings its own modules; a bundled package with the same
        // name replaces that entry rather than being listed twice.
        loaded.retain(|m| m.get("name").and_then(|n| n.as_str()) != Some(package.name.as_str()));
        loaded.push(serde_json::json!({ "name": package.name, "version": package.version }));
    }
    loaded.sort_by_key(|m| {
        m.get("name")
            .and_then(|n| n.as_str())
            .unwrap_or_default()
            .to_string()
    });

    let updated = serde_json::to_string_pretty(&profile).map_err(|e| IO {
        file: profile_file.display().to_string(),
        message: e.to_string(),
    })?;
    fs::write(profile_file, updated).map_err(|e| IO {
        file: profile_file.display().to_string(),
        message: e.to_string(),
    })?;
    Ok(())
}

/// Every file and folder under `dir`, in a stable order, parents before their contents.
#[cfg(windows)]
fn collect_entries(dir: &Path) -> Vec<PathBuf> {
    let mut entries = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(current) = stack.pop() {
        let Ok(read_dir) = fs::read_dir(&current) else {
            continue;
        };
        for entry in read_dir.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path.clone());
            }
            entries.push(path);
        }
    }
    entries.sort();
    entries
}

#[cfg(windows)]
fn write_archive(bundle_dir: &Path, archive_file: &Path, silent: bool) -> CommandResult {
    let file = fs::File::create(archive_file).map_err(|e| IO {
        file: archive_file.display().to_string(),
        message: e.to_string(),
    })?;
    let mut writer = zip::ZipWriter::new(std::io::BufWriter::new(file));
    let options: zip::write::SimpleFileOptions =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);

    let pb = common::get_progress_bar(silent);
    pb.enable_steady_tick(std::time::Duration::from_millis(100));
    for path in collect_entries(bundle_dir) {
        let Ok(relative) = path.strip_prefix(bundle_dir) else {
            continue;
        };
        let name = relative.to_string_lossy().replace('\\', "/");
        pb.set_message(format!("Compressing: {}", name));
        if path.is_dir() {
            writer
                .add_directory(name, options)
                .map_err(|e| CommandError::Zip { message: e.to_string() })?;
            continue;
        }
        let mut source = fs::File::open(&path).map_err(|e| IO {
            file: path.display().to_string(),
            message: e.to_string(),
        })?;
        // Anything past 4 GB needs the zip64 fields.
        let size = source.metadata().map(|m| m.len()).unwrap_or(0);
        writer
            .start_file(name, options.large_file(size >= u32::MAX as u64))
            .map_err(|e| CommandError::Zip { message: e.to_string() })?;
        std::io::copy(&mut source, &mut writer)?;
    }
    let mut inner = writer
        .finish()
        .map_err(|e| CommandError::Zip { message: e.to_string() })?;
    inner.flush().map_err(|e| IO {
        file: archive_file.display().to_string(),
        message: e.to_string(),
    })?;
    pb.finish_and_clear();
    Ok(())
}

#[cfg(not(windows))]
fn write_archive(bundle_dir: &Path, archive_file: &Path, _silent: bool) -> CommandResult {
    let file = fs::File::create(archive_file).map_err(|e| IO {
        file: archive_file.display().to_string(),
        message: e.to_string(),
    })?;
    let encoder = flate2::write::GzEncoder::new(
        std::io::BufWriter::new(file),
        flate2::Compression::default(),
    );
    let mut builder = tar::Builder::new(encoder);
    builder.append_dir_all(".", bundle_dir).map_err(|e| IO {
        file: bundle_dir.display().to_string(),
        message: e.to_string(),
    })?;
    let mut inner = builder
        .into_inner()
        .map_err(|e| IO {
            file: archive_file.display().to_string(),
            message: e.to_string(),
        })?
        .finish()
        .map_err(|e| IO {
            file: archive_file.display().to_string(),
            message: e.to_string(),
        })?;
    inner.flush().map_err(|e| IO {
        file: archive_file.display().to_string(),
        message: e.to_string(),
    })?;
    Ok(())
}

impl BundleCommand {
    #[allow(clippy::too_many_arguments)]
    fn run_bundle(
        &self,
        out_dir: PathBuf,
        release: &PackageIdentifier,
        packages: Vec<PackageIdentifier>,
        without_deps: bool,
        clean: bool,
        dont_ask: bool,
        archive_file: Option<PathBuf>,
    ) -> CommandResult {
        if out_dir.exists() {
            let item_count = fs::read_dir(&out_dir).map(|dir| dir.count()).unwrap_or(0);
            if item_count > 0 {
                if !clean {
                    return Err(InvalidArgument {
                        message: format!(
                            "{} is not empty. Use --clean to bundle into it anyway.",
                            out_dir.display()
                        ),
                    });
                }
                if !dont_ask {
                    let prompt = format!(
                        "This will delete {} item{} in {}. Continue?",
                        item_count,
                        if item_count == 1 { "" } else { "s" },
                        out_dir.display()
                    );
                    if !common::ask(&prompt, false, dont_ask) {
                        return Err(CommandError::Runtime { message: "Aborted by user".to_string() });
                    }
                }
                fs::remove_dir_all(&out_dir).map_err(|e| IO {
                    file: out_dir.display().to_string(),
                    message: e.to_string(),
                })?;
            }
        }
        fs::create_dir_all(&out_dir).map_err(|e| IO {
            file: out_dir.display().to_string(),
            message: e.to_string(),
        })?;

        let out_dir = dunce::canonicalize(&out_dir).map_err(|e| IO {
            file: out_dir.display().to_string(),
            message: e.to_string(),
        })?;

        println!("Bundling {} {} into {}", release.name, release.version, out_dir.display());

        // The bundle is a workspace of its own, and it usually gets built inside another
        // one, so it is scanned into place directly: init would also fetch the whole store
        // index, which is wasted work on an empty folder.
        let mut workspace = Workspace::from_root(&out_dir);
        workspace.rescan(RescanFlags::ScanPackages)?;

        GetCommand {}.run_get(&mut workspace, &release.name, &release.version, true, dont_ask, false)?;

        let mut flags = InstallFlags::empty();
        if without_deps {
            flags |= InstallFlags::WithoutDependencies;
        }
        let module_dir = Some(PathBuf::from("Module"));
        for package in &packages {
            // The index holds the packages the store lists publicly, so ask for each one by
            // name as well; a private package the user can reach is not in that list.
            workspace.fetch_package_releases(&package.name);
        }
        InstallCommand {}.run_install_many(&mut workspace, &packages, &module_dir, None, flags)?;

        let mut installed = Vec::new();
        for package in &packages {
            // The version on the command line is a prefix: '8.0' installs 8.0.7 and '8.0.3'
            // can install 8.0.3.b1201. The profile needs the version that landed on disk.
            let entry = workspace.get_latest_local_package_for_version(&package.name, &package.version)?;
            installed.push(PackageIdentifier {
                name: package.name.clone(),
                version: entry.info.id.version.clone(),
            });
        }

        if !installed.is_empty() {
            let profile_file = find_profile_file(&out_dir).ok_or_else(|| IO {
                file: out_dir.display().to_string(),
                message: "No Engine/*/Config/Profile.json in the bundle".to_string(),
            })?;
            add_to_profile(&profile_file, &installed)?;
            println!("Wrote {} package(s) to {}", installed.len(), profile_file.display());
        }

        if let Some(archive_file) = archive_file {
            println!("Compressing to {}", archive_file.display());
            write_archive(&out_dir, &archive_file, workspace.is_silent())?;
        }

        println!("{}", format!("Bundle ready at {}", out_dir.display()).green());
        Ok(())
    }
}

impl Command for BundleCommand {
    fn matched_args<'a>(&self, _workspace: &Workspace, args: &'a ArgMatches) -> Option<&'a ArgMatches> {
        args.subcommand_matches("bundle")
    }

    fn run(&self, workspace: &mut Workspace, _command_name: Option<&str>, args: &ArgMatches) -> CommandResult {
        // Only the host platform can be bundled, so asking for another one is refused
        // instead of quietly filled with host artifacts. Releases do carry the platform they
        // were built for, so the store side is not what stops us.
        // TODO: Bundle for any platform. What is in the way:
        //   1. The lookups that pick an artifact read the host platform themselves
        //      (Index::get_latest_compatible_release and friends). They, and the callers
        //      that resolve a release, have to take the wanted platform instead.
        //   2. The archive written here follows the host (zip on Windows, tar.gz elsewhere)
        //      rather than the platform being bundled.
        //   3. Artifacts are extracted to disk and packed again from there. Unpacking a
        //      linux tar.gz on Windows loses the executable bit, and packing it back as a
        //      tarball does not bring it back.
        //   4. Registering a package loads its binary, whose name and extension follow the
        //      host as well. A foreign package cannot be loaded, and since that failure is
        //      ignored its commands go missing from the bundled workspace without a word.
        let platform = args.get_one::<String>("platform");
        if let Some(platform) = platform {
            let host = get_host_platform().to_string();
            if *platform != host {
                return Err(InvalidArgument {
                    message: format!(
                        "Only {} can be bundled here: package lookup reads the host platform.",
                        host
                    ),
                });
            }
        }

        let requested = args
            .get_many::<String>("package")
            .unwrap()
            .map(|v| parse_package(v))
            .collect::<Result<Vec<PackageIdentifier>, CommandError>>()?;

        // Which one is the Nodos release is the store's answer, not the caller's. The
        // workspace here is only borrowed for its store client; the bundle's own workspace
        // does not exist yet, and asking before creating it keeps --clean from emptying a
        // folder for a set of packages that turns out to be unusable.
        let mut classified = Vec::new();
        for package in requested {
            let package_type = workspace.fetch_package_type(&package.name)?;
            classified.push((package, package_type));
        }
        let (release, packages) = split_release(classified)?;

        let out_dir = PathBuf::from(args.get_one::<String>("out").unwrap());
        let archive_file = if args.get_flag("archive") {
            Some(get_archive_path(&out_dir)?)
        } else {
            None
        };

        self.run_bundle(
            out_dir,
            &release,
            packages,
            args.get_flag("without_deps"),
            args.get_flag("clean"),
            args.get_flag("yes_to_all"),
            archive_file,
        )
    }

    fn needs_workspace(&self) -> bool {
        false
    }
}

pub fn get_cli() -> clap::Command {
    clap::Command::new("bundle")
        .about("Create a folder holding a Nodos release and a set of packages.")
        .after_help("Example:\n  \
            nodos bundle --package nodos:1.4 --package nos.sys.vulkan:8.0 --out MyBundle")
        .arg(
            Arg::new("package")
                .help("Package to bundle, as <name>:<version>. Repeat for each package. Exactly one of them has to be a Nodos release, such as nodos:1.4; the rest are installed into it.")
                .long("package")
                .action(ArgAction::Append)
                .required(true),
        )
        .arg(
            Arg::new("out")
                .help("Folder to create the bundle in.")
                .long("out")
                .short('o')
                .required(true),
        )
        .arg(
            Arg::new("archive")
                .help("Also write <out>.zip (Windows) or <out>.tar.gz (unix) next to the bundle folder.")
                .long("archive")
                .action(ArgAction::SetTrue)
                .num_args(0),
        )
        .arg(
            Arg::new("without_deps")
                .help("Do not install the dependencies of the bundled packages.")
                .long("without-deps")
                .action(ArgAction::SetTrue)
                .num_args(0),
        )
        .arg(
            Arg::new("clean")
                .help("Empty the output folder first if something is already there.")
                .long("clean")
                .action(ArgAction::SetTrue)
                .num_args(0),
        )
        .arg(
            Arg::new("yes_to_all")
                .help("Do not ask for confirmation. Execute default behaviour.")
                .short('y')
                .long("yes")
                .action(ArgAction::SetTrue)
                .num_args(0),
        )
        .arg(
            Arg::new("platform")
                .help("Platform to bundle for, as <arch>-<os> (e.g. x86_64-windows). Only the host platform is supported.")
                .long("platform"),
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_profile(folder: &Path, contents: serde_json::Value) -> PathBuf {
        let profile_file = folder.join("Profile.json");
        fs::write(&profile_file, contents.to_string()).unwrap();
        profile_file
    }

    fn read_profile(profile_file: &Path) -> serde_json::Value {
        serde_json::from_str(&fs::read_to_string(profile_file).unwrap()).unwrap()
    }

    fn bundled(name: &str, version: &str) -> Vec<PackageIdentifier> {
        vec![PackageIdentifier { name: name.to_string(), version: version.to_string() }]
    }

    #[test]
    fn parse_package_reads_name_and_version() {
        let package = parse_package("nos.sys.vulkan:8.0.3").unwrap();
        assert_eq!(package.name, "nos.sys.vulkan");
        assert_eq!(package.version, "8.0.3");
    }

    #[test]
    fn parse_package_rejects_incomplete_arguments() {
        for arg in ["nos.sys.vulkan", "nos.sys.vulkan:", ":8.0.3", ""] {
            assert!(parse_package(arg).is_err(), "'{}' should not parse", arg);
        }
    }

    #[test]
    fn split_release_separates_the_nodos_package() {
        let requested = vec![
            (PackageIdentifier { name: "nos.sys.vulkan".to_string(), version: "8.0".to_string() }, PackageType::Subsystem),
            (PackageIdentifier { name: "nodos".to_string(), version: "1.4".to_string() }, PackageType::Nodos),
            (PackageIdentifier { name: "nos.utilities".to_string(), version: "3".to_string() }, PackageType::Plugin),
        ];

        let (release, packages) = split_release(requested).unwrap();

        assert_eq!(release.name, "nodos");
        assert_eq!(release.version, "1.4");
        let names: Vec<&str> = packages.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, vec!["nos.sys.vulkan", "nos.utilities"]);
    }

    #[test]
    fn split_release_needs_exactly_one_release() {
        let none = vec![(
            PackageIdentifier { name: "nos.sys.vulkan".to_string(), version: "8.0".to_string() },
            PackageType::Subsystem,
        )];
        assert!(split_release(none).is_err());

        let two = vec![
            (PackageIdentifier { name: "nodos".to_string(), version: "1.4".to_string() }, PackageType::Nodos),
            (PackageIdentifier { name: "nodos-zd".to_string(), version: "1.4".to_string() }, PackageType::Nodos),
        ];
        assert!(split_release(two).is_err());
    }

    #[test]
    fn archive_path_keeps_a_version_in_the_folder_name() {
        let archive = get_archive_path(Path::new("out/Nodos-1.4.2")).unwrap();
        assert_eq!(archive, PathBuf::from(format!("out/Nodos-1.4.2{}", ARCHIVE_EXTENSION)));
    }

    #[test]
    fn archive_path_needs_a_folder_name() {
        assert!(get_archive_path(Path::new(".")).is_err());
    }

    #[test]
    fn write_archive_keeps_files_and_empty_folders() {
        let bundle_dir = tempfile::tempdir().unwrap();
        let config_dir = bundle_dir.path().join("Engine").join("Config");
        fs::create_dir_all(&config_dir).unwrap();
        fs::create_dir_all(bundle_dir.path().join("Module")).unwrap();
        write_profile(&config_dir, serde_json::json!({ "loaded_plugins": [] }));

        let out_dir = tempfile::tempdir().unwrap();
        let archive_file = get_archive_path(&out_dir.path().join("Bundle-1.4.2")).unwrap();
        write_archive(bundle_dir.path(), &archive_file, true).unwrap();

        assert!(fs::metadata(&archive_file).unwrap().len() > 0);
        #[cfg(windows)]
        {
            let archive = zip::ZipArchive::new(fs::File::open(&archive_file).unwrap()).unwrap();
            let names: Vec<&str> = archive.file_names().collect();
            assert!(names.contains(&"Module/"), "{:?}", names);
            assert!(names.contains(&"Engine/Config/Profile.json"), "{:?}", names);
        }
    }

    #[test]
    fn add_to_profile_replaces_the_release_entry() {
        let folder = tempfile::tempdir().unwrap();
        let profile_file = write_profile(
            folder.path(),
            serde_json::json!({
                "schema_version": "1.4-v1",
                "loaded_plugins": [
                    { "name": "nos.sys.vulkan", "version": "8.0.1" },
                    { "name": "nos.utilities", "version": "3.0.0" }
                ]
            }),
        );

        add_to_profile(&profile_file, &bundled("nos.sys.vulkan", "8.0.7")).unwrap();

        let profile = read_profile(&profile_file);
        assert_eq!(profile["schema_version"], "1.4-v1");
        let loaded = profile["loaded_plugins"].as_array().unwrap();
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0]["name"], "nos.sys.vulkan");
        assert_eq!(loaded[0]["version"], "8.0.7");
        assert_eq!(loaded[1]["name"], "nos.utilities");
    }

    #[test]
    fn add_to_profile_writes_the_key_the_profile_already_uses() {
        let folder = tempfile::tempdir().unwrap();
        let profile_file = write_profile(
            folder.path(),
            serde_json::json!({ "loaded_modules": [ { "name": "nos.reflect", "version": "3.0.0" } ] }),
        );

        add_to_profile(&profile_file, &bundled("nos.utilities", "3.18.1")).unwrap();

        let profile = read_profile(&profile_file);
        assert!(profile.get("loaded_plugins").is_none());
        let loaded = profile["loaded_modules"].as_array().unwrap();
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[1]["name"], "nos.utilities");
        assert_eq!(loaded[1]["version"], "3.18.1");
    }

    #[test]
    fn add_to_profile_fails_without_a_package_list() {
        let folder = tempfile::tempdir().unwrap();
        let profile_file = write_profile(folder.path(), serde_json::json!({ "schema_version": "1.4-v1" }));

        assert!(add_to_profile(&profile_file, &bundled("nos.utilities", "3.18.1")).is_err());
    }

    #[test]
    fn find_profile_file_finds_the_engine_profile() {
        let root = tempfile::tempdir().unwrap();
        let config_dir = root.path().join("Engine").join("1.4.0.b4846").join("Config");
        fs::create_dir_all(&config_dir).unwrap();
        let profile_file = write_profile(&config_dir, serde_json::json!({ "loaded_modules": [] }));

        assert_eq!(find_profile_file(root.path()), Some(profile_file));
    }
}
