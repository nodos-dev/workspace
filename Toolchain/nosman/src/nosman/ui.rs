//! Everything nosman says to a person goes through here.
//!
//! Human-facing lines go to stderr. Stdout is left for commands whose result
//! is data: `info`, `sdk-info`, `engine list --json` and `list` are read by the
//! CMake toolchain, by the Unreal plugin build and by CI, and a stray
//! diagnostic in the middle of their JSON breaks those callers.
//!
//! Two shapes carry most of the output. A step line puts a right-aligned verb
//! in front of its subject, so a run reads down a column of verbs. A summary
//! line closes a phase with a count and how long it took.

use std::fmt::Display;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use colored::Colorize;
use indicatif::{MultiProgress, ProgressBar, ProgressDrawTarget, ProgressStyle};

/// Width of the verb column. Same as cargo's, so the two look at home together.
const VERB_WIDTH: usize = 12;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Verbosity {
    /// Only warnings and errors.
    Quiet,
    Normal,
    /// Adds the lines a person only wants when something is wrong.
    Verbose,
}

static VERBOSITY: AtomicU8 = AtomicU8::new(1);

pub fn set_verbosity(level: Verbosity) {
    VERBOSITY.store(level as u8, Ordering::Relaxed);
}

pub fn verbosity() -> Verbosity {
    match VERBOSITY.load(Ordering::Relaxed) {
        0 => Verbosity::Quiet,
        2 => Verbosity::Verbose,
        _ => Verbosity::Normal,
    }
}

pub fn is_quiet() -> bool {
    verbosity() == Verbosity::Quiet
}

pub fn is_verbose() -> bool {
    verbosity() == Verbosity::Verbose
}

/// `auto` leaves it to the `colored` crate, which honours `NO_COLOR`,
/// `CLICOLOR_FORCE` and whether stdout is a terminal.
pub fn set_color(choice: &str) {
    match choice {
        "always" => colored::control::set_override(true),
        "never" => colored::control::set_override(false),
        _ => colored::control::unset_override(),
    }
}

/// The progress bars currently on screen, if any. Lines printed while they are
/// drawing have to go through them, or the bar and the line overwrite each other.
static ACTIVE_PROGRESS: OnceLock<Mutex<Option<MultiProgress>>> = OnceLock::new();

fn progress_slot() -> &'static Mutex<Option<MultiProgress>> {
    ACTIVE_PROGRESS.get_or_init(|| Mutex::new(None))
}

/// Writes one line to stderr, around whatever progress bars are drawing.
///
/// Hidden bars swallow anything handed to them, which is what happens whenever
/// stderr is not a terminal, so fall back to a plain write in that case.
fn emit(line: String) {
    if let Ok(guard) = progress_slot().lock() {
        if let Some(multi) = guard.as_ref() {
            if !multi.is_hidden() && multi.println(&line).is_ok() {
                return;
            }
        }
    }
    eprintln!("{}", line);
}

fn emit_unless_quiet(line: String) {
    if !is_quiet() {
        emit(line);
    }
}

/// `  Installing nos.aja` — a thing being done, or just done.
pub fn step(verb: &str, subject: impl Display) {
    emit_unless_quiet(format!(
        "{:>width$} {}",
        verb.bold().green(),
        subject,
        width = VERB_WIDTH
    ));
}

/// A step that did not work out. The run may still carry on.
pub fn step_failed(verb: &str, subject: impl Display) {
    emit(format!(
        "{:>width$} {}",
        verb.bold().red(),
        subject,
        width = VERB_WIDTH
    ));
}

/// A step that did nothing: already present, skipped, unchanged.
pub fn step_skipped(verb: &str, subject: impl Display) {
    emit_unless_quiet(format!(
        "{:>width$} {}",
        verb.dimmed(),
        subject.to_string().dimmed(),
        width = VERB_WIDTH
    ));
}

/// `Installed 3 packages in 1.20s` — closes a phase.
pub fn summary(verb: &str, count: usize, noun: &str, started: Instant) {
    emit_unless_quiet(format!(
        "{} {} in {}",
        verb.bold(),
        plural(count, noun),
        format_duration(started.elapsed())
    ));
}

