use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use futures_util::StreamExt;
use reqwest::{Client, RequestBuilder};
use serde_json::{Value, json};
use tokio::sync::mpsc;
use tracing::warn;

use crate::{
    config::IntelligenceConfig,
    error::{Result, WisprError},
    models::{
        ActionCommand, ActionScope, ActionType, CommandMode, CorrectionScope, DecisionKind,
        FormattingTriggerPolicy, GenerationRequest, GenerationStyle, ModifierKey,
        PreferredListStyle, SegmentDecision, SegmentDecisionRequest, TextOutputMode,
    },
};

pub struct LlmInterpreter {
    client: Client,
    config: IntelligenceConfig,
    api_key: String,
}

pub struct InterpreterOutput {
    pub decision: SegmentDecision,
    pub streamed_text: String,
}

impl LlmInterpreter {
    pub fn new(config: IntelligenceConfig, api_key: impl Into<String>) -> Result<Self> {
        let client = build_http_client(config.timeout_ms.max(250))?;
        Ok(Self {
            client,
            config,
            api_key: api_key.into(),
        })
    }

    pub fn is_enabled(&self) -> bool {
        self.config.enabled
    }

    pub async fn test_connection(&self) -> Result<InterpreterOutput> {
        self.decide(&SegmentDecisionRequest {
            segment_id: "wispr-test".to_string(),
            finalized_text: "hello enter".to_string(),
            literal_text: "hello enter".to_string(),
            recent_text: String::new(),
            active_block_raw: String::new(),
            active_block_rendered: String::new(),
            action_scope: self.config.action_scope.clone(),
            command_mode: CommandMode::AlwaysInfer,
            text_output_mode: TextOutputMode::Literal,
            preferred_list_style: PreferredListStyle::Numbered,
            formatting_trigger_policy: FormattingTriggerPolicy::ClearStructureOnly,
            correction_scope: CorrectionScope::CurrentBlockOnly,
            active_app: None,
        })
        .await
    }

    pub async fn decide(&self, request: &SegmentDecisionRequest) -> Result<InterpreterOutput> {
        let endpoint = format!("{}/responses", self.config.base_url.trim_end_matches('/'));
        let streaming_response = self
            .authorized_post(endpoint)
            .json(&build_request_body(&self.config, request, true))
            .send()
            .await?;

        let status = streaming_response.status();
        let content_type = streaming_response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_string();

        if content_type.contains("text/event-stream") {
            match self
                .decode_streaming_response(request, streaming_response, status)
                .await
            {
                Ok(output) => return Ok(output),
                Err(error) if should_retry_non_streaming(&error) => {
                    return self.decide_non_streaming(request).await;
                }
                Err(error) => return Err(error),
            }
        }

        self.decode_non_streaming_response(request, streaming_response, status)
            .await
    }

    async fn decide_non_streaming(
        &self,
        request: &SegmentDecisionRequest,
    ) -> Result<InterpreterOutput> {
        let endpoint = format!("{}/responses", self.config.base_url.trim_end_matches('/'));
        let response = self
            .authorized_post(endpoint)
            .json(&build_request_body(&self.config, request, false))
            .send()
            .await?;
        let status = response.status();
        self.decode_non_streaming_response(request, response, status)
            .await
    }

    async fn decode_streaming_response(
        &self,
        request: &SegmentDecisionRequest,
        response: reqwest::Response,
        status: reqwest::StatusCode,
    ) -> Result<InterpreterOutput> {
        let mut raw_stream = String::new();
        let mut streamed_text = String::new();
        let mut completion_payload = None::<Value>;
        let mut buffer = String::new();
        let mut saw_structured_output = false;

        let mut stream = response.bytes_stream();
        while let Some(next) = stream.next().await {
            match next {
                Ok(chunk) => {
                    let chunk_text = String::from_utf8_lossy(&chunk);
                    raw_stream.push_str(&chunk_text);
                    buffer.push_str(&chunk_text);

                    while let Some(frame) = pop_sse_frame(&mut buffer) {
                        saw_structured_output |= process_sse_frame(
                            &frame,
                            &mut streamed_text,
                            &mut completion_payload,
                            &raw_stream,
                        )?;
                    }
                }
                Err(error) => {
                    if saw_structured_output || completion_payload.is_some() {
                        break;
                    }
                    return Err(WisprError::Http(format!(
                        "error decoding response body: {error}"
                    )));
                }
            }
        }

        while let Some(frame) = pop_sse_frame(&mut buffer) {
            let _ = process_sse_frame(
                &frame,
                &mut streamed_text,
                &mut completion_payload,
                &raw_stream,
            )?;
        }

        if !status.is_success() && raw_stream.trim().is_empty() {
            return Err(WisprError::Http(format!(
                "HTTP {} from LLM backend with empty event stream",
                status
            )));
        }

        if streamed_text.trim().is_empty() {
            if let Some(event) = completion_payload {
                if let Some(text) = extract_completed_output_text(&event) {
                    streamed_text = text;
                }
            }
        }

        if streamed_text.trim().is_empty() {
            return Err(WisprError::InvalidState(
                "LLM interpreter returned no structured output".to_string(),
            ));
        }

        let parsed = serde_json::from_str::<SegmentDecision>(streamed_text.trim())?;
        let decision = validate_decision(request, parsed)?;

        Ok(InterpreterOutput {
            decision,
            streamed_text,
        })
    }

