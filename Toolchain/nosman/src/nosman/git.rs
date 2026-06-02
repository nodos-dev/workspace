use std::path::Path;
use std::process::Command;

/// Run a git command in `repo`, returning its output. Returns `None` when git
/// could not be spawned at all (e.g. not installed).
fn run(repo: &Path, args: &[&str]) -> Option<std::process::Output> {
    Command::new("git").current_dir(repo).args(args).output().ok()
}

/// Whether `dir` is inside a git work tree.
pub fn is_inside_work_tree(dir: &Path) -> bool {
    match run(dir, &["rev-parse", "--is-inside-work-tree"]) {
        Some(o) => o.status.success() && String::from_utf8_lossy(&o.stdout).trim() == "true",
        None => false,
    }
}

/// Whether the repository is a shallow clone.
pub fn is_shallow(repo: &Path) -> bool {
    match run(repo, &["rev-parse", "--is-shallow-repository"]) {
        Some(o) => o.status.success() && String::from_utf8_lossy(&o.stdout).trim() == "true",
        None => false,
    }
}

/// Fetch tags from `origin`. When `unshallow` is set (the repo is a shallow
/// clone), also deepen to full history so commit ranges between tags resolve.
/// CI typically checks out with `--depth 1 --tags`, which lands tag refs but
/// grafts away the history `describe`/`log` need — so we unshallow here rather
/// than requiring the build workflow to change its checkout depth.
pub fn fetch_tags(repo: &Path, unshallow: bool) -> Result<(), String> {
    let mut args: Vec<&str> = vec!["fetch", "--quiet", "--tags"];
    if unshallow {
        args.push("--unshallow");
    }
    args.push("origin");
    let o = run(repo, &args).ok_or_else(|| "git not available".to_string())?;
    if o.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&o.stderr).trim().to_string())
    }
}

/// Most recent tag reachable from HEAD matching `match_glob`, or `None` when
/// there is no such tag (e.g. the first release).
pub fn describe_latest_tag(repo: &Path, match_glob: &str) -> Option<String> {
    let o = run(repo, &["describe", "--tags", "--abbrev=0", "--match", match_glob, "HEAD"])?;
    if !o.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&o.stdout).trim().to_string();
    if s.is_empty() { None } else { Some(s) }
}

/// Commit subjects in `range` (e.g. `tag..HEAD`) touching `pathspec`, formatted
/// as `- <subject> (<short-hash>)`. Merge commits are excluded.
pub fn log_subjects(repo: &Path, range: &str, pathspec: &Path) -> Vec<String> {
    let path_str = pathspec.to_string_lossy().to_string();
    let o = match run(repo, &["log", range, "--no-merges", "--pretty=format:- %s (%h)", "--", &path_str]) {
        Some(o) if o.status.success() => o,
        _ => return vec![],
    };
    String::from_utf8_lossy(&o.stdout)
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.to_string())
        .collect()
}

/// Whether a tag with this exact name exists.
pub fn tag_exists(repo: &Path, tag: &str) -> bool {
    match run(repo, &["rev-parse", "-q", "--verify", &format!("refs/tags/{}", tag)]) {
        Some(o) => o.status.success(),
        None => false,
    }
}

/// Create an annotated tag at HEAD.
pub fn create_annotated_tag(repo: &Path, tag: &str, message: &str) -> Result<(), String> {
    let o = run(repo, &["tag", "-a", tag, "-m", message]).ok_or_else(|| "git not available".to_string())?;
    if o.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&o.stderr).trim().to_string())
    }
}

/// Push a single tag to `origin`.
pub fn push_tag(repo: &Path, tag: &str) -> Result<(), String> {
    let o = run(repo, &["push", "origin", tag]).ok_or_else(|| "git not available".to_string())?;
    if o.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&o.stderr).trim().to_string())
    }
}

/// Delete a tag locally and, when `remote` is set, on `origin` as well.
/// Best-effort: failures are ignored (used for rollback cleanup).
pub fn delete_tag(repo: &Path, tag: &str, remote: bool) {
    let _ = run(repo, &["tag", "-d", tag]);
    if remote {
        let _ = run(repo, &["push", "origin", &format!(":refs/tags/{}", tag)]);
    }
}