/// A phase summary whose subject is not a plain count.
pub fn summary_line(text: impl Display, started: Instant) {
    emit_unless_quiet(format!("{} in {}", text, format_duration(started.elapsed())));
}

pub fn added(name: &str, version: &str) {
    emit_unless_quiet(format!(" {} {}=={}", "+".green(), name, version.dimmed()));
}

pub fn removed(name: &str, version: &str) {
    emit_unless_quiet(format!(" {} {}=={}", "-".red(), name, version.dimmed()));
}

pub fn warn(msg: impl Display) {
    emit(format!("{} {}", "warning:".bold().yellow(), msg));
}

pub fn error(msg: impl Display) {
    emit(format!("{} {}", "error:".bold().red(), msg));
}

/// Only shown with `--verbose`.
pub fn detail(msg: impl Display) {
    if is_verbose() {
        emit(format!("{}", msg.to_string().dimmed()));
    }
}

pub fn blank() {
    emit_unless_quiet(String::new());
}

/// A line of output produced by something nosman ran, indented under its step.
pub fn nested(msg: impl Display) {
    emit_unless_quiet(format!("  {}", msg));
}

/// `1 package` / `3 packages` / `2 processes`.
pub fn plural(count: usize, noun: &str) -> String {
    let suffix = if count == 1 {
        ""
    } else if noun.ends_with('s') || noun.ends_with('x') || noun.ends_with("ch") || noun.ends_with("sh") {
        "es"
    } else {
        "s"
    };
    format!("{} {}{}", count, noun, suffix)
}

/// Short enough to sit at the end of a line: `412ms`, `1.20s`, `1m 03s`.
pub fn format_duration(elapsed: Duration) -> String {
    let secs = elapsed.as_secs_f64();
    if secs < 1.0 {
        format!("{}ms", elapsed.as_millis())
    } else if secs < 60.0 {
        format!("{:.2}s", secs)
    } else {
        format!("{}m {:02}s", elapsed.as_secs() / 60, elapsed.as_secs() % 60)
    }
}

fn draw_target() -> ProgressDrawTarget {
    if is_quiet() {
        ProgressDrawTarget::hidden()
    } else {
        ProgressDrawTarget::stderr()
    }
}

/// The group of bars drawn together. Lines printed while it is open are drawn
/// above the bars instead of through them, until [`finish_progress`].
///
/// Work that starts its own progress while some is already on screen joins it,
/// so that one [`finish_progress`] takes all of it down again.
pub fn progress_group() -> MultiProgress {
    let mut guard = match progress_slot().lock() {
        Ok(guard) => guard,
        Err(_) => return MultiProgress::with_draw_target(draw_target()),
    };
    guard
        .get_or_insert_with(|| MultiProgress::with_draw_target(draw_target()))
        .clone()
}

/// A spinner for work with no measurable total. Opens a group of its own, so
/// [`finish_progress`] ends it.
pub fn spinner(message: impl Into<String>) -> ProgressBar {
    let pb = progress_group().add(ProgressBar::new_spinner());
    pb.set_style(
        ProgressStyle::with_template("{spinner:.cyan} {msg}")
            .unwrap_or_else(|_| ProgressStyle::default_spinner()),
    );
    pb.set_message(message.into());
    pb.enable_steady_tick(Duration::from_millis(100));
    pb
}

/// Takes down whatever progress is on screen and sends printed lines straight
/// to stderr again.
pub fn finish_progress() {
    if let Ok(mut guard) = progress_slot().lock() {
        if let Some(multi) = guard.take() {
            multi.clear().ok();
        }
    }
}

/// A bar for one download. It starts as a running byte count, because the size
/// is only known once the server answers, and [`set_download_total`] turns it
/// into a real bar when that happens.
pub fn download_bar(multi: &MultiProgress, label: impl Into<String>) -> ProgressBar {
    let pb = multi.add(ProgressBar::new_spinner());
    pb.set_style(
        ProgressStyle::with_template("{prefix:>12.cyan} {spinner:.cyan} {bytes:>11} {binary_bytes_per_sec:>12}")
            .unwrap_or_else(|_| ProgressStyle::default_spinner()),
    );
    pb.set_prefix(label.into());
    pb.enable_steady_tick(Duration::from_millis(100));
    pb
}