    async fn decode_non_streaming_response(
        &self,
        request: &SegmentDecisionRequest,
        response: reqwest::Response,
        status: reqwest::StatusCode,
    ) -> Result<InterpreterOutput> {
        let body_bytes = response
            .bytes()
            .await
            .map_err(|error| WisprError::Http(format!("error decoding response body: {error}")))?;
        let body_text = String::from_utf8_lossy(&body_bytes).to_string();

        if !status.is_success() {
            return Err(WisprError::Http(format!(
                "HTTP {} from LLM backend: {}",
                status,
                truncate_for_error(&body_text)
            )));
        }

        self.decode_non_streaming_response_text(request, &body_text)
    }

    fn decode_non_streaming_response_text(
        &self,
        request: &SegmentDecisionRequest,
        body_text: &str,
    ) -> Result<InterpreterOutput> {
        let value: Value = serde_json::from_str(body_text).map_err(|error| {
            WisprError::Http(format!(
                "failed to parse non-streaming response body: {error}; body={}",
                truncate_for_error(body_text)
            ))
        })?;

        let streamed_text = extract_response_output_text(&value).ok_or_else(|| {
            warn!("unexpected llm response body: {}", value);
            WisprError::InvalidState(
                "LLM backend returned a response without structured output text".to_string(),
            )
        })?;

        let decision = serde_json::from_str::<SegmentDecision>(streamed_text.trim())?;
        let decision = validate_decision(request, decision)?;

        Ok(InterpreterOutput {
            decision,
            streamed_text,
        })
    }

    fn authorized_post(&self, endpoint: String) -> RequestBuilder {
        authorized_post_for(&self.client, &self.config.base_url, &self.api_key, endpoint)
    }

    pub fn start_generation_stream(
        &self,
        request: GenerationRequest,
        cancel_flag: Arc<AtomicBool>,
    ) -> mpsc::Receiver<Result<String>> {
        let config = self.config.clone();
        let api_key = self.api_key.clone();
        let (tx, rx) = mpsc::channel::<Result<String>>(32);

        tokio::spawn(async move {
            let client = match build_http_client(config.generation_timeout_ms.max(1_000)) {
                Ok(client) => client,
                Err(error) => {
                    let _ = tx.send(Err(error)).await;
                    return;
                }
            };
            let endpoint = format!("{}/responses", config.base_url.trim_end_matches('/'));
            let response = match authorized_post_for(&client, &config.base_url, &api_key, endpoint)
                .json(&build_generation_request_body(&config, &request, true))
                .send()
                .await
            {
                Ok(response) => response,
                Err(error) => {
                    let _ = tx.send(Err(WisprError::Http(error.to_string()))).await;
                    return;
                }
            };

            if let Err(error) = stream_generation_response(
                &client,
                config.clone(),
                &api_key,
                request,
                response,
                tx.clone(),
                cancel_flag.clone(),
            )
            .await
            {
                if cancel_flag.load(Ordering::Relaxed) {
                    return;
                }
                let _ = tx.send(Err(error)).await;
            }
        });

        rx
    }
}

fn build_http_client(timeout_ms: u64) -> Result<Client> {
    let timeout = Duration::from_millis(timeout_ms.max(250));
    Ok(Client::builder().timeout(timeout).build()?)
}

fn authorized_post_for(
    client: &Client,
    base_url: &str,
    api_key: &str,
    endpoint: String,
) -> RequestBuilder {
    let request = client.post(endpoint);
    if is_azure_compatible_base_url(base_url) {
        request.header("api-key", api_key)
    } else {
        request.bearer_auth(api_key)
    }
}

fn build_request_body(
    config: &IntelligenceConfig,
    request: &SegmentDecisionRequest,
    stream: bool,
) -> Value {
    json!({
        "model": config.model,
        "stream": stream,
        "input": [
            {
                "role": "developer",
                "content": [
                    {
                        "type": "input_text",
                        "text": developer_prompt()
                    }
                ]
            },
            {
                "role": "user",
                "content": [
                    {
                        "type": "input_text",
                        "text": serde_json::to_string_pretty(request).unwrap_or_else(|_| "{}".to_string())
                    }
                ]
            }
        ],
        "text": {
            "format": {
                "type": "json_schema",
                "name": "wispr_segment_decision",
                "strict": true,
                "schema": decision_schema()
            }
        }
    })
}

