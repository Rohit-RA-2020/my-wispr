mod app_context;
mod audio;
mod deepgram;
mod overlay;
mod portal;

use std::{collections::HashSet, future::pending, process::Command, sync::Arc};

use audio::{AudioCapture, resolve_selected_device};
use chrono::Utc;
use deepgram::{DeepgramSession, TranscriptChunk, TranscriptEvent};
use tokio::sync::{Mutex, RwLock, oneshot};
use tokio::time::{self, Duration, Instant};
use tracing::{error, info, warn};
use wispr_core::{
    AppConfig, CorrectionScope, DecisionKind, FormattingTriggerPolicy, LlmInterpreter,
    PreferredListStyle, Result, RewriteScope, SegmentDecisionRequest, WisprError,
    models::{ActiveAppContext, DaemonStatus, DeviceChoice, DictationState, FormatKind},
    resolve_actions,
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

#[derive(Clone, Debug)]
struct FormattingBlock {
    raw_text: String,
    rendered_text: String,
    format_kind: FormatKind,
}

#[derive(Default)]
struct StatusUpdate {
    state: Option<DictationState>,
    current_mic: Option<Option<DeviceChoice>>,
    partial_transcript: Option<Option<String>>,
    last_error: Option<Option<String>>,
    intelligence_ready: Option<bool>,
    llm_ready: Option<bool>,
    last_llm_error: Option<Option<String>>,
    last_decision_kind: Option<Option<DecisionKind>>,
    intelligence_state: Option<Option<String>>,
    active_app: Option<Option<ActiveAppContext>>,
    last_resolution: Option<Option<String>>,
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
        WisprError::InvalidState(
            "No valid microphone is selected. Open wispr-settings and choose a microphone."
                .to_string(),
        )
    })?;

    let secret_store = SecretStore::connect().await?;
    let api_key = secret_store.get_api_key().await?.ok_or_else(|| {
        WisprError::InvalidState(
            "No Deepgram API key is stored yet. Open wispr-settings first.".to_string(),
        )
    })?;
    let mut keyboard = wispr_core::typing::UInputKeyboard::open().map_err(|error| {
        WisprError::InvalidState(format!("Typing engine failed to open /dev/uinput: {error}"))
    })?;

    let llm_state = load_llm_interpreter(&config, &secret_store).await?;
    let llm_ready = llm_state.is_some();
    let intelligence_ready = !config.intelligence.enabled || llm_ready;
    let llm_setup_error = if config.intelligence.enabled && !llm_ready {
        Some("Intelligence is enabled but the LLM backend is not configured. Falling back to literal dictation.".to_string())
    } else {
        None
    };

    let capture = AudioCapture::start(&selected)?;
    let audio_rx = capture.receiver();
    let mut deepgram = DeepgramSession::connect(&api_key).await?;
    let audio_tx = deepgram.audio_sender();
    let (stop_tx, mut stop_rx) = oneshot::channel::<()>();

    publish_status(
        &state,
        StatusUpdate {
            state: Some(DictationState::Listening),
            current_mic: Some(Some(selected.clone())),
            partial_transcript: Some(Some(String::new())),
            intelligence_ready: Some(intelligence_ready),
            llm_ready: Some(llm_ready),
            last_error: Some(None),
            last_llm_error: Some(llm_setup_error.clone()),
            last_decision_kind: Some(None),
            intelligence_state: Some(Some(if config.intelligence.enabled {
                if llm_ready {
                    "Interpreter ready".to_string()
                } else {
                    "Literal fallback".to_string()
                }
            } else {
                "Intelligence disabled".to_string()
            })),
            active_app: Some(None),
            last_resolution: Some(None),
        },
    )
    .await;

    let state_for_task = state.clone();
    tokio::spawn(async move {
        let mut committed_prefix = String::new();
        let mut active_block = None::<FormattingBlock>;
        let mut active_turn = String::new();
        let mut pending_audio = Vec::new();
        let mut logged_audio = false;
        let mut shutting_down = false;
        let mut shutdown_deadline = None::<Instant>;
        let mut processed_finals = HashSet::<String>::new();
        let llm_interpreter = llm_state;

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
                                        StatusUpdate {
                                            state: Some(DictationState::Error),
                                            last_error: Some(Some("Failed to forward audio to Deepgram".to_string())),
                                            ..StatusUpdate::default()
                                        },
                                    )
                                    .await;
                                    break;
                                }
                            }
                        }
                        Err(_) => {
                            publish_status(
                                &state_for_task,
                                StatusUpdate {
                                    state: Some(DictationState::Error),
                                    last_error: Some(Some("Audio stream ended unexpectedly".to_string())),
                                    ..StatusUpdate::default()
                                },
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
                        publish_status(
                            &state_for_task,
                            StatusUpdate {
                                state: Some(DictationState::Idle),
                                partial_transcript: Some(None),
                                intelligence_state: Some(None),
                                active_app: Some(None),
                                last_resolution: Some(None),
                                ..StatusUpdate::default()
                            },
                        )
                        .await;
                        let mut guard = state_for_task.session.lock().await;
                        *guard = None;
                        break;
                    };

                    match event {
                        TranscriptEvent::Partial(chunk) => {
                            info!("deepgram partial: {}", chunk.text);
                            let previous_rendered =
                                render_transcript(&committed_prefix, active_block.as_ref(), &active_turn);
                            let next_rendered =
                                render_transcript(&committed_prefix, active_block.as_ref(), &chunk.text);
                            apply_transcript(
                                &state_for_task,
                                &mut keyboard,
                                &previous_rendered,
                                &next_rendered,
                            )
                            .await;
                            active_turn = chunk.text;
                        }
                        TranscriptEvent::Final(chunk) => {
                            if !processed_finals.insert(chunk.dedupe_key()) {
                                continue;
                            }

                            info!("deepgram final: {}", chunk.text);
                            process_finalized_segment(
                                &state_for_task,
                                &config,
                                &mut keyboard,
                                &llm_interpreter,
                                &mut committed_prefix,
                                &mut active_block,
                                &mut active_turn,
                                chunk,
                            )
                            .await;
                        }
                        TranscriptEvent::TurnEnded => {
                            info!("deepgram turn ended");
                            finalize_unresolved_turn(
                                &state_for_task,
                                &mut keyboard,
                                &mut committed_prefix,
                                &mut active_block,
                                &mut active_turn,
                            )
                            .await;
                        }
                        TranscriptEvent::TurnEndedWithTranscript(chunk) => {
                            if processed_finals.insert(chunk.dedupe_key()) {
                                info!("deepgram turn ended with transcript: {}", chunk.text);
                                process_finalized_segment(
                                    &state_for_task,
                                    &config,
                                    &mut keyboard,
                                    &llm_interpreter,
                                    &mut committed_prefix,
                                    &mut active_block,
                                    &mut active_turn,
                                    chunk,
                                )
                                .await;
                            } else {
                                finalize_unresolved_turn(
                                    &state_for_task,
                                    &mut keyboard,
                                    &mut committed_prefix,
                                    &mut active_block,
                                    &mut active_turn,
                                )
                                .await;
                            }
                        }
                        TranscriptEvent::Warning(message) => {
                            warn!("deepgram warning: {}", message);
                            publish_status(
                                &state_for_task,
                                StatusUpdate {
                                    state: Some(DictationState::Error),
                                    last_error: Some(Some(message)),
                                    ..StatusUpdate::default()
                                },
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
                    publish_status(
                        &state_for_task,
                        StatusUpdate {
                            state: Some(DictationState::Idle),
                            partial_transcript: Some(None),
                            intelligence_state: Some(None),
                            active_app: Some(None),
                            last_resolution: Some(None),
                            ..StatusUpdate::default()
                        },
                    )
                    .await;
                    break;
                }
            }
        }
    });

    let mut guard = state.session.lock().await;
    *guard = Some(RunningSession { stop_tx });
    Ok("Started dictation".to_string())
}

