mod app_context;
mod audio;
mod deepgram;
mod local_whisper;
mod overlay;
mod portal;

use std::{
    collections::HashSet,
    future::pending,
    process::Command,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use audio::{AudioCapture, resolve_selected_device};
use chrono::Utc;
use deepgram::{DeepgramSession, TranscriptChunk, TranscriptEvent};
use local_whisper::LocalWhisperSession;
use tokio::sync::{Mutex, RwLock, mpsc, oneshot};
use tokio::time::{self, Duration, Instant};
use tracing::{error, info, warn};
use wispr_core::{
    AppConfig, CorrectionScope, DecisionKind, FormattingTriggerPolicy, GenerationRequest,
    LlmInterpreter, PreferredListStyle, Result, RewriteScope, SegmentDecisionRequest, WisprError,
    models::{
        ActiveAppContext, DaemonStatus, DeviceChoice, DictationState, FormatKind,
        TranscriptionProvider,
    },
    resolve_actions,
    secrets::SecretStore,
    typing::{UInputKeyboard, diff_patch},
    whisper,
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
    cancel_flag: Arc<AtomicBool>,
}

struct DeepgramBackend {
    session: DeepgramSession,
    audio_tx: mpsc::Sender<Vec<u8>>,
    pending_audio: Vec<u8>,
}

enum TranscriptionBackend {
    Deepgram(DeepgramBackend),
    WhisperLocal(LocalWhisperSession),
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
    transcription_provider: Option<TranscriptionProvider>,
    transcription_ready: Option<bool>,
    transcription_state: Option<Option<String>>,
    selected_whisper_model: Option<Option<String>>,
    last_transcription_error: Option<Option<String>>,
    intelligence_ready: Option<bool>,
    llm_ready: Option<bool>,
    last_llm_error: Option<Option<String>>,
    last_decision_kind: Option<Option<DecisionKind>>,
    intelligence_state: Option<Option<String>>,
    active_app: Option<Option<ActiveAppContext>>,
    last_resolution: Option<Option<String>>,
    generation_active: Option<bool>,
    generation_ready: Option<bool>,
    last_generation_error: Option<Option<String>>,
    generation_state: Option<Option<String>>,
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

impl TranscriptionBackend {
    async fn submit_audio(&mut self, frame: Vec<u8>) -> Result<()> {
        match self {
            Self::Deepgram(backend) => {
                backend.pending_audio.extend_from_slice(&frame);
                while backend.pending_audio.len() >= DEEPGRAM_FRAME_BYTES {
                    let chunk = backend
                        .pending_audio
                        .drain(..DEEPGRAM_FRAME_BYTES)
                        .collect::<Vec<u8>>();
                    backend.audio_tx.send(chunk).await.map_err(|_| {
                        WisprError::InvalidState("Failed to forward audio to Deepgram".to_string())
                    })?;
                }
                Ok(())
            }
            Self::WhisperLocal(session) => session.submit_audio(frame).await,
        }
    }

    async fn next_event(&mut self) -> Option<TranscriptEvent> {
        match self {
            Self::Deepgram(backend) => backend.session.next_event().await,
            Self::WhisperLocal(session) => session.next_event().await,
        }
    }

    async fn close(&mut self) {
        match self {
            Self::Deepgram(backend) => {
                if !backend.pending_audio.is_empty() {
                    let chunk = std::mem::take(&mut backend.pending_audio);
                    let _ = backend.audio_tx.send(chunk).await;
                }
                backend.session.close_stream();
            }
            Self::WhisperLocal(session) => session.close(),
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
    let mut keyboard = wispr_core::typing::UInputKeyboard::open().map_err(|error| {
        WisprError::InvalidState(format!("Typing engine failed to open /dev/uinput: {error}"))
    })?;

    let llm_state = load_llm_interpreter(&config, &secret_store).await?;
    let llm_ready = llm_state.is_some();
    let intelligence_ready = !config.intelligence.enabled || llm_ready;
    let generation_ready =
        !config.intelligence.enabled || !config.intelligence.generation_enabled || llm_ready;
    let llm_setup_error = if config.intelligence.enabled && !llm_ready {
        Some("Intelligence is enabled but the LLM backend is not configured. Falling back to literal dictation.".to_string())
    } else {
        None
    };

    let capture = AudioCapture::start(&selected)?;
    let audio_rx = capture.receiver();
    let transcription = build_transcription_backend(&config, &secret_store).await?;
    let (stop_tx, mut stop_rx) = oneshot::channel::<()>();
    let cancel_flag = Arc::new(AtomicBool::new(false));
    let transcription_label = active_transcription_label(&config);

    publish_status(
        &state,
        StatusUpdate {
            state: Some(DictationState::Listening),
            current_mic: Some(Some(selected.clone())),
            partial_transcript: Some(match config.transcription.provider {
                TranscriptionProvider::Deepgram => Some(String::new()),
                TranscriptionProvider::WhisperLocal => None,
            }),
            transcription_provider: Some(config.transcription.provider.clone()),
            transcription_ready: Some(true),
            transcription_state: Some(Some(transcription_label)),
            selected_whisper_model: Some(selected_whisper_model(&config)),
            last_transcription_error: Some(None),
            intelligence_ready: Some(intelligence_ready),
            llm_ready: Some(llm_ready),
            last_error: Some(None),
            last_llm_error: Some(llm_setup_error.clone()),
            last_generation_error: Some(None),
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
            generation_active: Some(false),
            generation_ready: Some(generation_ready),
            generation_state: Some(Some(if config.intelligence.generation_enabled {
                if generation_ready {
                    "Generation ready".to_string()
                } else {
                    "Generation unavailable".to_string()
                }
            } else {
                "Generation disabled".to_string()
            })),
        },
    )
    .await;

    let state_for_task = state.clone();
    let cancel_for_task = cancel_flag.clone();
    tokio::spawn(async move {
        let mut committed_prefix = String::new();
        let mut active_block = None::<FormattingBlock>;
        let mut active_turn = String::new();
        let mut logged_audio = false;
        let mut shutting_down = false;
        let mut shutdown_deadline = None::<Instant>;
        let mut processed_finals = HashSet::<String>::new();
        let llm_interpreter = llm_state;
        let mut transcription = transcription;

        loop {
            tokio::select! {
                _ = &mut stop_rx, if !shutting_down => {
                    let _ = capture.stop();
                    transcription.close().await;
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
                            if let Err(error) = transcription.submit_audio(frame).await {
                                publish_status(
                                    &state_for_task,
                                    StatusUpdate {
                                        state: Some(DictationState::Error),
                                        last_error: Some(Some(error.to_string())),
                                        last_transcription_error: Some(Some(error.to_string())),
                                        generation_active: Some(false),
                                        ..StatusUpdate::default()
                                    },
                                )
                                .await;
                                let mut guard = state_for_task.session.lock().await;
                                *guard = None;
                                break;
                            }
                        }
                        Err(_) => {
                            publish_status(
                                &state_for_task,
                                StatusUpdate {
                                    state: Some(DictationState::Error),
                                    last_error: Some(Some("Audio stream ended unexpectedly".to_string())),
                                    generation_active: Some(false),
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
                maybe_event = transcription.next_event() => {
                    let Some(event) = maybe_event else {
                        publish_status(
                            &state_for_task,
                            StatusUpdate {
                                state: Some(DictationState::Idle),
                                partial_transcript: Some(None),
                                transcription_state: Some(None),
                                intelligence_state: Some(None),
                                active_app: Some(None),
                                last_resolution: Some(None),
                                generation_active: Some(false),
                                ..StatusUpdate::default()
                            },
                        )
                        .await;
                        let mut guard = state_for_task.session.lock().await;
                        *guard = None;
                        break;
                    };

                    match event {
                        TranscriptEvent::BackendState(message) => {
                            publish_status(
                                &state_for_task,
                                StatusUpdate {
                                    transcription_state: Some(Some(message)),
                                    ..StatusUpdate::default()
                                },
                            )
                            .await;
                        }
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
                                &cancel_for_task,
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
                                    &cancel_for_task,
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
                                    last_error: Some(Some(message.clone())),
                                    last_transcription_error: Some(Some(message)),
                                    generation_active: Some(false),
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
                            transcription_state: Some(None),
                            intelligence_state: Some(None),
                            active_app: Some(None),
                            last_resolution: Some(None),
                            generation_active: Some(false),
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
    *guard = Some(RunningSession {
        stop_tx,
        cancel_flag,
    });
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
    cancel_flag: &Arc<AtomicBool>,
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
                if output.decision.kind == DecisionKind::Generation {
                    process_generation_segment(
                        state,
                        config,
                        keyboard,
                        interpreter,
                        committed_prefix,
                        active_block,
                        active_turn,
                        cancel_flag,
                        previous_rendered,
                        &request,
                        output.decision,
                        chunk,
                    )
                    .await;
                    return;
                }

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
                        apply_literal_fallback(
                            state,
                            keyboard,
                            committed_prefix,
                            active_block,
                            active_turn,
                            &previous_rendered,
                            &chunk.text,
                        )
                        .await;
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
                copy_final_literal_transcript(
                    state,
                    clipboard_text_for_decision(&output.decision).as_deref(),
                );
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

    apply_literal_fallback(
        state,
        keyboard,
        committed_prefix,
        active_block,
        active_turn,
        &previous_rendered,
        &chunk.text,
    )
    .await;
}

#[allow(clippy::too_many_arguments)]
async fn process_generation_segment(
    state: &Arc<AppState>,
    config: &AppConfig,
    keyboard: &mut UInputKeyboard,
    interpreter: &LlmInterpreter,
    committed_prefix: &mut String,
    active_block: &mut Option<FormattingBlock>,
    active_turn: &mut String,
    cancel_flag: &Arc<AtomicBool>,
    previous_rendered: String,
    request: &SegmentDecisionRequest,
    decision: wispr_core::SegmentDecision,
    chunk: TranscriptChunk,
) {
    if !config.intelligence.generation_enabled {
        apply_literal_fallback(
            state,
            keyboard,
            committed_prefix,
            active_block,
            active_turn,
            &previous_rendered,
            &chunk.text,
        )
        .await;
        return;
    }

    let generation_prompt = match decision.generation_prompt.clone() {
        Some(prompt) if !prompt.trim().is_empty() => prompt,
        _ => {
            warn!("generation decision is missing generation_prompt");
            apply_literal_fallback(
                state,
                keyboard,
                committed_prefix,
                active_block,
                active_turn,
                &previous_rendered,
                &chunk.text,
            )
            .await;
            return;
        }
    };

    let generation_request = GenerationRequest {
        request_text: chunk.text.clone(),
        generation_prompt,
        generation_style: decision.generation_style.unwrap_or_default(),
        recent_text: request.recent_text.clone(),
        active_app: request.active_app.clone(),
    };

    let generation_prefix = render_transcript(committed_prefix, active_block.as_ref(), "");
    let mut current_rendered = generation_prefix.clone();
    apply_transcript(state, keyboard, &previous_rendered, &current_rendered).await;

    publish_status(
        state,
        StatusUpdate {
            last_decision_kind: Some(Some(DecisionKind::Generation)),
            last_llm_error: Some(None),
            last_generation_error: Some(None),
            intelligence_state: Some(Some("Generating reply".to_string())),
            generation_active: Some(true),
            generation_state: Some(Some("Generating reply".to_string())),
            active_app: Some(request.active_app.clone()),
            last_resolution: Some(Some("Autonomous writing".to_string())),
            ..StatusUpdate::default()
        },
    )
    .await;

    let mut generated_text = String::new();
    let mut generation_stream =
        interpreter.start_generation_stream(generation_request, cancel_flag.clone());

    while let Some(next) = generation_stream.recv().await {
        if cancel_flag.load(Ordering::Relaxed) {
            break;
        }

        match next {
            Ok(delta) => {
                if delta.is_empty() {
                    continue;
                }
                generated_text.push_str(&delta);
                let next_rendered = render_generated_output(&generation_prefix, &generated_text);
                apply_transcript(state, keyboard, &current_rendered, &next_rendered).await;
                current_rendered = next_rendered;
            }
            Err(error) => {
                warn!("generation failed: {error}");
                if generated_text.is_empty() {
                    apply_literal_fallback(
                        state,
                        keyboard,
                        committed_prefix,
                        active_block,
                        active_turn,
                        &current_rendered,
                        &chunk.text,
                    )
                    .await;
                    publish_status(
                        state,
                        StatusUpdate {
                            generation_active: Some(false),
                            generation_state: Some(Some("Literal fallback".to_string())),
                            last_generation_error: Some(Some(error.to_string())),
                            intelligence_state: Some(Some("Literal fallback".to_string())),
                            ..StatusUpdate::default()
                        },
                    )
                    .await;
                    return;
                }

                *committed_prefix = current_rendered;
                *active_block = None;
                active_turn.clear();
                publish_status(
                    state,
                    StatusUpdate {
                        generation_active: Some(false),
                        generation_state: Some(Some("Generation failed".to_string())),
                        last_generation_error: Some(Some(error.to_string())),
                        intelligence_state: Some(Some("Generation failed".to_string())),
                        ..StatusUpdate::default()
                    },
                )
                .await;
                return;
            }
        }
    }

    if cancel_flag.load(Ordering::Relaxed) {
        if generated_text.is_empty() {
            apply_literal_fallback(
                state,
                keyboard,
                committed_prefix,
                active_block,
                active_turn,
                &current_rendered,
                &chunk.text,
            )
            .await;
        } else {
            *committed_prefix = current_rendered;
            *active_block = None;
            active_turn.clear();
        }
        publish_status(
            state,
            StatusUpdate {
                generation_active: Some(false),
                generation_state: Some(Some("Generation stopped".to_string())),
                intelligence_state: Some(Some("Generation stopped".to_string())),
                ..StatusUpdate::default()
            },
        )
        .await;
        return;
    }

    *committed_prefix = current_rendered;
    *active_block = None;
    active_turn.clear();
    publish_status(
        state,
        StatusUpdate {
            generation_active: Some(false),
            generation_state: Some(Some("Generation complete".to_string())),
            last_generation_error: Some(None),
            intelligence_state: Some(Some("Generation complete".to_string())),
            ..StatusUpdate::default()
        },
    )
    .await;
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

async fn apply_literal_fallback(
    state: &Arc<AppState>,
    keyboard: &mut UInputKeyboard,
    committed_prefix: &mut String,
    active_block: &mut Option<FormattingBlock>,
    active_turn: &mut String,
    previous_rendered: &str,
    literal_text: &str,
) {
    let next_rendered =
        render_literal_fallback(committed_prefix, active_block.as_ref(), literal_text);
    copy_final_literal_transcript(
        state,
        normalized_literal_clipboard_text(literal_text).as_deref(),
    );
    apply_transcript(state, keyboard, previous_rendered, &next_rendered).await;
    *committed_prefix = next_rendered;
    *active_block = None;
    active_turn.clear();
}

fn copy_final_literal_transcript(state: &Arc<AppState>, text: Option<&str>) {
    let Some(text) = text else {
        return;
    };

    if let Err(error) = state.overlay.copy_text(text) {
        warn!("failed to copy literal transcript to clipboard: {error}");
    }
}

fn clipboard_text_for_decision(decision: &wispr_core::SegmentDecision) -> Option<String> {
    if decision.kind == DecisionKind::Literal {
        normalized_literal_clipboard_text(&decision.text_to_emit)
    } else {
        None
    }
}

fn normalized_literal_clipboard_text(text: &str) -> Option<String> {
    let rendered = append_block("", text, &FormatKind::Plain);
    (!rendered.is_empty()).then_some(rendered)
}

async fn stop_dictation(state: Arc<AppState>) -> Result<String> {
    let session = {
        let mut guard = state.session.lock().await;
        guard.take()
    };
    let Some(session) = session else {
        return Ok("Dictation is not active".to_string());
    };

    session.cancel_flag.store(true, Ordering::Relaxed);
    let _ = session.stop_tx.send(());
    publish_status(
        &state,
        StatusUpdate {
            state: Some(DictationState::Idle),
            partial_transcript: Some(None),
            intelligence_state: Some(None),
            active_app: Some(None),
            last_resolution: Some(None),
            generation_active: Some(false),
            generation_state: Some(Some("Generation stopped".to_string())),
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
    let transcription_provider = config.transcription.provider.clone();
    let (transcription_ready, transcription_state, last_transcription_error) =
        build_transcription_status(&config).await;
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
    let generation_ready =
        !config.intelligence.enabled || !config.intelligence.generation_enabled || llm_ready;

    DaemonStatus {
        state: state_override.unwrap_or(DictationState::Idle),
        mic_ready: resolve_selected_device(&config).is_some(),
        typing_ready: std::fs::OpenOptions::new()
            .write(true)
            .open("/dev/uinput")
            .is_ok(),
        hotkey_ready: *state.hotkey_ready.read().await,
        transcription_provider,
        transcription_ready,
        transcription_state,
        selected_whisper_model: selected_whisper_model(&config),
        last_transcription_error,
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
        generation_active: false,
        generation_ready,
        last_generation_error: None,
        generation_state: Some(if config.intelligence.generation_enabled {
            if generation_ready {
                "Generation ready".to_string()
            } else {
                "Generation unavailable".to_string()
            }
        } else {
            "Generation disabled".to_string()
        }),
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
    status.generation_active = existing.generation_active;
    status.last_generation_error = existing.last_generation_error;
    status.generation_state = existing.generation_state.or(status.generation_state);
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
    if let Some(transcription_provider) = update.transcription_provider {
        status.transcription_provider = transcription_provider;
    }
    if let Some(transcription_ready) = update.transcription_ready {
        status.transcription_ready = transcription_ready;
    }
    if let Some(transcription_state) = update.transcription_state {
        status.transcription_state = transcription_state;
    }
    if let Some(selected_whisper_model) = update.selected_whisper_model {
        status.selected_whisper_model = selected_whisper_model;
    }
    if let Some(last_transcription_error) = update.last_transcription_error {
        status.last_transcription_error = last_transcription_error;
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
    if let Some(generation_active) = update.generation_active {
        status.generation_active = generation_active;
    }
    if let Some(generation_ready) = update.generation_ready {
        status.generation_ready = generation_ready;
    }
    if let Some(last_generation_error) = update.last_generation_error {
        status.last_generation_error = last_generation_error;
    }
    if let Some(generation_state) = update.generation_state {
        status.generation_state = generation_state;
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
    let literal_text = active_turn.clone();
    apply_literal_fallback(
        state,
        keyboard,
        committed_prefix,
        active_block,
        active_turn,
        &previous_rendered,
        &literal_text,
    )
    .await;
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

fn render_generated_output(committed_prefix: &str, generated_text: &str) -> String {
    if committed_prefix.is_empty() {
        return generated_text.to_string();
    }
    if generated_text.is_empty() {
        return committed_prefix.to_string();
    }
    if generated_text.starts_with('\n') {
        return format!("{committed_prefix}{generated_text}");
    }
    if needs_separator(committed_prefix, generated_text) {
        format!("{committed_prefix} {generated_text}")
    } else {
        format!("{committed_prefix}{generated_text}")
    }
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

async fn build_transcription_backend(
    config: &AppConfig,
    secret_store: &SecretStore,
) -> Result<TranscriptionBackend> {
    match &config.transcription.provider {
        TranscriptionProvider::Deepgram => {
            let api_key = secret_store.get_api_key().await?.ok_or_else(|| {
                WisprError::InvalidState(
                    "No Deepgram API key is stored yet. Open wispr-settings first.".to_string(),
                )
            })?;
            let session = DeepgramSession::connect(&api_key).await?;
            let audio_tx = session.audio_sender();
            Ok(TranscriptionBackend::Deepgram(DeepgramBackend {
                session,
                audio_tx,
                pending_audio: Vec::new(),
            }))
        }
        TranscriptionProvider::WhisperLocal => Ok(TranscriptionBackend::WhisperLocal(
            LocalWhisperSession::connect(config.transcription.whisper_local.clone()).await?,
        )),
    }
}

async fn build_transcription_status(config: &AppConfig) -> (bool, Option<String>, Option<String>) {
    match &config.transcription.provider {
        TranscriptionProvider::Deepgram => {
            let ready = match SecretStore::connect().await {
                Ok(store) => store
                    .get_api_key()
                    .await
                    .ok()
                    .flatten()
                    .map(|key| !key.trim().is_empty())
                    .unwrap_or(false),
                Err(_) => false,
            };
            let state = if ready {
                Some("Deepgram ready".to_string())
            } else {
                Some("Deepgram not configured".to_string())
            };
            (ready, state, None)
        }
        TranscriptionProvider::WhisperLocal => {
            let status = whisper::collect_manager_status(&config.transcription.whisper_local);
            let model_installed = whisper::is_model_installed(
                &config.transcription.whisper_local,
                &config.transcription.whisper_local.model,
            );
            let ready = status.runtime.backend_ready() && model_installed;
            let state = if ready {
                Some(format!(
                    "Whisper ready ({})",
                    config.transcription.whisper_local.model
                ))
            } else if !status.runtime.backend_ready() {
                Some("Whisper unavailable".to_string())
            } else {
                Some("Whisper model not installed".to_string())
            };
            let error = (!model_installed)
                .then(|| {
                    format!(
                        "Whisper model {} is not installed.",
                        config.transcription.whisper_local.model
                    )
                })
                .or(status.runtime.detail.clone());
            (ready, state, error)
        }
    }
}

fn selected_whisper_model(config: &AppConfig) -> Option<String> {
    matches!(
        config.transcription.provider,
        TranscriptionProvider::WhisperLocal
    )
    .then(|| config.transcription.whisper_local.model.clone())
}

fn active_transcription_label(config: &AppConfig) -> String {
    match &config.transcription.provider {
        TranscriptionProvider::Deepgram => "Listening (cloud)".to_string(),
        TranscriptionProvider::WhisperLocal => "Listening (local)".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wispr_core::{ActionType, RewriteScope, SegmentDecision};

    fn decision_with_kind(kind: DecisionKind, text: &str) -> SegmentDecision {
        SegmentDecision {
            kind,
            rewrite_scope: RewriteScope::Segment,
            format_kind: FormatKind::Plain,
            text_to_emit: text.to_string(),
            keep_block_open: false,
            actions: Vec::new(),
            generation_prompt: None,
            generation_style: None,
            replace_current_segment: false,
        }
    }

    #[test]
    fn clipboard_copy_is_limited_to_pure_literal_decisions() {
        let literal = SegmentDecision::literal("hello world");
        let action = decision_with_kind(DecisionKind::Action, "copy");
        let literal_and_action = SegmentDecision {
            actions: vec![wispr_core::ActionCommand {
                action_type: ActionType::Key,
                key: Some(wispr_core::ActionKey::Enter),
                modifiers: Vec::new(),
                repeat: 1,
                command_id: None,
                target_app: None,
            }],
            ..decision_with_kind(DecisionKind::LiteralAndAction, "hello")
        };
        let generation = decision_with_kind(DecisionKind::Generation, "");

        assert_eq!(
            clipboard_text_for_decision(&literal),
            Some("hello world".to_string())
        );
        assert_eq!(clipboard_text_for_decision(&action), None);
        assert_eq!(clipboard_text_for_decision(&literal_and_action), None);
        assert_eq!(clipboard_text_for_decision(&generation), None);
    }

    #[test]
    fn literal_fallback_clipboard_text_matches_segment_emission() {
        let literal_text = "  pasted later  ";
        let rendered_segment =
            render_with_segment_decision("", None, literal_text, &FormatKind::Plain);
        let rendered_with_prefix = render_literal_fallback("already there", None, literal_text);

        assert_eq!(
            normalized_literal_clipboard_text(literal_text),
            Some(rendered_segment)
        );
        assert_eq!(rendered_with_prefix, "already there pasted later");
    }
}