fn build_generation_request_body(
    config: &IntelligenceConfig,
    request: &GenerationRequest,
    stream: bool,
) -> Value {
    json!({
        "model": config.model,
        "stream": stream,
        "input": [
            {
                "role": "developer",
                "content": [
                    {
                        "type": "input_text",
                        "text": generation_prompt(request.generation_style.clone())
                    }
                ]
            },
            {
                "role": "user",
                "content": [
                    {
                        "type": "input_text",
                        "text": serde_json::to_string_pretty(request).unwrap_or_else(|_| "{}".to_string())
                    }
                ]
            }
        ]
    })
}

fn generation_prompt(style: GenerationStyle) -> &'static str {
    match style {
        GenerationStyle::Generic => {
            "You are Wispr's autonomous writing mode. Produce ready-to-paste end-user text only. Do not return JSON, markdown fences, labels, or commentary about what you are doing. Use the user's request, recent typed context, and active app context only as writing context."
        }
        GenerationStyle::PlainText => {
            "You are Wispr's autonomous writing mode. Produce ready-to-paste plain text only. Do not return JSON, markdown fences, labels, or commentary about what you are doing. Use the user's request, recent typed context, and active app context only as writing context."
        }
        GenerationStyle::Email => {
            "You are Wispr's autonomous writing mode. Produce a ready-to-paste email body in plain text unless the request explicitly asks for a subject line. Do not return JSON, markdown fences, or commentary."
        }
        GenerationStyle::Essay => {
            "You are Wispr's autonomous writing mode. Produce a ready-to-paste essay or paragraph response in plain text. Do not return JSON, markdown fences, or commentary."
        }
    }
}

fn developer_prompt() -> &'static str {
    "You are Wispr, a safe dictation command interpreter, formatter, and writing-mode detector. You receive one finalized spoken segment, recent typed context, an optional active formatting block, and optional active app context. Return strict JSON only. Keep ordinary prose literal unless structure is explicit or very strong. Prefer numbered lists for sequential speech. Use bullet lists only when the speech is clearly unordered. Spoken corrections such as 'wait', 'no', 'not X, Y instead', or 'replace the third one' should usually rewrite the current block, not append a new literal sentence. rewrite_scope='segment' means only the current finalized segment should be committed. rewrite_scope='current_block' means text_to_emit is the full replacement for the active formatting block. keep_block_open should stay true while the user is still building or correcting the same list or note block. You may emit explicit keyboard actions with modifiers Ctrl, Shift, Alt, and Super. You may also emit semantic_command actions for common app intents such as new_tab, close_tab, reopen_closed_tab, refresh, find, save, copy, paste, cut, undo, redo, focus_address_bar, next_tab, and previous_tab. Prefer semantic_command for high-level phrases like 'open a new browser tab' or 'focus the address bar'. You may return kind='generation' only for explicit autonomous-writing requests such as 'write an essay on...', 'draft an email for leave', 'compose a reply saying...', or 'generate a paragraph about...'. Do not use generation for ordinary dictation, formatting, editing commands, shell commands, or ambiguous wording. When kind='generation', set generation_prompt to the normalized user request, set generation_style to generic/plain_text/email/essay, set replace_current_segment=true, set text_to_emit to an empty string, and return no actions. If active app context is available, use it. Allowed keys include letters A-Z, digits Digit0-Digit9, Space, Enter, Tab, Escape, Backspace, Delete, Insert, Left, Right, Up, Down, Home, End, PageUp, PageDown, and F1-F12. Each action may set repeat to run the same key multiple times, such as Space repeated twice. Never invent dangerous system actions. Never launch apps, run shell commands, click, or move the mouse. If unsure, keep the speech literal. If the spoken text is ordinary prose, return kind literal, format_kind plain, rewrite_scope segment, and keep text_to_emit equal to the literal transcript. If command words should not remain in the editor, remove them from text_to_emit. Normalize command-like text when the user is dictating shell commands or flags: for example 'flutter dash dash version enter' should become text_to_emit 'flutter --version' plus Enter. Example formatting behavior: a spoken to-do list should become a numbered list; a later correction like 'wait, not housecleaning, washing of clothes' should rewrite the current list block so the corrected item replaces the old one."
}