async fn process_finalized_segment(
    state: &Arc<AppState>,
    config: &AppConfig,
    keyboard: &mut UInputKeyboard,
    llm_interpreter: &Option<LlmInterpreter>,
    committed_prefix: &mut String,
    active_block: &mut Option<FormattingBlock>,
    active_turn: &mut String,
    chunk: TranscriptChunk,
) {
    let previous_rendered = render_transcript(committed_prefix, active_block.as_ref(), active_turn);

    if let Some(interpreter) = llm_interpreter {
        publish_status(
            state,
            StatusUpdate {
                intelligence_state: Some(Some("Resolving segment".to_string())),
                last_llm_error: Some(None),
                ..StatusUpdate::default()
            },
        )
        .await;

        let request = SegmentDecisionRequest {
            segment_id: uuid::Uuid::new_v4().to_string(),
            finalized_text: chunk.text.clone(),
            literal_text: chunk.text.clone(),
            recent_text: recent_text_window(committed_prefix, config.intelligence.max_recent_chars),
            active_block_raw: active_block
                .as_ref()
                .map(|block| block.raw_text.clone())
                .unwrap_or_default(),
            active_block_rendered: active_block
                .as_ref()
                .map(|block| block.rendered_text.clone())
                .unwrap_or_default(),
            action_scope: config.intelligence.action_scope.clone(),
            command_mode: config.intelligence.command_mode.clone(),
            text_output_mode: config.intelligence.text_output_mode.clone(),
            preferred_list_style: PreferredListStyle::Numbered,
            formatting_trigger_policy: FormattingTriggerPolicy::ClearStructureOnly,
            correction_scope: CorrectionScope::CurrentBlockOnly,
            active_app: app_context::detect_active_app().await,
        };

        publish_status(
            state,
            StatusUpdate {
                active_app: Some(request.active_app.clone()),
                last_resolution: Some(None),
                ..StatusUpdate::default()
            },
        )
        .await;

        match interpreter.decide(&request).await {
            Ok(output) => {
                let resolved_actions = match resolve_actions(
                    &config.intelligence,
                    &output.decision.actions,
                    request.active_app.as_ref(),
                ) {
                    Ok(resolved) => resolved,
                    Err(error) => {
                        warn!("action resolution failed: {error}");
                        publish_status(
                            state,
                            StatusUpdate {
                                last_llm_error: Some(Some(error.to_string())),
                                last_decision_kind: Some(None),
                                intelligence_state: Some(Some("Literal fallback".to_string())),
                                last_resolution: Some(None),
                                ..StatusUpdate::default()
                            },
                        )
                        .await;
                        let next_rendered = render_literal_fallback(
                            committed_prefix,
                            active_block.as_ref(),
                            &chunk.text,
                        );
                        apply_transcript(state, keyboard, &previous_rendered, &next_rendered).await;
                        *committed_prefix = next_rendered;
                        *active_block = None;
                        active_turn.clear();
                        return;
                    }
                };

                let next_rendered = match output.decision.rewrite_scope {
                    RewriteScope::CurrentBlock => render_transcript(
                        committed_prefix,
                        Some(&FormattingBlock {
                            raw_text: next_block_raw(active_block.as_ref(), &chunk.text),
                            rendered_text: output.decision.text_to_emit.clone(),
                            format_kind: output.decision.format_kind.clone(),
                        }),
                        "",
                    ),
                    RewriteScope::Segment => render_with_segment_decision(
                        committed_prefix,
                        active_block.as_ref(),
                        &output.decision.text_to_emit,
                        &output.decision.format_kind,
                    ),
                };
                apply_transcript(state, keyboard, &previous_rendered, &next_rendered).await;
                let closes_block =
                    !resolved_actions.actions.is_empty() || !output.decision.keep_block_open;
                if let Err(error) = keyboard.emit_actions(&resolved_actions.actions) {
                    warn!("failed to emit intelligent action(s): {error}");
                    publish_status(
                        state,
                        StatusUpdate {
                            last_llm_error: Some(Some(format!("Action execution failed: {error}"))),
                            intelligence_state: Some(Some("Action execution failed".to_string())),
                            last_resolution: Some(None),
                            ..StatusUpdate::default()
                        },
                    )
                    .await;
                } else {
                    let intelligence_state =
                        if output.decision.rewrite_scope == RewriteScope::CurrentBlock {
                            Some("Formatting current block".to_string())
                        } else {
                            Some(format!("Decision: {}", output.decision.kind.as_label()))
                        };
                    publish_status(
                        state,
                        StatusUpdate {
                            last_llm_error: Some(None),
                            last_decision_kind: Some(Some(output.decision.kind.clone())),
                            intelligence_state: Some(intelligence_state),
                            last_resolution: Some(
                                resolved_actions.description.map(|description| description),
                            ),
                            ..StatusUpdate::default()
                        },
                    )
                    .await;
                }

                match output.decision.rewrite_scope {
                    RewriteScope::CurrentBlock => {
                        let next_raw = next_block_raw(active_block.as_ref(), &chunk.text);
                        if closes_block {
                            *committed_prefix = render_committed(
                                committed_prefix,
                                Some(&FormattingBlock {
                                    raw_text: next_raw,
                                    rendered_text: output.decision.text_to_emit,
                                    format_kind: output.decision.format_kind,
                                }),
                            );
                            *active_block = None;
                        } else {
                            *active_block = Some(FormattingBlock {
                                raw_text: next_raw,
                                rendered_text: output.decision.text_to_emit,
                                format_kind: output.decision.format_kind,
                            });
                        }
                    }
                    RewriteScope::Segment => {
                        *committed_prefix = render_with_segment_decision(
                            committed_prefix,
                            active_block.as_ref(),
                            &output.decision.text_to_emit,
                            &output.decision.format_kind,
                        );
                        *active_block = None;
                    }
                }
                active_turn.clear();
                return;
            }
            Err(error) => {
                warn!("llm interpretation failed: {error}");
                publish_status(
                    state,
                    StatusUpdate {
                        last_llm_error: Some(Some(error.to_string())),
                        last_decision_kind: Some(None),
                        intelligence_state: Some(Some("Literal fallback".to_string())),
                        ..StatusUpdate::default()
                    },
                )
                .await;
            }
        }
    }

    let next_rendered =
        render_literal_fallback(committed_prefix, active_block.as_ref(), &chunk.text);
    apply_transcript(state, keyboard, &previous_rendered, &next_rendered).await;
    *committed_prefix = next_rendered;
    *active_block = None;
    active_turn.clear();
}

