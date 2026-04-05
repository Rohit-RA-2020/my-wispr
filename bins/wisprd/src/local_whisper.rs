use std::{fs, path::PathBuf, sync::mpsc, time::Duration};

use hound::{SampleFormat, WavSpec, WavWriter};
use tokio::sync::mpsc as tokio_mpsc;
use webrtc_vad::{SampleRate, Vad, VadMode};
use wispr_core::{Result, WisprError, models::WhisperLocalConfig, whisper};

use crate::deepgram::{TranscriptChunk, TranscriptEvent};

const SAMPLE_RATE_HZ: u32 = 16_000;
const FRAME_MS: usize = 30;
const FRAME_SAMPLES: usize = (SAMPLE_RATE_HZ as usize * FRAME_MS) / 1_000;
const FRAME_BYTES: usize = FRAME_SAMPLES * 2;
const START_VOICED_FRAMES: usize = 2;
const END_SILENCE_FRAMES: usize = 27;
const MIN_UTTERANCE_FRAMES: usize = 10;
const MAX_UTTERANCE_FRAMES: usize = 1_500;

pub struct LocalWhisperSession {
    audio_tx: mpsc::Sender<Vec<u8>>,
    event_rx: tokio_mpsc::Receiver<TranscriptEvent>,
    shutdown_tx: Option<mpsc::Sender<()>>,
}

impl LocalWhisperSession {
    pub async fn connect(config: WhisperLocalConfig) -> Result<Self> {
        whisper::ensure_backend_ready(&config)?;

        let (audio_tx, audio_rx) = mpsc::channel::<Vec<u8>>();
        let (event_tx, event_rx) = tokio_mpsc::channel::<TranscriptEvent>(32);
        let (shutdown_tx, shutdown_rx) = mpsc::channel::<()>();

        std::thread::spawn(move || {
            let mut segmenter = WhisperSegmenter::new();

            loop {
                if shutdown_rx.try_recv().is_ok() {
                    if let Some(utterance) = segmenter.flush() {
                        process_utterance(&config, utterance, &event_tx);
                    }
                    break;
                }

                match audio_rx.recv_timeout(Duration::from_millis(50)) {
                    Ok(frame) => match segmenter.push_bytes(&frame) {
                        Ok(utterances) => {
                            for utterance in utterances {
                                process_utterance(&config, utterance, &event_tx);
                            }
                        }
                        Err(error) => {
                            let _ = event_tx.blocking_send(TranscriptEvent::Warning(format!(
                                "Local Whisper segmentation failed: {error}"
                            )));
                            break;
                        }
                    },
                    Err(mpsc::RecvTimeoutError::Disconnected) => {
                        if let Some(utterance) = segmenter.flush() {
                            process_utterance(&config, utterance, &event_tx);
                        }
                        break;
                    }
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                }
            }
        });

        Ok(Self {
            audio_tx,
            event_rx,
            shutdown_tx: Some(shutdown_tx),
        })
    }

    pub async fn submit_audio(&self, frame: Vec<u8>) -> Result<()> {
        self.audio_tx.send(frame).map_err(|_| {
            WisprError::InvalidState(
                "local Whisper session is no longer accepting audio".to_string(),
            )
        })
    }

    pub async fn next_event(&mut self) -> Option<TranscriptEvent> {
        self.event_rx.recv().await
    }

    pub fn close(&mut self) {
        if let Some(shutdown) = self.shutdown_tx.take() {
            let _ = shutdown.send(());
        }
    }
}

fn process_utterance(
    config: &WhisperLocalConfig,
    utterance: WhisperUtterance,
    event_tx: &tokio_mpsc::Sender<TranscriptEvent>,
) {
    let _ = event_tx.blocking_send(TranscriptEvent::BackendState(
        "Transcribing locally".to_string(),
    ));

    match transcribe_utterance(config, &utterance) {
        Ok(chunk) => {
            if !chunk.text.is_empty() {
                let _ = event_tx.blocking_send(TranscriptEvent::Final(chunk));
                let _ = event_tx.blocking_send(TranscriptEvent::TurnEnded);
            }
            let _ = event_tx.blocking_send(TranscriptEvent::BackendState(
                "Listening (local)".to_string(),
            ));
        }
        Err(error) => {
            let _ = event_tx.blocking_send(TranscriptEvent::Warning(format!(
                "Local Whisper transcription failed: {error}"
            )));
        }
    }
}

fn transcribe_utterance(
    config: &WhisperLocalConfig,
    utterance: &WhisperUtterance,
) -> Result<TranscriptChunk> {
    let wav_path = write_temp_wav(&utterance.samples)?;
    let text = whisper::transcribe_wav(config, &wav_path);
    let _ = fs::remove_file(&wav_path);
    let text = text?;

    Ok(TranscriptChunk {
        text,
        start: utterance.start_sample as f64 / SAMPLE_RATE_HZ as f64,
        duration: utterance.samples.len() as f64 / SAMPLE_RATE_HZ as f64,
    })
}

fn write_temp_wav(samples: &[i16]) -> Result<PathBuf> {
    let path = std::env::temp_dir().join(format!("wispr-utterance-{}.wav", uuid::Uuid::new_v4()));
    let spec = WavSpec {
        channels: 1,
        sample_rate: SAMPLE_RATE_HZ,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };
    let mut writer = WavWriter::create(&path, spec)
        .map_err(|error| WisprError::Message(format!("failed to create WAV file: {error}")))?;
    for sample in samples {
        writer
            .write_sample(*sample)
            .map_err(|error| WisprError::Message(format!("failed to write WAV sample: {error}")))?;
    }
    writer
        .finalize()
        .map_err(|error| WisprError::Message(format!("failed to finalize WAV file: {error}")))?;
    Ok(path)
}

