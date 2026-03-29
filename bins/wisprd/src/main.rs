mod audio;
mod deepgram;
mod overlay;
mod portal;

use std::{future::pending, process::Command, sync::Arc};

use audio::{AudioCapture, resolve_selected_device};
use chrono::Utc;
use deepgram::{DeepgramSession, TranscriptEvent};
use tokio::sync::{Mutex, RwLock, oneshot};
use tokio::time::{self, Duration, Instant};
use tracing::{error, info, warn};
use wispr_core::{
    AppConfig, Result,
    models::{DaemonStatus, DictationState},
    secrets::SecretStore,
    typing::{UInputKeyboard, diff_patch},
};
use zbus::connection::Builder as ConnectionBuilder;

const DEEPGRAM_FRAME_BYTES: usize = 1_280;

struct AppState {
    config: RwLock<AppConfig>,
    status: RwLock<DaemonStatus>,
    hotkey_ready: RwLock<bool>,
    overlay: overlay::OverlayHandle,
    session: Mutex<Option<RunningSession>>,
}

struct RunningSession {
    stop_tx: oneshot::Sender<()>,
}

struct DictationDbus {
    state: Arc<AppState>,
}

#[zbus::interface(name = "io.wispr.Dictation1")]
impl DictationDbus {
    async fn toggle(&self) -> String {
        match toggle_dictation(self.state.clone()).await {
            Ok(message) => message,
            Err(error) => error.to_string(),
        }
    }

    async fn start(&self) -> String {
        match start_dictation(self.state.clone()).await {
            Ok(message) => message,
            Err(error) => error.to_string(),
        }
    }

    async fn stop(&self) -> String {
        match stop_dictation(self.state.clone()).await {
            Ok(message) => message,
            Err(error) => error.to_string(),
        }
    }

    async fn status(&self) -> String {
        let status = self.state.status.read().await.clone();
        serde_json::to_string(&status).unwrap_or_else(|_| "{}".to_string())
    }

    async fn open_settings(&self) -> String {
        let status = Command::new("wispr-settings").spawn();
        match status {
            Ok(_) => "Opened settings".to_string(),
            Err(error) => format!("Failed to open settings: {error}"),
        }
    }
}

#[tokio::main(flavor = "multi_thread")]
async fn main() -> Result<()> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    tracing_subscriber::fmt()
        .with_env_filter("wisprd=info,wispr_core=info")
        .init();

    let config = AppConfig::load()?;
    let overlay = overlay::OverlayHandle::spawn();

    let state = Arc::new(AppState {
        config: RwLock::new(config),
        status: RwLock::new(DaemonStatus::default()),
        hotkey_ready: RwLock::new(false),
        overlay,
        session: Mutex::new(None),
    });

    let connection = ConnectionBuilder::session()?
        .name(wispr_core::DICTATION_SERVICE)?
        .serve_at(
            wispr_core::DICTATION_OBJECT_PATH,
            DictationDbus {
                state: state.clone(),
            },
        )?
        .build()
        .await?;

    sync_status(state.clone(), None, None).await?;

    let hotkey = state.config.read().await.hotkey.clone();
    match portal::register_toggle_shortcut(&connection, &hotkey).await {
        Ok(mut events) => {
            *state.hotkey_ready.write().await = true;
            sync_status(state.clone(), None, None).await?;
            let state_for_hotkey = state.clone();
            tokio::spawn(async move {
                while events.recv().await.is_some() {
                    if let Err(error) = toggle_dictation(state_for_hotkey.clone()).await {
                        error!("hotkey toggle failed: {error}");
                    }
                }
            });
        }
        Err(error) => {
            warn!("Portal hotkey registration failed: {error}");
            sync_status(
                state.clone(),
                None,
                Some("Portal shortcut binding failed. Use a GNOME custom shortcut that runs `wisprctl toggle`.".to_string()),
            )
            .await?;
        }
    }

    info!("wisprd is running");
    futures_util::future::pending::<()>().await;
    Ok(())
}

async fn toggle_dictation(state: Arc<AppState>) -> Result<String> {
    if state.session.lock().await.is_some() {
        stop_dictation(state).await
    } else {
        start_dictation(state).await
    }
}

