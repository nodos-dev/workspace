mod support;

use nosman::nosman::command::get::GetCommand;
use nosman::nosman::common::set_prompt_handler;
use std::fs;
use std::sync::{Arc, Mutex};

#[test]
fn get_preserves_modules_when_clean_modules_false() {
    let mut test = workspace!();
    let module_dir = test.workspace.root.join("Module").join("keep.module");
    fs::create_dir_all(&module_dir).expect("Failed to create module dir");
    let keep_file = module_dir.join("keep.txt");
    fs::write(&keep_file, "keep").expect("Failed to write module file");

    let res = GetCommand {}.run_get(
        &mut test.workspace,
        &"nodos".to_string(),
        &"1.4".to_string(),
        true,
        true,
        false,
    );
    assert!(res.is_ok(), "Failed to install nodos for module preservation test");
    assert!(module_dir.exists(), "Module dir should remain when clean_modules is false");
    assert!(keep_file.exists(), "Module file should remain when clean_modules is false");
}

#[test]
fn get_prompts_when_deletions_exist() {
    struct PromptGuard;
    impl Drop for PromptGuard {
        fn drop(&mut self) {
            set_prompt_handler(None);
        }
    }

    let mut test = workspace!();
    let stale_dir = test.workspace.root.join("Stale");
    fs::create_dir_all(&stale_dir).expect("Failed to create stale dir");
    fs::write(stale_dir.join("stale.txt"), "stale").expect("Failed to write stale file");

    let seen = Arc::new(Mutex::new(Vec::<String>::new()));
    let seen_clone = Arc::clone(&seen);
    set_prompt_handler(Some(Box::new(move |question, _default, _dont_ask| {
        seen_clone.lock().unwrap().push(question.to_string());
        false
    })));
    let _guard = PromptGuard;

    let res = GetCommand {}.run_get(
        &mut test.workspace,
        &"nodos".to_string(),
        &"1.4".to_string(),
        true,
        false,
        false,
    );
    assert!(res.is_err(), "Expected get to abort when prompt is declined");
    let seen = seen.lock().unwrap();
    assert!(
        seen.iter().any(|q| q.contains("will delete")),
        "Expected deletion prompt to be shown"
    );
}
