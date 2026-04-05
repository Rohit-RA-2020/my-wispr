use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use tokio::sync::{mpsc, oneshot};
use tokio::time::{self, Duration};
use tokio_tungstenite::{
    connect_async,
    tungstenite::{
        Message,
        client::IntoClientRequest,
        http::{HeaderValue, header::AUTHORIZATION},
    },
};
use tracing::info;
use wispr_core::error::{Result, WisprError};

const DEEPGRAM_URL: &str = "wss://api.deepgram.com/v1/listen?model=nova-3&language=en&encoding=linear16&sample_rate=16000&interim_results=true&smart_format=true&punctuate=true&dictation=true&utterance_end_ms=1500&tag=wispr";

#[derive(Debug, Clone)]
pub struct TranscriptChunk {
    pub text: String,
    pub start: f64,
    pub duration: f64,
}

impl TranscriptChunk {
    pub fn dedupe_key(&self) -> String {
        format!("{:.3}:{:.3}:{}", self.start, self.duration, self.text)
    }
}

#[derive(Debug, Clone)]
pub enum TranscriptEvent {
    Partial(TranscriptChunk),
    Final(TranscriptChunk),
    TurnEnded,
    TurnEndedWithTranscript(TranscriptChunk),
    BackendState(String),
    Warning(String),
}

pub struct DeepgramSession {
    audio_tx: mpsc::Sender<Vec<u8>>,
    event_rx: mpsc::Receiver<TranscriptEvent>,
    shutdown_tx: Option<oneshot::Sender<()>>,
}

impl DeepgramSession {
    pub async fn connect(api_key: &str) -> Result<Self> {
        let mut request = DEEPGRAM_URL
            .into_client_request()
            .map_err(|err| WisprError::Message(err.to_string()))?;
        let token = format!("Token {api_key}");
        request.headers_mut().insert(
            AUTHORIZATION,
            HeaderValue::from_str(&token).map_err(|err| WisprError::Message(err.to_string()))?,
        );

        let (ws_stream, _) = connect_async(request)
            .await
            .map_err(|err| WisprError::Message(err.to_string()))?;
        let (mut ws_sink, mut ws_stream) = ws_stream.split();

        let (audio_tx, mut audio_rx) = mpsc::channel::<Vec<u8>>(32);
        let (event_tx, event_rx) = mpsc::channel::<TranscriptEvent>(32);
        let (shutdown_tx, mut shutdown_rx) = oneshot::channel::<()>();

        let event_tx_writer = event_tx.clone();
        tokio::spawn(async move {
            let mut keepalive = time::interval(Duration::from_secs(3));
            loop {
                tokio::select! {
                    maybe_chunk = audio_rx.recv() => {
                        let Some(chunk) = maybe_chunk else {
                            let _ = ws_sink.send(Message::Text("{\"type\":\"CloseStream\"}".into())).await;
                            break;
                        };

                        if let Err(error) = ws_sink.send(Message::Binary(chunk.into())).await {
                            let _ = event_tx_writer
                                .send(TranscriptEvent::Warning(format!("Deepgram audio send failed: {error}")))
                                .await;
                            break;
                        }
                    }
                    _ = keepalive.tick() => {
                        let _ = ws_sink
                            .send(Message::Text("{\"type\":\"KeepAlive\"}".into()))
                            .await;
                    }
                    _ = &mut shutdown_rx => {
                        let _ = ws_sink.send(Message::Text("{\"type\":\"CloseStream\"}".into())).await;
                        break;
                    }
                }
            }
        });

        tokio::spawn(async move {
            while let Some(message) = ws_stream.next().await {
                match message {
                    Ok(Message::Text(text)) => {
                        info!("deepgram raw: {}", text);
                        if let Ok(parsed) = serde_json::from_str::<DeepgramMessage>(&text) {
                            if let Some(event) = parsed.into_event() {
                                let _ = event_tx.send(event).await;
                            }
                        }
                    }
                    Ok(Message::Close(_)) => break,
                    Ok(_) => {}
                    Err(error) => {
                        let _ = event_tx
                            .send(TranscriptEvent::Warning(format!(
                                "Deepgram stream error: {error}"
                            )))
                            .await;
                        break;
                    }
                }
            }
        });

        Ok(Self {
            audio_tx,
            event_rx,
            shutdown_tx: Some(shutdown_tx),
        })
    }

