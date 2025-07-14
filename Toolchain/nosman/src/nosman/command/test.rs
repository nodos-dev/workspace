use std::fs;
use std::path::{Path, PathBuf};
use clap::ArgMatches;
use colored::Colorize;
use crate::nosman::command::{Command, CommandResult};
use crate::nosman::workspace::Workspace;
use crate::nosman::module;
use std::fs::File;
use crate::nosman::module::ModuleInfo;
use std::time::Instant;
use std::time::Duration;

pub struct TestCommand {}

struct TestCase {
    module_name: String, // now the package name from manifest
    module_dir: PathBuf,
    test_graph: PathBuf,
}

struct TestResult {
    module_name: String,
    module_dir: PathBuf,
    test_graph: PathBuf,
    exit_code: i32,
    duration: std::time::Duration,
}

impl TestCommand {
    fn collect_tests(modules_folder: &PathBuf) -> Vec<TestCase> {
        let mut tests = Vec::new();
        let manifests = module::get_module_manifests(modules_folder, true);
        for (_module_type, manifest_path) in manifests {
            let module_dir = manifest_path.parent().unwrap().to_path_buf();
            let tests_dir = module_dir.join("Tests");
            if !tests_dir.exists() || !tests_dir.is_dir() {
                continue;
            }
            // Read manifest to get package name from 'info' field (ModuleInfo)
            let package_name = match File::open(&manifest_path)
                .ok()
                .and_then(|f| serde_json::from_reader::<_, serde_json::Value>(f).ok())
                .and_then(|json| json.get("info").cloned())
                .and_then(|info_val| serde_json::from_value::<ModuleInfo>(info_val).ok())
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
                    if path.is_file() {
                        tests.push(TestCase {
                            module_name: package_name.clone(),
                            module_dir: module_dir.clone(),
                            test_graph: path,
                        });
                    }
                }
            }
        }
        tests
    }

    fn run_nodos_graph_test(workspace: &Workspace, engine_dir: Option<&Path>, graph_path: &Path, timeout: Duration) -> Result<i32, String> {
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
        let mut child = std::process::Command::new(&engine_path)
            .arg("--load-graph")
            .arg(graph_path)
            .arg("--load-graph-plugins")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map_err(|e| e.to_string())?;
        match child.wait_timeout(timeout).map_err(|e| e.to_string())? {
            Some(status) => Ok(status.code().unwrap_or(-1)),
            None => {
                // Timeout expired, kill the process
                let _ = child.kill();
                let _ = child.wait();
                Err(format!("Test timed out after {:?}", timeout))
            }
        }
    }

    fn print_summary(results: &[TestResult], workspace: &Workspace) {
        use colored::*;
        use std::collections::BTreeMap;
        println!("\n{}", "Test Results".bold().underline());
        // Group by module name and module_dir
        let mut grouped: BTreeMap<(&str, &PathBuf), Vec<&TestResult>> = BTreeMap::new();
        for result in results {
            grouped.entry((result.module_name.as_str(), &result.module_dir)).or_default().push(result);
        }
        for ((module, module_dir), tests) in grouped.iter() {
            let rel_dir = pathdiff::diff_paths(module_dir, &workspace.root)
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| "<unknown>".to_string());
            println!("\n{} ({})", module.bold(), rel_dir.dimmed());
            for result in tests {
                let (icon, status) = if result.exit_code == 0 {
                    ("✓".green(), "PASS".green())
                } else {
                    ("✗".red(), "FAIL".red())
                };
                let test = result.test_graph.file_name().unwrap().to_string_lossy().dimmed();
                println!("  {} {}  {}  ({:?})", icon, status, test, result.duration);
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

impl Command for TestCommand {
    fn matched_args<'a>(&self, _workspace: &Workspace, args: &'a ArgMatches) -> Option<&'a ArgMatches> {
        args.subcommand_matches("test")
    }

    fn run(&self, workspace: &mut Workspace, _command_name: Option<&str>, args: &ArgMatches) -> CommandResult {
        let modules_folder = args.get_one::<String>("modules_folder")
            .map(PathBuf::from)
            .unwrap_or_else(|| workspace.root.clone());
        let engine_dir = args.get_one::<String>("engine_dir").map(PathBuf::from);
        let timeout_secs = args.get_one::<u64>("timeout").copied().unwrap_or(30);
        let timeout = Duration::from_secs(timeout_secs);
        let tests = Self::collect_tests(&modules_folder);
        if tests.is_empty() {
            println!("{}", "No modules with tests found.".yellow());
            return Ok(());
        }
        println!("Found {} test(s) in {} module(s).", tests.len(), tests.iter().map(|t| &t.module_name).collect::<std::collections::HashSet<_>>().len());
        let mut results = Vec::new();
        for test in &tests {
            let test_graph_relpath = test.test_graph
                .strip_prefix(&modules_folder)
                .unwrap_or(&test.test_graph);
            println!("{} {} (module: {})", "Running test graph:".green(), test_graph_relpath.display(), test.module_name);
            let start = Instant::now();
            let exit_code = match Self::run_nodos_graph_test(workspace, engine_dir.as_deref(), &test.test_graph, timeout) {
                Ok(code) => code,
                Err(e) => {
                    eprintln!("{} {}: {}", "Error when running test graph".red(), test_graph_relpath.display(), e);
                    -1
                }
            };
            let duration = start.elapsed();
            results.push(TestResult {
                module_name: test.module_name.clone(),
                module_dir: test.module_dir.clone(),
                test_graph: test.test_graph.clone(),
                exit_code,
                duration,
            });
        }
        Self::print_summary(&results, workspace);
        Ok(())
    }

    fn needs_workspace(&self) -> bool {
        true
    }
} 