async fn load_llm_interpreter(
    config: &AppConfig,
    secret_store: &SecretStore,
) -> Result<Option<LlmInterpreter>> {
    if !config.intelligence.enabled {
        return Ok(None);
    }

    if config.intelligence.base_url.trim().is_empty() || config.intelligence.model.trim().is_empty()
    {
        return Ok(None);
    }

    let Some(api_key) = secret_store.get_llm_api_key().await? else {
        return Ok(None);
    };

    Ok(Some(LlmInterpreter::new(
        config.intelligence.clone(),
        api_key,
    )?))
}

async fn apply_transcript(
    state: &Arc<AppState>,
    keyboard: &mut UInputKeyboard,
    previous: &str,
    latest: &str,
) {
    let patch = diff_patch(previous, latest);
    if let Err(error) = keyboard.emit_patch(&patch) {
        warn!("failed to emit transcript patch: {error}");
        let mut status = state.status.write().await.clone();
        status.last_error = Some(format!("Text injection failed: {error}"));
        status.updated_at = Utc::now();
        state.overlay.push(status.clone());
        *state.status.write().await = status;
        return;
    }

    let mut status = state.status.write().await;
    status.partial_transcript = Some(latest.to_string());
    status.updated_at = Utc::now();
    state.overlay.push(status.clone());
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
    publish_status(
        &state,
        StatusUpdate {
            state: Some(DictationState::Idle),
            partial_transcript: Some(None),
            intelligence_state: Some(None),
            active_app: Some(None),
            last_resolution: Some(None),
            ..StatusUpdate::default()
        },
    )
    .await;
    Ok("Stopped dictation".to_string())
}