async fn start_dictation(state: Arc<AppState>) -> Result<String> {
    if state.session.lock().await.is_some() {
        return Ok("Dictation is already active".to_string());
    }

    let config = state.config.read().await.clone();
    let selected = resolve_selected_device(&config).ok_or_else(|| {
        wispr_core::WisprError::InvalidState(
            "No valid microphone is selected. Open wispr-settings and choose a microphone."
                .to_string(),
        )
    })?;

    let secret_store = SecretStore::connect().await?;
    let api_key = secret_store.get_api_key().await?.ok_or_else(|| {
        wispr_core::WisprError::InvalidState(
            "No Deepgram API key is stored yet. Open wispr-settings first.".to_string(),
        )
    })?;
    let mut keyboard = UInputKeyboard::open().map_err(|error| {
        wispr_core::WisprError::InvalidState(format!(
            "Typing engine failed to open /dev/uinput: {error}"
        ))
    })?;

    let capture = AudioCapture::start(&selected)?;
    let audio_rx = capture.receiver();
    let mut deepgram = DeepgramSession::connect(&api_key).await?;
    let audio_tx = deepgram.audio_sender();
    let (stop_tx, mut stop_rx) = oneshot::channel::<()>();

    publish_status(
        &state,
        Some(DictationState::Listening),
        Some(selected.clone()),
        Some(String::new()),
        None,
    )
    .await;

    let state_for_task = state.clone();
    tokio::spawn(async move {
        let mut committed_text = String::new();
        let mut active_turn = String::new();
        let mut pending_audio = Vec::new();
        let mut logged_audio = false;
        let mut shutting_down = false;
        let mut shutdown_deadline = None::<Instant>;

        loop {
            tokio::select! {
                _ = &mut stop_rx, if !shutting_down => {
                    if !pending_audio.is_empty() {
                        let _ = audio_tx.send(pending_audio.clone()).await;
                        pending_audio.clear();
                    }
                    let _ = capture.stop();
                    deepgram.close_stream();
                    shutting_down = true;
                    shutdown_deadline = Some(Instant::now() + Duration::from_secs(2));
                }
                maybe_audio = audio_rx.recv(), if !shutting_down => {
                    match maybe_audio {
                        Ok(frame) => {
                            if !logged_audio {
                                info!("audio frame received: {} bytes", frame.len());
                                logged_audio = true;
                            }

                            pending_audio.extend_from_slice(&frame);
                            while pending_audio.len() >= DEEPGRAM_FRAME_BYTES {
                                let chunk = pending_audio.drain(..DEEPGRAM_FRAME_BYTES).collect::<Vec<u8>>();
                                if audio_tx.send(chunk).await.is_err() {
                                    publish_status(
                                        &state_for_task,
                                        Some(DictationState::Error),
                                        None,
                                        None,
                                        Some("Failed to forward audio to Deepgram".to_string()),
                                    )
                                    .await;
                                    break;
                                }
                            }
                        }
                        Err(_) => {
                            publish_status(
                                &state_for_task,
                                Some(DictationState::Error),
                                None,
                                None,
                                Some("Audio stream ended unexpectedly".to_string()),
                            )
                            .await;
                            let mut guard = state_for_task.session.lock().await;
                            *guard = None;
                            break;
                        }
                    }
                }
                maybe_event = deepgram.next_event() => {
                    let Some(event) = maybe_event else {
                        publish_status(&state_for_task, Some(DictationState::Idle), None, None, None).await;
                        let mut guard = state_for_task.session.lock().await;
                        *guard = None;
                        break;
                    };

                    match event {
                        TranscriptEvent::Partial(text) => {
                            info!("deepgram partial: {}", text);
                            let previous_rendered = render_transcript(&committed_text, &active_turn);
                            let next_rendered = render_transcript(&committed_text, &text);
                            apply_transcript(
                                &state_for_task,
                                &mut keyboard,
                                &previous_rendered,
                                &next_rendered,
                            )
                            .await;
                            active_turn = text;
                        }
                        TranscriptEvent::Final(text) => {
                            info!("deepgram final: {}", text);
                            let previous_rendered = render_transcript(&committed_text, &active_turn);
                            let next_rendered = render_transcript(&committed_text, &text);
                            apply_transcript(
                                &state_for_task,
                                &mut keyboard,
                                &previous_rendered,
                                &next_rendered,
                            )
                            .await;
                            committed_text = next_rendered;
                            active_turn.clear();
                        }
                        TranscriptEvent::TurnEnded => {
                            info!("deepgram turn ended");
                            committed_text = append_turn(&committed_text, &active_turn);
                            active_turn.clear();
                        }
                        TranscriptEvent::TurnEndedWithTranscript(text) => {
                            info!("deepgram turn ended with transcript: {}", text);
                            let previous_rendered = render_transcript(&committed_text, &active_turn);
                            let next_rendered = render_transcript(&committed_text, &text);
                            apply_transcript(
                                &state_for_task,
                                &mut keyboard,
                                &previous_rendered,
                                &next_rendered,
                            )
                            .await;
                            committed_text = append_turn(&committed_text, &text);
                            active_turn.clear();
                        }
                        TranscriptEvent::Warning(message) => {
                            warn!("deepgram warning: {}", message);
                            publish_status(
                                &state_for_task,
                                Some(DictationState::Error),
                                None,
                                None,
                                Some(message),
                            )
                            .await;
                        }
                    }
                }
                _ = async {
                    if let Some(deadline) = shutdown_deadline {
                        time::sleep_until(deadline).await;
                    } else {
                        pending::<()>().await;
                    }
                }, if shutting_down => {
                    info!("deepgram shutdown grace period elapsed");
                    publish_status(&state_for_task, Some(DictationState::Idle), None, None, None).await;
                    break;
                }
            }
        }
    });

    let mut guard = state.session.lock().await;
    *guard = Some(RunningSession { stop_tx });
    Ok("Started dictation".to_string())
}