/// Tells a download bar how far it has to go.
pub fn set_download_total(pb: &ProgressBar, total: u64) {
    pb.set_length(total);
    pb.set_style(
        ProgressStyle::with_template(
            "{prefix:>12.cyan} {bar:22.cyan/blue} {bytes:>11}/{total_bytes:<11} {binary_bytes_per_sec:>12}",
        )
        .unwrap_or_else(|_| ProgressStyle::default_bar())
        .progress_chars("=> "),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use indicatif::TermLike;
    use std::sync::{Arc, Mutex};

    /// A terminal that keeps everything written to it, so a test can look at
    /// what a bar actually drew.
    #[derive(Debug, Clone, Default)]
    struct FakeTerm {
        written: Arc<Mutex<String>>,
    }

    impl FakeTerm {
        fn contents(&self) -> String {
            self.written.lock().unwrap().clone()
        }
    }

    impl TermLike for FakeTerm {
        fn width(&self) -> u16 {
            100
        }
        fn move_cursor_up(&self, _n: usize) -> std::io::Result<()> {
            Ok(())
        }
        fn move_cursor_down(&self, _n: usize) -> std::io::Result<()> {
            Ok(())
        }
        fn move_cursor_right(&self, _n: usize) -> std::io::Result<()> {
            Ok(())
        }
        fn move_cursor_left(&self, _n: usize) -> std::io::Result<()> {
            Ok(())
        }
        fn write_line(&self, s: &str) -> std::io::Result<()> {
            self.written.lock().unwrap().push_str(s);
            self.written.lock().unwrap().push('\n');
            Ok(())
        }
        fn write_str(&self, s: &str) -> std::io::Result<()> {
            self.written.lock().unwrap().push_str(s);
            Ok(())
        }
        fn clear_line(&self) -> std::io::Result<()> {
            Ok(())
        }
        fn flush(&self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn durations_read_at_a_glance() {
        assert_eq!(format_duration(Duration::from_millis(412)), "412ms");
        assert_eq!(format_duration(Duration::from_millis(1200)), "1.20s");
        assert_eq!(format_duration(Duration::from_secs(63)), "1m 03s");
        assert_eq!(format_duration(Duration::from_secs(600)), "10m 00s");
    }

    #[test]
    fn nouns_are_pluralised() {
        assert_eq!(plural(1, "package"), "1 package");
        assert_eq!(plural(3, "package"), "3 packages");
        assert_eq!(plural(0, "package"), "0 packages");
        assert_eq!(plural(2, "process"), "2 processes");
        assert_eq!(plural(1, "process"), "1 process");
    }

    #[test]
    fn a_download_bar_grows_and_learns_its_total() {
        let term = FakeTerm::default();
        let multi = MultiProgress::with_draw_target(ProgressDrawTarget::term_like(Box::new(
            term.clone(),
        )));
        let bar = multi.add(ProgressBar::new_spinner());
        bar.set_prefix("nos.aja");

        set_download_total(&bar, 2048);
        assert_eq!(bar.length(), Some(2048));

        bar.set_position(1024);
        assert_eq!(bar.position(), 1024);

        // The bar has to actually reach the terminal, otherwise a download
        // looks like a hang.
        bar.tick();
        let drawn = term.contents();
        assert!(drawn.contains("nos.aja"), "bar did not draw its label: {drawn:?}");
    }

    #[test]
    fn printed_lines_go_around_the_bars() {
        let term = FakeTerm::default();
        let multi = MultiProgress::with_draw_target(ProgressDrawTarget::term_like(Box::new(
            term.clone(),
        )));
        multi.add(ProgressBar::new_spinner()).tick();
        multi.println("Installed 2 packages in 43ms").unwrap();

        assert!(
            term.contents().contains("Installed 2 packages in 43ms"),
            "a line printed while bars are drawing must still appear: {:?}",
            term.contents()
        );
    }

    #[test]
    fn quiet_hides_the_bars() {
        set_verbosity(Verbosity::Quiet);
        assert!(draw_target().is_hidden());
        set_verbosity(Verbosity::Normal);
        // Off a terminal the target is hidden anyway, so only the quiet case
        // can be asserted here.
    }
}