fn decision_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["kind", "rewrite_scope", "format_kind", "text_to_emit", "keep_block_open", "actions", "generation_prompt", "generation_style", "replace_current_segment"],
        "properties": {
            "kind": {
                "type": "string",
                "enum": ["literal", "action", "literal_and_action", "generation"]
            },
            "rewrite_scope": {
                "type": "string",
                "enum": ["segment", "current_block"]
            },
            "format_kind": {
                "type": "string",
                "enum": ["plain", "numbered_list", "bullet_list"]
            },
            "text_to_emit": {
                "type": "string"
            },
            "keep_block_open": {
                "type": "boolean"
            },
            "actions": {
                "type": "array",
                "items": {
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["type", "key", "modifiers", "repeat", "command_id", "target_app"],
                    "properties": {
                        "type": {
                            "type": "string",
                            "enum": ["key", "shortcut", "semantic_command"]
                        },
                        "key": {
                            "anyOf": [
                                {
                                    "type": "string",
                                    "enum": ["Space", "Enter", "Tab", "Escape", "Backspace", "Delete", "Insert", "Left", "Right", "Up", "Down", "Home", "End", "PageUp", "PageDown", "A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M", "N", "O", "P", "Q", "R", "S", "T", "U", "V", "W", "X", "Y", "Z", "Digit0", "Digit1", "Digit2", "Digit3", "Digit4", "Digit5", "Digit6", "Digit7", "Digit8", "Digit9", "F1", "F2", "F3", "F4", "F5", "F6", "F7", "F8", "F9", "F10", "F11", "F12"]
                                },
                                {
                                    "type": "null"
                                }
                            ]
                        },
                        "modifiers": {
                            "type": "array",
                            "items": {
                                "type": "string",
                                "enum": ["Ctrl", "Shift", "Alt", "Super"]
                            }
                        },
                        "repeat": {
                            "type": "integer",
                            "minimum": 1,
                            "maximum": 12
                        },
                        "command_id": {
                            "anyOf": [
                                {
                                    "type": "string",
                                    "enum": ["new_tab", "close_tab", "reopen_closed_tab", "refresh", "find", "save", "copy", "paste", "cut", "undo", "redo", "focus_address_bar", "next_tab", "previous_tab"]
                                },
                                {
                                    "type": "null"
                                }
                            ]
                        },
                        "target_app": {
                            "anyOf": [
                                {
                                    "type": "string",
                                    "enum": ["browser", "editor", "terminal", "generic"]
                                },
                                {
                                    "type": "null"
                                }
                            ]
                        }
                    }
                }
            },
            "generation_prompt": {
                "anyOf": [
                    {
                        "type": "string"
                    },
                    {
                        "type": "null"
                    }
                ]
            },
            "generation_style": {
                "anyOf": [
                    {
                        "type": "string",
                        "enum": ["generic", "plain_text", "email", "essay"]
                    },
                    {
                        "type": "null"
                    }
                ]
            },
            "replace_current_segment": {
                "type": "boolean"
            }
        }
    })
}

async fn stream_generation_response(
    client: &Client,
    config: IntelligenceConfig,
    api_key: &str,
    request: GenerationRequest,
    response: reqwest::Response,
    tx: mpsc::Sender<Result<String>>,
    cancel_flag: Arc<AtomicBool>,
) -> Result<()> {
    let status = response.status();
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_string();

    if !content_type.contains("text/event-stream") {
        return send_generation_non_streaming(client, &config, api_key, request, tx).await;
    }

    let mut raw_stream = String::new();
    let mut fallback_text = String::new();
    let mut completion_payload = None::<Value>;
    let mut buffer = String::new();
    let mut emitted_any = false;
    let mut stream = response.bytes_stream();

    while let Some(next) = stream.next().await {
        if cancel_flag.load(Ordering::Relaxed) {
            return Ok(());
        }

        match next {
            Ok(chunk) => {
                let chunk_text = String::from_utf8_lossy(&chunk);
                raw_stream.push_str(&chunk_text);
                buffer.push_str(&chunk_text);

                while let Some(frame) = pop_sse_frame(&mut buffer) {
                    if let Some(delta) = process_generation_sse_frame(
                        &frame,
                        &mut fallback_text,
                        &mut completion_payload,
                        &raw_stream,
                    )? {
                        emitted_any = true;
                        if tx.send(Ok(delta)).await.is_err() {
                            return Ok(());
                        }
                    }
                }
            }
            Err(error) => {
                if emitted_any || !fallback_text.trim().is_empty() || completion_payload.is_some() {
                    break;
                }
                let err = WisprError::Http(format!("error decoding response body: {error}"));
                if should_retry_non_streaming(&err) {
                    return send_generation_non_streaming(client, &config, api_key, request, tx)
                        .await;
                }
                return Err(err);
            }
        }
    }

    while let Some(frame) = pop_sse_frame(&mut buffer) {
        if let Some(delta) = process_generation_sse_frame(
            &frame,
            &mut fallback_text,
            &mut completion_payload,
            &raw_stream,
        )? {
            emitted_any = true;
            if tx.send(Ok(delta)).await.is_err() {
                return Ok(());
            }
        }
    }

    if cancel_flag.load(Ordering::Relaxed) {
        return Ok(());
    }

    if !status.is_success() && !emitted_any && fallback_text.trim().is_empty() {
        return Err(WisprError::Http(format!(
            "HTTP {} from LLM backend with empty generation stream",
            status
        )));
    }

    if !emitted_any {
        if let Some(event) = completion_payload {
            if let Some(text) = extract_completed_output_text(&event) {
                if !text.is_empty() {
                    let _ = tx.send(Ok(text)).await;
                    return Ok(());
                }
            }
        }
        if !fallback_text.is_empty() {
            let _ = tx.send(Ok(fallback_text)).await;
            return Ok(());
        }
        return send_generation_non_streaming(client, &config, api_key, request, tx).await;
    }

    Ok(())
}