async fn apply_transcript(
    state: &Arc<AppState>,
    keyboard: &mut UInputKeyboard,
    previous: &str,
    latest: &str,
) {
    let patch = diff_patch(previous, latest);
    let _ = keyboard.emit_patch(&patch);

    let mut status = state.status.write().await;
    status.partial_transcript = Some(latest.to_string());
    status.updated_at = Utc::now();
    state.overlay.push(status.clone());
}

fn render_transcript(committed: &str, active_turn: &str) -> String {
    append_turn(committed, active_turn)
}

fn append_turn(committed: &str, turn: &str) -> String {
    let turn = turn.trim();
    if turn.is_empty() {
        return committed.to_string();
    }
    if committed.is_empty() {
        return turn.to_string();
    }
    if needs_separator(committed, turn) {
        format!("{committed} {turn}")
    } else {
        format!("{committed}{turn}")
    }
}

fn needs_separator(left: &str, right: &str) -> bool {
    let left_last = left.chars().last();
    let right_first = right.chars().next();

    match (left_last, right_first) {
        (Some(left), Some(right)) => {
            !left.is_whitespace()
                && !right.is_whitespace()
                && !matches!(right, '.' | ',' | '!' | '?' | ':' | ';' | ')' | ']' | '}')
                && !matches!(left, '(' | '[' | '{' | '/' | '-' | '\n')
        }
        _ => false,
    }
}

async fn stop_dictation(state: Arc<AppState>) -> Result<String> {
    let session = {
        let mut guard = state.session.lock().await;
        guard.take()
    };
    let Some(session) = session else {
        return Ok("Dictation is not active".to_string());
    };

    let _ = session.stop_tx.send(());
    publish_status(&state, Some(DictationState::Idle), None, None, None).await;
    Ok("Stopped dictation".to_string())
}

async fn build_status(
    state: &Arc<AppState>,
    state_override: Option<DictationState>,
    error: Option<String>,
) -> DaemonStatus {
    let config = state.config.read().await.clone();
    DaemonStatus {
        state: state_override.unwrap_or(DictationState::Idle),
        mic_ready: resolve_selected_device(&config).is_some(),
        typing_ready: std::fs::OpenOptions::new()
            .write(true)
            .open("/dev/uinput")
            .is_ok(),
        hotkey_ready: *state.hotkey_ready.read().await,
        current_mic: config.selected_device.clone(),
        partial_transcript: None,
        last_error: error,
        updated_at: Utc::now(),
    }
}

async fn sync_status(
    state: Arc<AppState>,
    state_override: Option<DictationState>,
    error: Option<String>,
) -> Result<()> {
    let mut status = build_status(&state, state_override, error).await;
    if let Some(existing) = state.status.read().await.partial_transcript.clone() {
        status.partial_transcript = Some(existing);
    }
    state.overlay.push(status.clone());
    *state.status.write().await = status;
    Ok(())
}

async fn publish_status(
    state: &Arc<AppState>,
    state_override: Option<DictationState>,
    current_mic: Option<wispr_core::DeviceChoice>,
    partial_transcript: Option<String>,
    error: Option<String>,
) {
    let mut status = state.status.write().await.clone();
    let is_idle = matches!(state_override.as_ref(), Some(DictationState::Idle));

    if let Some(next_state) = state_override {
        status.state = next_state;
    }
    if let Some(mic) = current_mic {
        status.current_mic = Some(mic);
    }
    if let Some(partial) = partial_transcript {
        status.partial_transcript = Some(partial);
    } else if is_idle {
        status.partial_transcript = None;
    }
    status.last_error = error;
    status.updated_at = Utc::now();

    state.overlay.push(status.clone());
    *state.status.write().await = status;
}
