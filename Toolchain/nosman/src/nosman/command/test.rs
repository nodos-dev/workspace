use std::fs;
use std::path::{Path, PathBuf};
use clap::ArgMatches;
use colored::Colorize;
use crate::nosman::command::{Command, CommandResult, CommandError};
use crate::nosman::workspace::Workspace;
use crate::nosman::module;

pub struct TestCommand {}

struct TestCase {
    module_name: String,
    module_dir: PathBuf,
    test_graph: PathBuf,
}

struct TestResult {
    module_name: String,
    test_graph: PathBuf,
    exit_code: i32,
}

impl TestCommand {
    fn collect_tests(modules_folder: &PathBuf) -> Vec<TestCase> {
        let mut tests = Vec::new();
        let manifests = module::get_module_manifests(modules_folder, true);
        for (_module_type, manifest_path) in manifests {
            let module_dir = manifest_path.parent().unwrap().to_path_buf();
            let module_name = module_dir.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| module_dir.display().to_string());
            let tests_dir = module_dir.join("Tests");
            if !tests_dir.exists() || !tests_dir.is_dir() {
                continue;
            }
            let entries = match fs::read_dir(&tests_dir) {
                Ok(e) => e,
                Err(_) => continue,
            };
            for entry in entries {
                if let Ok(entry) = entry {
                    let path = entry.path();
                    if path.is_file() {
                        tests.push(TestCase {
                            module_name: module_name.clone(),
                            module_dir: module_dir.clone(),
                            test_graph: path,
                        });
                    }
                }
            }
        }
        tests
    }

    fn run_nodos_graph_test(workspace: &Workspace, engine_dir: Option<&Path>, graph_path: &Path) -> Result<i32, String> {
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
        let status = std::process::Command::new(&engine_path)
            .arg("--load-graph")
            .arg(graph_path)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map_err(|e| e.to_string())?;
        Ok(status.code().unwrap_or(-1))
    }

    fn print_summary(results: &[TestResult]) {
        println!("\nTest Summary:");
        println!("{:<30} | {:<40} | {:<10} | {}", "Module", "Test Graph", "ExitCode", "Result");
        println!("{}", "-".repeat(90));
        for result in results {
            let status = if result.exit_code == 0 { "PASS".green() } else { "FAIL".red() };
            println!("{:<30} | {:<40} | {:<10} | {}", result.module_name, result.test_graph.file_name().unwrap().to_string_lossy(), result.exit_code, status);
        }
        let passed = results.iter().filter(|r| r.exit_code == 0).count();
        let total = results.len();
        println!("\n{} of {} tests passed.", passed, total);
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
        let tests = Self::collect_tests(&modules_folder);
        if tests.is_empty() {
            println!("{}", "No modules with tests found.".yellow());
            return Ok(());
        }
        println!("Found {} test(s) in {} module(s).", tests.len(), tests.iter().map(|t| &t.module_name).collect::<std::collections::HashSet<_>>().len());
        let mut results = Vec::new();
        for test in &tests {
            println!("{} {} (module: {})", "Running test graph:".green(), test.test_graph.display(), test.module_name);
            let exit_code = match Self::run_nodos_graph_test(workspace, engine_dir.as_deref(), &test.test_graph) {
                Ok(code) => code,
                Err(e) => {
                    eprintln!("{} {}: {}", "Failed to run nosLauncher for".red(), test.test_graph.display(), e);
                    -1
                }
            };
            results.push(TestResult {
                module_name: test.module_name.clone(),
                test_graph: test.test_graph.clone(),
                exit_code,
            });
        }
        Self::print_summary(&results);
        Ok(())
    }

    fn needs_workspace(&self) -> bool {
        true
    }
} 