async fn send_generation_non_streaming(
    client: &Client,
    config: &IntelligenceConfig,
    api_key: &str,
    request: GenerationRequest,
    tx: mpsc::Sender<Result<String>>,
) -> Result<()> {
    let endpoint = format!("{}/responses", config.base_url.trim_end_matches('/'));
    let response = authorized_post_for(client, &config.base_url, api_key, endpoint)
        .json(&build_generation_request_body(config, &request, false))
        .send()
        .await?;
    let status = response.status();
    let body_text = response
        .text()
        .await
        .map_err(|error| WisprError::Http(format!("error decoding response body: {error}")))?;

    if !status.is_success() {
        return Err(WisprError::Http(format!(
            "HTTP {} from LLM backend: {}",
            status,
            truncate_for_error(&body_text)
        )));
    }

    let value: Value = serde_json::from_str(&body_text).map_err(|error| {
        WisprError::Http(format!(
            "failed to parse non-streaming generation response body: {error}; body={}",
            truncate_for_error(&body_text)
        ))
    })?;
    let generated = extract_response_output_text(&value).ok_or_else(|| {
        WisprError::InvalidState(
            "LLM backend returned a generation response without output text".to_string(),
        )
    })?;
    if tx.send(Ok(generated)).await.is_err() {
        return Ok(());
    }
    Ok(())
}

fn pop_sse_frame(buffer: &mut String) -> Option<String> {
    if let Some(index) = buffer.find("\r\n\r\n") {
        let frame = buffer[..index].to_string();
        buffer.drain(..index + 4);
        return Some(frame);
    }

    if let Some(index) = buffer.find("\n\n") {
        let frame = buffer[..index].to_string();
        buffer.drain(..index + 2);
        return Some(frame);
    }

    None
}

fn extract_sse_data(frame: &str) -> Option<String> {
    let mut data_lines = Vec::new();
    for line in frame.lines() {
        if let Some(rest) = line.strip_prefix("data:") {
            data_lines.push(rest.trim_start().to_string());
        }
    }

    if data_lines.is_empty() {
        None
    } else {
        Some(data_lines.join("\n"))
    }
}

fn process_sse_frame(
    frame: &str,
    streamed_text: &mut String,
    completion_payload: &mut Option<Value>,
    raw_stream: &str,
) -> Result<bool> {
    let Some(data) = extract_sse_data(frame) else {
        return Ok(false);
    };
    if data.trim() == "[DONE]" {
        return Ok(false);
    }

    let event: Value = serde_json::from_str(&data).map_err(|error| {
        WisprError::Http(format!(
            "failed to parse streamed response event: {error}; body={}",
            truncate_for_error(raw_stream)
        ))
    })?;

    match event.get("type").and_then(Value::as_str) {
        Some("response.output_text.delta") => {
            if let Some(delta) = event.get("delta").and_then(Value::as_str) {
                streamed_text.push_str(delta);
                return Ok(true);
            }
        }
        Some("response.output_text.done") => {
            if streamed_text.is_empty() {
                if let Some(text) = event.get("text").and_then(Value::as_str) {
                    streamed_text.push_str(text);
                }
            }
            return Ok(!streamed_text.is_empty());
        }
        Some("response.completed") => {
            *completion_payload = Some(event);
            return Ok(true);
        }
        Some("error") | Some("response.failed") => {
            return Err(WisprError::Http(
                event
                    .get("message")
                    .and_then(Value::as_str)
                    .or_else(|| {
                        event
                            .get("error")
                            .and_then(|value| value.get("message"))
                            .and_then(Value::as_str)
                    })
                    .unwrap_or("LLM request failed")
                    .to_string(),
            ));
        }
        _ => {}
    }

    Ok(false)
}

fn process_generation_sse_frame(
    frame: &str,
    fallback_text: &mut String,
    completion_payload: &mut Option<Value>,
    raw_stream: &str,
) -> Result<Option<String>> {
    let Some(data) = extract_sse_data(frame) else {
        return Ok(None);
    };
    if data.trim() == "[DONE]" {
        return Ok(None);
    }

    let event: Value = serde_json::from_str(&data).map_err(|error| {
        WisprError::Http(format!(
            "failed to parse streamed generation event: {error}; body={}",
            truncate_for_error(raw_stream)
        ))
    })?;

    match event.get("type").and_then(Value::as_str) {
        Some("response.output_text.delta") => {
            if let Some(delta) = event.get("delta").and_then(Value::as_str) {
                fallback_text.push_str(delta);
                return Ok(Some(delta.to_string()));
            }
        }
        Some("response.output_text.done") => {
            if fallback_text.is_empty() {
                if let Some(text) = event.get("text").and_then(Value::as_str) {
                    fallback_text.push_str(text);
                    return Ok(Some(text.to_string()));
                }
            }
        }
        Some("response.completed") => {
            *completion_payload = Some(event);
        }
        Some("error") | Some("response.failed") => {
            return Err(WisprError::Http(
                event
                    .get("message")
                    .and_then(Value::as_str)
                    .or_else(|| {
                        event
                            .get("error")
                            .and_then(|value| value.get("message"))
                            .and_then(Value::as_str)
                    })
                    .unwrap_or("LLM generation request failed")
                    .to_string(),
            ));
        }
        _ => {}
    }

    Ok(None)
}

