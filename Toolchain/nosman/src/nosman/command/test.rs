use std::fs;
use std::path::{Path, PathBuf};
use clap::ArgMatches;
use colored::Colorize;
use crate::nosman::command::{Command, CommandResult};
use crate::nosman::workspace::Workspace;
use crate::nosman::{package};
use std::fs::File;
use crate::nosman::package::PackageInfo;
use std::time::Instant;
use std::time::Duration;

pub struct TestCommand {}

struct TestCase {
    package_name: String, // now the package name from manifest
    package_dir: PathBuf,
    test_graph: PathBuf,
}

struct TestResult {
    plugin_name: String,
    plugin_dir: PathBuf,
    test_graph: PathBuf,
    exit_code: i32,
    duration: std::time::Duration,
    output_file: Option<PathBuf>,
}

impl TestCommand {
    fn collect_tests(plugins_folder: &PathBuf) -> Vec<TestCase> {
        let mut tests = Vec::new();
        let manifests = package::get_package_manifests(plugins_folder, true);
        for (_plugin_type, manifest_path) in manifests {
            let package_dir = manifest_path.parent().unwrap().to_path_buf();
            let tests_dir = package_dir.join("Tests");
            if !tests_dir.exists() || !tests_dir.is_dir() {
                continue;
            }
            // Read manifest to get package name from 'info' field (PackageInfo)
            let package_name = match File::open(&manifest_path)
                .ok()
                .and_then(|f| serde_json::from_reader::<_, serde_json::Value>(f).ok())
                .and_then(|json| json.get("info").cloned())
                .and_then(|info_val| serde_json::from_value::<PackageInfo>(info_val).ok())
            {
                Some(info) => info.id.name,
                None => String::new(),
            };
            let entries = match fs::read_dir(&tests_dir) {
                Ok(e) => e,
                Err(_) => continue,
            };
            for entry in entries {
                if let Ok(entry) = entry {
                    let path = entry.path();
                    if path.is_file() && path.extension().map_or(false, |ext| ext == "nosa" || ext == "nos") {
                        tests.push(TestCase {
                            package_name: package_name.clone(),
                            package_dir: package_dir.clone(),
                            test_graph: path,
                        });
                    }
                }
            }
        }
        tests
    }

    fn run_nodos_graph_test(workspace: &Workspace, engine_dir: Option<&Path>, graph_path: &Path, timeout: Duration) -> Result<(i32, Option<PathBuf>), String> {
        let engine_path = if let Some(dir) = engine_dir {
            let mut engine_path = dir.join("Binaries").join("nosLauncher");
            if cfg!(target_os = "windows") {
                engine_path = engine_path.with_extension("exe");
            }
            if !engine_path.exists() {
                return Err(format!("nosLauncher not found at {}", engine_path.display()));
            }
            engine_path
        } else {
            let engines_dir = crate::nosman::path::get_default_engines_dir(&workspace.root);
            if !engines_dir.exists() {
                return Err("No installed Nodos engine found in workspace.".to_string());
            }
            let mut opt_engine_path = None;
            for entry in fs::read_dir(&engines_dir).map_err(|e| e.to_string())? {
                let entry = entry.map_err(|e| e.to_string())?;
                let path = entry.path();
                if !path.is_dir() {
                    continue;
                }
                let mut engine_path = path.join("Binaries").join("nosLauncher");
                if cfg!(target_os = "windows") {
                    engine_path = engine_path.with_extension("exe");
                }
                if engine_path.exists() {
                    opt_engine_path = Some(engine_path);
                    break;
                }
            }
            opt_engine_path.ok_or("No nosLauncher found in any engine Binaries folder.".to_string())?
        };
        use wait_timeout::ChildExt;
        
        // Create output file in system temp directory
        let temp_dir = std::env::temp_dir();
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let output_file_path = temp_dir.join(format!("nodos_test_{}_{}.log", 
            graph_path.file_stem().unwrap_or_default().to_string_lossy(),
            timestamp));
        let output_file = File::create(&output_file_path)
            .map_err(|e| format!("Failed to create output file: {}", e))?;
        let output_file_clone = output_file.try_clone()
            .map_err(|e| format!("Failed to clone file handle: {}", e))?;
        
        let mut child = std::process::Command::new(&engine_path)
            .arg("--load-graph")
            .arg(graph_path)
            .arg("--load-graph-plugins")
            .stdout(output_file)
            .stderr(output_file_clone)
            .spawn()
            .map_err(|e| e.to_string())?;
        let result = match child.wait_timeout(timeout).map_err(|e| e.to_string())? {
            Some(status) => {
                let exit_code = status.code().unwrap_or(-1);
                if exit_code == 0 {
                    // Test passed, delete the output file
                    let _ = fs::remove_file(&output_file_path);
                    Ok((exit_code, None))
                } else {
                    // Test failed, keep the output file
                    Ok((exit_code, Some(output_file_path)))
                }
            }
            None => {
                // Timeout expired, kill the process and keep the output file
                let _ = child.kill();
                let _ = child.wait();
                Ok((-1, Some(output_file_path)))
            }
        };
        result
    }