    pub fn audio_sender(&self) -> mpsc::Sender<Vec<u8>> {
        self.audio_tx.clone()
    }

    pub async fn next_event(&mut self) -> Option<TranscriptEvent> {
        self.event_rx.recv().await
    }

    pub fn close_stream(&mut self) {
        if let Some(shutdown) = self.shutdown_tx.take() {
            let _ = shutdown.send(());
        }
    }
}

#[derive(Debug, Deserialize)]
struct DeepgramMessage {
    #[serde(rename = "type")]
    kind: Option<String>,
    channel: Option<DeepgramChannel>,
    is_final: Option<bool>,
    speech_final: Option<bool>,
    description: Option<String>,
    message: Option<String>,
    error: Option<String>,
    event: Option<String>,
    transcript: Option<String>,
    start: Option<f64>,
    duration: Option<f64>,
}

#[derive(Debug, Deserialize)]
struct DeepgramChannel {
    alternatives: Vec<DeepgramAlternative>,
}

#[derive(Debug, Deserialize)]
struct DeepgramAlternative {
    transcript: String,
}

impl DeepgramMessage {
    fn into_event(self) -> Option<TranscriptEvent> {
        match self.kind.as_deref() {
            Some("Results") => {
                let transcript = self
                    .channel
                    .and_then(|channel| channel.alternatives.into_iter().next())
                    .map(|alt| alt.transcript)
                    .unwrap_or_default();

                if transcript.is_empty() {
                    return None;
                }

                let chunk = TranscriptChunk {
                    text: transcript,
                    start: self.start.unwrap_or_default(),
                    duration: self.duration.unwrap_or_default(),
                };

                if self.is_final.unwrap_or(false) || self.speech_final.unwrap_or(false) {
                    Some(TranscriptEvent::Final(chunk))
                } else {
                    Some(TranscriptEvent::Partial(chunk))
                }
            }
            Some("UtteranceEnd") => Some(TranscriptEvent::TurnEnded),
            Some("SpeechStarted") => None,
            Some("TurnInfo") => {
                let transcript = self.transcript.unwrap_or_default();
                let chunk = TranscriptChunk {
                    text: transcript,
                    start: self.start.unwrap_or_default(),
                    duration: self.duration.unwrap_or_default(),
                };

                match self.event.as_deref() {
                    Some("Update") | Some("EagerEndOfTurn") | Some("TurnResumed") => {
                        if chunk.text.is_empty() {
                            None
                        } else {
                            Some(TranscriptEvent::Partial(chunk))
                        }
                    }
                    Some("EndOfTurn") => {
                        if chunk.text.is_empty() {
                            Some(TranscriptEvent::TurnEnded)
                        } else {
                            Some(TranscriptEvent::TurnEndedWithTranscript(chunk))
                        }
                    }
                    Some("StartOfTurn") => None,
                    Some(_) | None => {
                        if chunk.text.is_empty() {
                            Some(TranscriptEvent::TurnEnded)
                        } else {
                            Some(TranscriptEvent::Partial(chunk))
                        }
                    }
                }
            }
            Some("Connected") | Some("Metadata") => None,
            Some("Error") => Some(TranscriptEvent::Warning(
                self.description
                    .or(self.message)
                    .or(self.error)
                    .unwrap_or_else(|| "Deepgram returned an unspecified error".to_string()),
            )),
            Some(other) => Some(TranscriptEvent::Warning(format!(
                "Unhandled Deepgram message: {other}"
            ))),
            None => None,
        }
    }
}