fn extract_completed_output_text(event: &Value) -> Option<String> {
    event
        .get("response")
        .and_then(|response| response.get("output"))
        .and_then(Value::as_array)
        .and_then(|items| items.first())
        .and_then(|item| item.get("content"))
        .and_then(Value::as_array)
        .and_then(|contents| contents.first())
        .and_then(|content| content.get("text"))
        .and_then(Value::as_str)
        .map(ToString::to_string)
}

fn extract_response_output_text(value: &Value) -> Option<String> {
    value
        .get("output")
        .and_then(Value::as_array)
        .and_then(|items| items.first())
        .and_then(|item| item.get("content"))
        .and_then(Value::as_array)
        .and_then(|contents| contents.first())
        .and_then(|content| content.get("text"))
        .and_then(Value::as_str)
        .map(ToString::to_string)
        .or_else(|| {
            value
                .get("output_text")
                .and_then(Value::as_str)
                .map(ToString::to_string)
        })
}

fn truncate_for_error(body: &str) -> String {
    const MAX_LEN: usize = 400;
    if body.len() <= MAX_LEN {
        body.to_string()
    } else {
        format!("{}...", &body[..MAX_LEN])
    }
}

fn is_azure_compatible_base_url(base_url: &str) -> bool {
    let normalized = base_url.to_ascii_lowercase();
    normalized.contains(".cognitiveservices.azure.com")
        || normalized.contains(".openai.azure.com")
        || normalized.contains("azure.com/openai/")
}

fn should_retry_non_streaming(error: &WisprError) -> bool {
    match error {
        WisprError::Http(message) => {
            message.contains("error decoding response body")
                || message.contains("returned no structured output")
                || message.contains("failed to parse streamed response event")
                || message.contains("failed to parse streamed generation event")
        }
        WisprError::InvalidState(message) => message.contains("returned no structured output"),
        WisprError::Json(_) => true,
        _ => false,
    }
}

fn validate_decision(
    request: &SegmentDecisionRequest,
    mut decision: SegmentDecision,
) -> Result<SegmentDecision> {
    if request.text_output_mode == TextOutputMode::Literal
        && decision.kind == DecisionKind::Literal
        && decision.text_to_emit.trim().is_empty()
    {
        return Ok(SegmentDecision::literal(request.literal_text.clone()));
    }

    if decision.kind == DecisionKind::Generation {
        return validate_generation_decision(request, decision);
    }

    hydrate_missing_actions(request, &mut decision);
    let normalized_text = normalize_command_text(&decision.text_to_emit);

    for action in &decision.actions {
        validate_action(action, &request.action_scope)?;
    }
    decision.actions = dedupe_actions(decision.actions);

    if decision.kind == DecisionKind::Literal && decision.actions.is_empty() {
        if let Some((text_to_emit, key)) = infer_literal_and_key_action(&request.finalized_text) {
            let normalized_text = normalize_command_text(&text_to_emit);
            return Ok(SegmentDecision {
                kind: if normalized_text.is_empty() {
                    DecisionKind::Action
                } else {
                    DecisionKind::LiteralAndAction
                },
                rewrite_scope: crate::models::RewriteScope::Segment,
                format_kind: crate::models::FormatKind::Plain,
                text_to_emit: normalized_text,
                keep_block_open: false,
                actions: vec![ActionCommand {
                    action_type: ActionType::Key,
                    key: Some(key),
                    modifiers: Vec::new(),
                    repeat: 1,
                    command_id: None,
                    target_app: request.active_app.as_ref().map(|app| app.app_class.clone()),
                }],
                generation_prompt: None,
                generation_style: None,
                replace_current_segment: false,
            });
        }
    }

    if !decision.actions.is_empty() {
        let mut action_text = normalized_text;
        if let Some((stripped_text, inferred_key)) =
            infer_literal_and_key_action(&request.finalized_text)
        {
            if decision.actions.iter().any(|action| {
                action.action_type == ActionType::Key
                    && action.key == Some(inferred_key.clone())
                    && action.modifiers.is_empty()
            }) {
                action_text = normalize_command_text(&stripped_text);
            }
        }
        return Ok(SegmentDecision {
            kind: if action_text.is_empty() {
                DecisionKind::Action
            } else {
                DecisionKind::LiteralAndAction
            },
            rewrite_scope: decision.rewrite_scope,
            format_kind: decision.format_kind,
            text_to_emit: action_text,
            keep_block_open: decision.keep_block_open,
            actions: decision.actions,
            generation_prompt: None,
            generation_style: None,
            replace_current_segment: false,
        });
    }

    Ok(SegmentDecision {
        generation_prompt: None,
        generation_style: None,
        replace_current_segment: false,
        text_to_emit: normalized_text,
        ..decision
    })
}

