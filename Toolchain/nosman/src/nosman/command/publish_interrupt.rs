//! Clearing up after a publish someone interrupts.
//!
//! The store client removes the draft it opened whenever a publish returns or
//! unwinds. Ctrl-C does neither: the process ends where it stands, and the
//! draft it staged stays in the store until the next publish of the same
//! release sweeps it. Catching the signal here takes that draft away straight
//! away instead.
//!
//! Only a real publish is watched. A dry run stages nothing, so it leaves the
//! signal alone.

use std::sync::{mpsc, Mutex, MutexGuard, OnceLock};
use std::time::Duration;

use nodos_store_client::StoreClient;

use crate::nosman::ui;

/// What a shell reports for a program Ctrl-C stopped.
const INTERRUPTED_EXIT_CODE: i32 = 130;

/// Cleanup gets a brief chance to finish, but Ctrl-C must remain prompt even
/// when the store is unavailable.
const CLEANUP_TIMEOUT: Duration = Duration::from_secs(2);

/// The release a publish is in the middle of.
///
/// All three parts decide where the store puts the artifact, so all three are
/// needed to name the draft to take away and nothing else.
#[derive(Clone, Debug, PartialEq)]
pub struct PublishInProgress {
    pub session_id: i64,
    pub name: String,
    pub version: String,
    pub target_platform: String,
}

fn in_progress() -> &'static Mutex<Option<PublishInProgress>> {
    static SLOT: OnceLock<Mutex<Option<PublishInProgress>>> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(None))
}

/// The slot, or nothing at all when it can no longer be reached. Losing it
/// costs the cleanup; it must never cost the publish, so nothing here fails
/// loudly.
fn slot() -> Option<MutexGuard<'static, Option<PublishInProgress>>> {
    in_progress().lock().ok()
}

static HANDLER: OnceLock<()> = OnceLock::new();

/// Whether Ctrl-C is being listened for. Only [`watch`] arranges that, and once
/// arranged it stays for the rest of the run.
pub fn watching_for_interrupts() -> bool {
    HANDLER.get().is_some()
}

/// The release being published right now, if one is.
pub fn publish_in_progress() -> Option<PublishInProgress> {
    slot().and_then(|slot| slot.clone())
}

/// Takes Ctrl-C over for as long as the returned value lives, so that an
/// interrupted publish takes away the draft it staged instead of leaving it.
///
/// Call it around a real publish only.
pub fn watch(session_id: i64, name: &str, version: &str, target_platform: &str) -> Watch {
    if let Some(mut slot) = slot() {
        *slot = Some(PublishInProgress {
            session_id,
            name: name.to_string(),
            version: version.to_string(),
            target_platform: target_platform.to_string(),
        });
    }
    HANDLER.get_or_init(|| {
        if let Err(e) = ctrlc::set_handler(on_interrupt) {
            // Without the handler an interrupted publish leaves its draft
            // behind, which is what happened before this existed, and the next
            // publish of the same release still clears it. Nothing the
            // publisher can act on, so it stays out of the way.
            ui::detail(format!("cannot listen for Ctrl-C: {}", e));
        }
    });
    Watch
}

/// Keeps an interrupt from acting on a publish that has already ended.
pub struct Watch;

impl Drop for Watch {
    fn drop(&mut self) {
        if let Some(mut slot) = slot() {
            *slot = None;
        }
    }
}

/// Takes away the exact draft this publish created and returns the line to show
/// for what happened.
///
/// It cannot fail. An interrupt has to leave the publisher no worse off than no
/// handler at all, so a cleanup that did not work is said out loud and the run
/// ends either way.
pub fn clear_draft(client: &StoreClient, publishing: &PublishInProgress) -> String {
    match client.discard_publish_session(publishing.session_id) {
        Ok(()) => format!(
            "publish of {} and removed its unfinished upload from the store",
            release(publishing)
        ),
        Err(e) => left_behind(publishing, e),
    }
}

/// `nos.aja==1.2.0 for x86_64-windows`.
fn release(publishing: &PublishInProgress) -> String {
    format!(
        "{}=={} for {}",
        publishing.name, publishing.version, publishing.target_platform
    )
}

/// The line for an upload still sitting in the store. It says the thing the
/// publisher needs: it is not stuck, the next publish of the same release takes
/// it away.
fn left_behind(publishing: &PublishInProgress, reason: nodos_store_client::Error) -> String {
    left_behind_because(publishing, &one_line(&reason))
}

fn left_behind_because(publishing: &PublishInProgress, reason: &str) -> String {
    format!(
        "publish of {}. Its unfinished upload could not be removed ({}), so the next publish of it will clear it",
        release(publishing),
        reason
    )
}

/// A store error folded onto one line.
///
/// A connection failure chains every cause onto its own `"caused by: ..."`
/// line, and this whole message lands inside one step line further down, so a
/// multi-line reason would print as broken lines instead of one sentence.
fn one_line(reason: &nodos_store_client::Error) -> String {
    reason
        .to_string()
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Runs on a thread of its own when the publisher presses Ctrl-C.
///
/// Nothing in here gives up loudly. A failed lock, a client that will not
/// build, a store that will not answer: each is reported and then let go,
/// because the run is ending anyway and an interrupt must not turn into a
/// crash.
fn on_interrupt() {
    let publishing = slot().and_then(|mut slot| slot.take());
    // Whatever was spinning on screen goes first, or the line lands in it.
    ui::finish_progress();
    if let Some(publishing) = publishing {
        let timeout_message = left_behind_because(
            &publishing,
            "the store did not answer before interruption finished",
        );
        let (sender, receiver) = mpsc::sync_channel(1);
        std::thread::spawn(move || {
            let line = match store_client() {
                Ok(client) => clear_draft(&client, &publishing),
                Err(e) => left_behind(&publishing, e),
            };
            let _ = sender.send(line);
        });
        let line = match receiver.recv_timeout(CLEANUP_TIMEOUT) {
            Ok(line) => line,
            Err(_) => timeout_message,
        };
        ui::step_failed("Interrupted", line);
    }
    std::process::exit(INTERRUPTED_EXIT_CODE);
}

/// A client of this thread's own, built the way the workspace builds the one
/// the publish is using. The in-flight one belongs to another thread and cannot
/// be borrowed from here.
fn store_client() -> nodos_store_client::Result<StoreClient> {
    StoreClient::builder()
        .with_token_store(nodos_store_client::TokenStore::new(
            std::path::PathBuf::from("nosman"),
        ))
        .with_auth(nodos_store_client::Auth::from_env().unwrap_or(nodos_store_client::Auth::None))
        .build()
}