async fn build_status(
    state: &Arc<AppState>,
    state_override: Option<DictationState>,
    error: Option<String>,
) -> DaemonStatus {
    let config = state.config.read().await.clone();
    let llm_ready = match SecretStore::connect().await {
        Ok(store) if config.intelligence.enabled => store
            .get_llm_api_key()
            .await
            .ok()
            .flatten()
            .map(|key| !key.trim().is_empty())
            .unwrap_or(false),
        _ => false,
    };

    DaemonStatus {
        state: state_override.unwrap_or(DictationState::Idle),
        mic_ready: resolve_selected_device(&config).is_some(),
        typing_ready: std::fs::OpenOptions::new()
            .write(true)
            .open("/dev/uinput")
            .is_ok(),
        hotkey_ready: *state.hotkey_ready.read().await,
        intelligence_ready: !config.intelligence.enabled || llm_ready,
        llm_ready,
        current_mic: config.selected_device.clone(),
        partial_transcript: None,
        last_error: error,
        last_llm_error: None,
        last_decision_kind: None,
        intelligence_state: if config.intelligence.enabled {
            Some(if llm_ready {
                "Interpreter ready".to_string()
            } else {
                "Literal fallback".to_string()
            })
        } else {
            Some("Intelligence disabled".to_string())
        },
        active_app: None,
        last_resolution: None,
        updated_at: Utc::now(),
    }
}

async fn sync_status(
    state: Arc<AppState>,
    state_override: Option<DictationState>,
    error: Option<String>,
) -> Result<()> {
    let mut status = build_status(&state, state_override, error).await;
    let existing = state.status.read().await.clone();
    status.partial_transcript = existing.partial_transcript;
    status.last_llm_error = existing.last_llm_error;
    status.last_decision_kind = existing.last_decision_kind;
    status.intelligence_state = existing.intelligence_state.or(status.intelligence_state);
    status.active_app = existing.active_app;
    status.last_resolution = existing.last_resolution;
    state.overlay.push(status.clone());
    *state.status.write().await = status;
    Ok(())
}