fn dedupe_actions(actions: Vec<ActionCommand>) -> Vec<ActionCommand> {
    let mut deduped = Vec::new();
    for action in actions {
        if !deduped
            .iter()
            .any(|existing| actions_match(existing, &action))
        {
            deduped.push(action);
        }
    }
    deduped
}

fn actions_match(left: &ActionCommand, right: &ActionCommand) -> bool {
    left.action_type == right.action_type
        && left.key == right.key
        && left.modifiers == right.modifiers
        && left.repeat == right.repeat
        && left.command_id == right.command_id
}

fn validate_generation_decision(
    _request: &SegmentDecisionRequest,
    decision: SegmentDecision,
) -> Result<SegmentDecision> {
    let generation_prompt = decision
        .generation_prompt
        .as_ref()
        .map(|value| value.trim())
        .unwrap_or_default();
    if generation_prompt.is_empty() {
        return Err(WisprError::InvalidState(
            "generation decision is missing generation_prompt".to_string(),
        ));
    }
    if !decision.replace_current_segment {
        return Err(WisprError::InvalidState(
            "generation decision must replace the current segment".to_string(),
        ));
    }
    if !decision.actions.is_empty() {
        return Err(WisprError::InvalidState(
            "generation decision cannot include actions".to_string(),
        ));
    }
    if decision.rewrite_scope != crate::models::RewriteScope::Segment {
        return Err(WisprError::InvalidState(
            "generation decision must use rewrite_scope=segment".to_string(),
        ));
    }
    if !decision.text_to_emit.trim().is_empty() {
        warn!(
            "generation decision returned text_to_emit={}, ignoring it",
            decision.text_to_emit
        );
    }

    Ok(SegmentDecision {
        kind: DecisionKind::Generation,
        rewrite_scope: crate::models::RewriteScope::Segment,
        format_kind: crate::models::FormatKind::Plain,
        text_to_emit: String::new(),
        keep_block_open: false,
        actions: Vec::new(),
        generation_prompt: Some(generation_prompt.to_string()),
        generation_style: Some(
            decision
                .generation_style
                .unwrap_or(GenerationStyle::Generic),
        ),
        replace_current_segment: true,
    })
}

fn hydrate_missing_actions(request: &SegmentDecisionRequest, decision: &mut SegmentDecision) {
    let inferred_key = infer_key_from_spoken_text(&request.finalized_text);
    for action in &mut decision.actions {
        if action.action_type == ActionType::Key
            && action.key.is_none()
            && action.command_id.is_none()
        {
            action.key = inferred_key.clone();
        }
    }
}

fn infer_key_from_spoken_text(text: &str) -> Option<crate::models::ActionKey> {
    let normalized = text.trim().to_ascii_lowercase();
    let tokens = normalized.split_whitespace().collect::<Vec<_>>();

    let last = tokens.last().copied().unwrap_or_default();
    match last {
        "enter" | "return" => Some(crate::models::ActionKey::Enter),
        "tab" => Some(crate::models::ActionKey::Tab),
        "escape" | "esc" => Some(crate::models::ActionKey::Escape),
        "backspace" => Some(crate::models::ActionKey::Backspace),
        "delete" => Some(crate::models::ActionKey::Delete),
        "space" => Some(crate::models::ActionKey::Space),
        "left" => Some(crate::models::ActionKey::Left),
        "right" => Some(crate::models::ActionKey::Right),
        "up" => Some(crate::models::ActionKey::Up),
        "down" => Some(crate::models::ActionKey::Down),
        "home" => Some(crate::models::ActionKey::Home),
        "end" => Some(crate::models::ActionKey::End),
        _ => infer_function_key(last),
    }
}

fn infer_literal_and_key_action(text: &str) -> Option<(String, crate::models::ActionKey)> {
    let tokens = text.split_whitespace().collect::<Vec<_>>();
    let last = tokens.last().copied()?;
    let key = infer_key_from_spoken_text(last)?;
    let stripped = tokens[..tokens.len().saturating_sub(1)].join(" ");
    Some((stripped, key))
}

fn infer_function_key(token: &str) -> Option<crate::models::ActionKey> {
    match token {
        "f1" => Some(crate::models::ActionKey::F1),
        "f2" => Some(crate::models::ActionKey::F2),
        "f3" => Some(crate::models::ActionKey::F3),
        "f4" => Some(crate::models::ActionKey::F4),
        "f5" => Some(crate::models::ActionKey::F5),
        "f6" => Some(crate::models::ActionKey::F6),
        "f7" => Some(crate::models::ActionKey::F7),
        "f8" => Some(crate::models::ActionKey::F8),
        "f9" => Some(crate::models::ActionKey::F9),
        "f10" => Some(crate::models::ActionKey::F10),
        "f11" => Some(crate::models::ActionKey::F11),
        "f12" => Some(crate::models::ActionKey::F12),
        _ => None,
    }
}