struct WhisperUtterance {
    samples: Vec<i16>,
    start_sample: u64,
}

struct WhisperSegmenter {
    vad: Vad,
    pending_bytes: Vec<u8>,
    pending_voice_frames: Vec<Vec<i16>>,
    pending_voice_start_sample: Option<u64>,
    current_samples: Vec<i16>,
    current_start_sample: Option<u64>,
    current_frame_count: usize,
    silence_frames: usize,
    frames_seen: u64,
}

impl WhisperSegmenter {
    fn new() -> Self {
        Self {
            vad: Vad::new_with_rate_and_mode(SampleRate::Rate16kHz, VadMode::Aggressive),
            pending_bytes: Vec::new(),
            pending_voice_frames: Vec::new(),
            pending_voice_start_sample: None,
            current_samples: Vec::new(),
            current_start_sample: None,
            current_frame_count: 0,
            silence_frames: 0,
            frames_seen: 0,
        }
    }

    fn push_bytes(&mut self, bytes: &[u8]) -> Result<Vec<WhisperUtterance>> {
        self.pending_bytes.extend_from_slice(bytes);
        let mut utterances = Vec::new();

        while self.pending_bytes.len() >= FRAME_BYTES {
            let frame_bytes = self.pending_bytes.drain(..FRAME_BYTES).collect::<Vec<_>>();
            let samples = frame_bytes
                .chunks_exact(2)
                .map(|chunk| i16::from_le_bytes([chunk[0], chunk[1]]))
                .collect::<Vec<_>>();
            let frame_start_sample = self.frames_seen * FRAME_SAMPLES as u64;
            let is_voice = self.vad.is_voice_segment(&samples).map_err(|_| {
                WisprError::InvalidState("invalid audio frame passed to VAD".to_string())
            })?;
            if let Some(utterance) = self.push_frame(samples, is_voice, frame_start_sample) {
                utterances.push(utterance);
            }
            self.frames_seen += 1;
        }

        Ok(utterances)
    }

    fn flush(&mut self) -> Option<WhisperUtterance> {
        self.finalize_current(true)
    }

    fn push_frame(
        &mut self,
        samples: Vec<i16>,
        is_voice: bool,
        frame_start_sample: u64,
    ) -> Option<WhisperUtterance> {
        if self.current_start_sample.is_none() {
            if is_voice {
                if self.pending_voice_start_sample.is_none() {
                    self.pending_voice_start_sample = Some(frame_start_sample);
                }
                self.pending_voice_frames.push(samples);
                if self.pending_voice_frames.len() >= START_VOICED_FRAMES {
                    self.current_start_sample = self.pending_voice_start_sample.take();
                    for frame in self.pending_voice_frames.drain(..) {
                        self.current_samples.extend(frame);
                        self.current_frame_count += 1;
                    }
                }
            } else {
                self.pending_voice_frames.clear();
                self.pending_voice_start_sample = None;
            }
            return None;
        }

        self.current_samples.extend(&samples);
        self.current_frame_count += 1;
        if is_voice {
            self.silence_frames = 0;
        } else {
            self.silence_frames += 1;
        }

        if self.silence_frames >= END_SILENCE_FRAMES {
            return self.finalize_current(false);
        }
        if self.current_frame_count >= MAX_UTTERANCE_FRAMES {
            return self.finalize_current(true);
        }
        None
    }

    fn finalize_current(&mut self, keep_trailing_silence: bool) -> Option<WhisperUtterance> {
        let start_sample = self.current_start_sample.take()?;
        let mut samples = std::mem::take(&mut self.current_samples);
        let mut frame_count = self.current_frame_count;

        if !keep_trailing_silence && self.silence_frames > 0 {
            let trailing_samples = self.silence_frames * FRAME_SAMPLES;
            let trimmed_len = samples.len().saturating_sub(trailing_samples);
            samples.truncate(trimmed_len);
            frame_count = samples.len() / FRAME_SAMPLES;
        }

        self.current_frame_count = 0;
        self.silence_frames = 0;
        self.pending_voice_frames.clear();
        self.pending_voice_start_sample = None;

        if frame_count < MIN_UTTERANCE_FRAMES || samples.is_empty() {
            return None;
        }

        Some(WhisperUtterance {
            samples,
            start_sample,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dummy_frame() -> Vec<i16> {
        vec![0; FRAME_SAMPLES]
    }

    #[test]
    fn drops_short_noise() {
        let mut segmenter = WhisperSegmenter::new();
        let mut emitted = None;
        for index in 0..5 {
            emitted =
                segmenter.push_frame(dummy_frame(), true, index as u64 * FRAME_SAMPLES as u64);
        }
        assert!(emitted.is_none());
        assert!(segmenter.flush().is_none());
    }

    #[test]
    fn flushes_after_silence() {
        let mut segmenter = WhisperSegmenter::new();
        for index in 0..12 {
            let _ = segmenter.push_frame(dummy_frame(), true, index as u64 * FRAME_SAMPLES as u64);
        }
        let mut utterance = None;
        for index in 12..40 {
            utterance =
                segmenter.push_frame(dummy_frame(), false, index as u64 * FRAME_SAMPLES as u64);
            if utterance.is_some() {
                break;
            }
        }
        assert!(utterance.is_some());
    }

    #[test]
    fn force_flushes_long_utterance() {
        let mut segmenter = WhisperSegmenter::new();
        let mut utterance = None;
        for index in 0..(MAX_UTTERANCE_FRAMES + 5) {
            utterance =
                segmenter.push_frame(dummy_frame(), true, index as u64 * FRAME_SAMPLES as u64);
            if utterance.is_some() {
                break;
            }
        }
        assert!(utterance.is_some());
    }
}