async fn publish_status(state: &Arc<AppState>, update: StatusUpdate) {
    let mut status = state.status.write().await.clone();

    if let Some(next_state) = update.state {
        status.state = next_state;
    }
    if let Some(mic) = update.current_mic {
        status.current_mic = mic;
    }
    if let Some(partial) = update.partial_transcript {
        status.partial_transcript = partial;
    }
    if let Some(last_error) = update.last_error {
        status.last_error = last_error;
    }
    if let Some(intelligence_ready) = update.intelligence_ready {
        status.intelligence_ready = intelligence_ready;
    }
    if let Some(llm_ready) = update.llm_ready {
        status.llm_ready = llm_ready;
    }
    if let Some(last_llm_error) = update.last_llm_error {
        status.last_llm_error = last_llm_error;
    }
    if let Some(last_decision_kind) = update.last_decision_kind {
        status.last_decision_kind = last_decision_kind;
    }
    if let Some(intelligence_state) = update.intelligence_state {
        status.intelligence_state = intelligence_state;
    }
    if let Some(active_app) = update.active_app {
        status.active_app = active_app;
    }
    if let Some(last_resolution) = update.last_resolution {
        status.last_resolution = last_resolution;
    }

    status.updated_at = Utc::now();

    state.overlay.push(status.clone());
    *state.status.write().await = status;
}

async fn finalize_unresolved_turn(
    state: &Arc<AppState>,
    keyboard: &mut UInputKeyboard,
    committed_prefix: &mut String,
    active_block: &mut Option<FormattingBlock>,
    active_turn: &mut String,
) {
    if active_turn.trim().is_empty() {
        active_turn.clear();
        return;
    }

    let previous_rendered = render_transcript(committed_prefix, active_block.as_ref(), active_turn);
    let next_rendered =
        render_literal_fallback(committed_prefix, active_block.as_ref(), active_turn);
    apply_transcript(state, keyboard, &previous_rendered, &next_rendered).await;
    *committed_prefix = next_rendered;
    *active_block = None;
    active_turn.clear();
}

fn render_transcript(
    committed_prefix: &str,
    active_block: Option<&FormattingBlock>,
    active_turn: &str,
) -> String {
    let committed = render_committed(committed_prefix, active_block);
    append_turn(&committed, active_turn)
}

fn render_committed(committed_prefix: &str, active_block: Option<&FormattingBlock>) -> String {
    match active_block {
        Some(block) => append_block(committed_prefix, &block.rendered_text, &block.format_kind),
        None => committed_prefix.to_string(),
    }
}

fn render_with_segment_decision(
    committed_prefix: &str,
    active_block: Option<&FormattingBlock>,
    segment_text: &str,
    format_kind: &FormatKind,
) -> String {
    let base = render_committed(committed_prefix, active_block);
    append_block(&base, segment_text, format_kind)
}

fn render_literal_fallback(
    committed_prefix: &str,
    active_block: Option<&FormattingBlock>,
    literal_text: &str,
) -> String {
    render_with_segment_decision(
        committed_prefix,
        active_block,
        literal_text,
        &FormatKind::Plain,
    )
}

fn next_block_raw(active_block: Option<&FormattingBlock>, finalized_text: &str) -> String {
    match active_block {
        Some(block) => append_turn(&block.raw_text, finalized_text),
        None => finalized_text.trim().to_string(),
    }
}

fn append_block(committed: &str, block_text: &str, format_kind: &FormatKind) -> String {
    let block_text = block_text.trim();
    if block_text.is_empty() {
        return committed.to_string();
    }
    if committed.is_empty() {
        return block_text.to_string();
    }
    if matches!(
        format_kind,
        FormatKind::NumberedList | FormatKind::BulletList
    ) || block_text.contains('\n')
    {
        if committed.ends_with('\n') {
            format!("{committed}{block_text}")
        } else {
            format!("{committed}\n{block_text}")
        }
    } else {
        append_turn(committed, block_text)
    }
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

fn recent_text_window(text: &str, max_chars: usize) -> String {
    if max_chars == 0 {
        return String::new();
    }

    let chars = text.chars().collect::<Vec<_>>();
    let start = chars.len().saturating_sub(max_chars);
    chars[start..].iter().collect()
}