fn validate_action(action: &ActionCommand, scope: &ActionScope) -> Result<()> {
    match scope {
        ActionScope::EditingOnly => validate_editing_only_action(action),
    }
}

fn validate_editing_only_action(action: &ActionCommand) -> Result<()> {
    let modifiers = &action.modifiers;
    let ctrl = modifiers
        .iter()
        .filter(|modifier| **modifier == ModifierKey::Ctrl)
        .count();
    let shift = modifiers
        .iter()
        .filter(|modifier| **modifier == ModifierKey::Shift)
        .count();
    let repeat = action.repeat.max(1);

    if ctrl > 1 || shift > 1 {
        return Err(WisprError::InvalidState(
            "duplicate modifiers are not allowed".to_string(),
        ));
    }

    if !(1..=12).contains(&repeat) {
        return Err(WisprError::InvalidState(format!(
            "repeat count {} is outside the supported range",
            action.repeat
        )));
    }

    match action.action_type {
        ActionType::Key => {
            if action.key.is_none() {
                return Err(WisprError::InvalidState(
                    "key action is missing a primary key".to_string(),
                ));
            }
            if action.command_id.is_some() {
                return Err(WisprError::InvalidState(
                    "key action cannot include a semantic command id".to_string(),
                ));
            }
            if !modifiers.is_empty() {
                return Err(WisprError::InvalidState(format!(
                    "key action {} cannot carry modifiers",
                    describe_action(action)
                )));
            }
        }
        ActionType::Shortcut => {
            if action.key.is_none() {
                return Err(WisprError::InvalidState(
                    "shortcut action is missing a primary key".to_string(),
                ));
            }
            if action.command_id.is_some() {
                return Err(WisprError::InvalidState(
                    "shortcut action cannot include a semantic command id".to_string(),
                ));
            }
            if modifiers.is_empty() {
                return Err(WisprError::InvalidState(format!(
                    "shortcut {} requires at least one modifier",
                    describe_action(action)
                )));
            }
        }
        ActionType::SemanticCommand => {
            if action.command_id.is_none() {
                return Err(WisprError::InvalidState(
                    "semantic command action is missing command_id".to_string(),
                ));
            }
            if action.key.is_some() {
                return Err(WisprError::InvalidState(
                    "semantic command action cannot include a raw key".to_string(),
                ));
            }
            if !modifiers.is_empty() {
                return Err(WisprError::InvalidState(
                    "semantic command action cannot include modifiers".to_string(),
                ));
            }
        }
    }

    Ok(())
}

fn describe_action(action: &ActionCommand) -> String {
    if let Some(command_id) = &action.command_id {
        let target = action
            .target_app
            .as_ref()
            .map(|target| format!(" for {:?}", target))
            .unwrap_or_default();
        let repeat = if action.repeat > 1 {
            format!(" x{}", action.repeat)
        } else {
            String::new()
        };
        return format!("{:?}{target}{repeat}", command_id);
    }

    let repeat = if action.repeat > 1 {
        format!(" x{}", action.repeat)
    } else {
        String::new()
    };

    if action.modifiers.is_empty() {
        format!("{:?}{repeat}", action.key)
    } else {
        let modifiers = action
            .modifiers
            .iter()
            .map(ModifierKey::as_label)
            .collect::<Vec<_>>()
            .join("+");
        format!("{modifiers}+{:?}{repeat}", action.key)
    }
}

fn normalize_command_text(input: &str) -> String {
    let mut text = input.trim().to_string();
    if text.is_empty() {
        return text;
    }

    text = normalize_symbol_words(&text);

    let replacements = [("-- ", "--"), (" . ", "."), (" / ", "/"), (" :: ", "::")];
    for (from, to) in replacements {
        while text.contains(from) {
            text = text.replace(from, to);
        }
    }

    text = attach_flag_after_double_dash(&text);
    text
}

fn normalize_symbol_words(input: &str) -> String {
    let words = input.split_whitespace().collect::<Vec<_>>();
    let mut parts = Vec::new();
    let mut index = 0usize;

    while index < words.len() {
        let current = words[index].to_ascii_lowercase();
        let next = words.get(index + 1).map(|word| word.to_ascii_lowercase());

        if matches!(current.as_str(), "dash" | "hyphen" | "minus" | "-")
            && matches!(next.as_deref(), Some("dash" | "hyphen" | "minus" | "-"))
        {
            parts.push("--".to_string());
            index += 2;
            continue;
        }

        parts.push(words[index].to_string());
        index += 1;
    }

    parts.join(" ")
}

fn attach_flag_after_double_dash(input: &str) -> String {
    let mut words = input.split_whitespace().peekable();
    let mut parts = Vec::new();

    while let Some(word) = words.next() {
        if word == "--" {
            if let Some(next) = words.peek() {
                if next
                    .chars()
                    .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
                {
                    parts.push(format!("--{}", words.next().unwrap_or_default()));
                    continue;
                }
            }
        }

        parts.push(word.to_string());
    }

    parts.join(" ")
}