    fn print_summary(results: &[TestResult], workspace: &Workspace) {
        use colored::*;
        use std::collections::BTreeMap;
        println!("\n{}", "Test Results".bold().underline());
        // Group by plugin name and plugin_dir
        let mut grouped: BTreeMap<(&str, &PathBuf), Vec<&TestResult>> = BTreeMap::new();
        for result in results {
            grouped.entry((result.plugin_name.as_str(), &result.plugin_dir)).or_default().push(result);
        }
        for ((plugin, plugin_dir), tests) in grouped.iter() {
            let rel_dir = workspace.root.canonicalize()
                .ok()
                .and_then(|canonical_root| {
                    plugin_dir.canonicalize()
                        .ok()
                        .and_then(|canonical_plugin| pathdiff::diff_paths(&canonical_plugin, &canonical_root))
                })
                .unwrap_or_else(|| plugin_dir.to_path_buf())
                .display()
                .to_string();
            println!("\n{} ({})", plugin.bold(), rel_dir.dimmed());
            for result in tests {
                let (icon, status) = if result.exit_code == 0 {
                    ("✓".green(), "PASS".green())
                } else {
                    ("✗".red(), "FAIL".red())
                };
                let test = result.test_graph.file_name().unwrap().to_string_lossy().dimmed();
                print!("  {} {}  {}  ({:?})", icon, status, test, result.duration);
                if let Some(ref output_file) = result.output_file {
                    print!(" ({})", output_file.display().to_string().dimmed());
                }
                println!();
            }
        }
        let passed = results.iter().filter(|r| r.exit_code == 0).count();
        let total = results.len();
        println!(
            "\n{} of {} tests passed.",
            passed.to_string().bold().green(),
            total.to_string().bold()
        );
    }
}

pub fn get_cli() -> clap::Command {
    clap::Command::new("test")
        .about("Enumerate plugins in a folder, look under Tests folder of each plugin, and run nosLauncher with --load-graph for each graph file.")
        .arg(clap::Arg::new("plugins_folder")
            .help("Path to the folder containing plugins (default: workspace root)")
            .long("plugins-folder")
            .short('p')
            .required(false)
        )
        .arg(clap::Arg::new("engine_dir")
            .help("Path to the engine directory to use for nosLauncher (default: auto-detect from workspace)")
            .long("engine-dir")
            .short('e')
            .required(false)
        )
        .arg(clap::Arg::new("timeout")
            .help("Timeout in seconds for each test")
            .long("timeout")
            .short('t')
            .required(false)
            .value_parser(clap::value_parser!(u64))
            .default_value("30")
        )
}

impl Command for TestCommand {
    fn matched_args<'a>(&self, _workspace: &Workspace, args: &'a ArgMatches) -> Option<&'a ArgMatches> {
        args.subcommand_matches("test")
    }

    fn run(&self, workspace: &mut Workspace, _command_name: Option<&str>, args: &ArgMatches) -> CommandResult {
        let plugins_folder = args.get_one::<String>("plugins_folder")
            .map(PathBuf::from)
            .unwrap_or_else(|| workspace.root.clone());
        let engine_dir = args.get_one::<String>("engine_dir").map(PathBuf::from);
        let timeout_secs = args.get_one::<u64>("timeout").copied().unwrap_or(30);
        let timeout = Duration::from_secs(timeout_secs);
        let tests = Self::collect_tests(&plugins_folder);
        if tests.is_empty() {
            println!("{}", "No plugins with tests found.".yellow());
            return Ok(());
        }
        println!("Found {} test(s) in {} plugin(s).", tests.len(), tests.iter().map(|t| &t.package_name).collect::<std::collections::HashSet<_>>().len());
        let mut results = Vec::new();
        for test in &tests {
            let test_graph_relpath = test.test_graph
                .strip_prefix(&plugins_folder)
                .unwrap_or(&test.test_graph);
            println!("{} {} (plugin: {})", "Running test graph:".green(), test_graph_relpath.display(), test.package_name);
            let start = Instant::now();
            let (exit_code, output_file) = Self::run_nodos_graph_test(workspace, engine_dir.as_deref(), &test.test_graph, timeout)
                .unwrap_or_else(|e| {
                    eprintln!("{} {}: {}", "Error when running test graph".red(), test_graph_relpath.display(), e);
                    (-1, None)
                });
            let duration = start.elapsed();
            results.push(TestResult {
                plugin_name: test.package_name.clone(),
                plugin_dir: test.package_dir.clone(),
                test_graph: test.test_graph.clone(),
                exit_code,
                duration,
                output_file,
            });
        }
        Self::print_summary(&results, workspace);
        if results.iter().any(|r| r.exit_code != 0) {
            return Err(crate::nosman::command::CommandError::Runtime {
                message: format!("{} test(s) failed.", results.iter().filter(|r| r.exit_code != 0).count()),
            });
        }
        Ok(())
    }

    fn needs_workspace(&self) -> bool {
        true
    }
} 