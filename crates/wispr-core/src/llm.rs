use std::time::Duration;

use futures_util::StreamExt;
use reqwest::{Client, RequestBuilder};
use serde_json::{Value, json};
use tracing::warn;

use crate::{
    config::IntelligenceConfig,
    error::{Result, WisprError},
    models::{
        ActionCommand, ActionKey, ActionScope, ActionType, CommandMode, DecisionKind, ModifierKey,
        SegmentDecision, SegmentDecisionRequest, TextOutputMode,
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
        let timeout = Duration::from_millis(config.timeout_ms.max(250));
        let client = Client::builder().timeout(timeout).build()?;
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
            action_scope: self.config.action_scope.clone(),
            command_mode: CommandMode::AlwaysInfer,
            text_output_mode: TextOutputMode::Literal,
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
        let request = self.client.post(endpoint);
        if is_azure_compatible_base_url(&self.config.base_url) {
            request.header("api-key", &self.api_key)
        } else {
            request.bearer_auth(&self.api_key)
        }
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

fn developer_prompt() -> &'static str {
    "You are Wispr, a safe dictation command interpreter. You receive one finalized spoken segment plus recent typed context. Return strict JSON only. Keep normal dictation literal. Only convert clearly spoken editing commands into allowed keyboard actions. Never invent actions. Never launch apps, run shell commands, click, move the mouse, or use unsupported shortcuts. Allowed special keys: Space, Enter, Tab, Escape, Backspace, Delete, Left, Right, Up, Down, Home, End, F1, F2, F3, F4, F5, F6, F7, F8, F9, F10, F11, F12. Allowed shortcuts: Ctrl+A, Ctrl+C, Ctrl+V, Ctrl+X, Ctrl+Z, Ctrl+Shift+Z. Each action may set repeat to run the same key multiple times, such as Space repeated twice. If the spoken text is ordinary prose, return kind literal and keep text_to_emit equal to the literal transcript. If command words should not remain in the editor, remove them from text_to_emit. Normalize command-like text when the user is dictating shell commands or flags: for example 'flutter dash dash version enter' or 'flutter hyphen hyphen version enter' should become text_to_emit 'flutter --version' plus Enter. 'press space key twice' should produce a Space key action with repeat 2. 'press the F5 key' should produce an F5 key action."
}

fn decision_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["kind", "text_to_emit", "actions"],
        "properties": {
            "kind": {
                "type": "string",
                "enum": ["literal", "action", "literal_and_action"]
            },
            "text_to_emit": {
                "type": "string"
            },
            "actions": {
                "type": "array",
                "items": {
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["type", "key", "modifiers", "repeat"],
                    "properties": {
                        "type": {
                            "type": "string",
                            "enum": ["key", "shortcut"]
                        },
                        "key": {
                            "type": "string",
                            "enum": ["Space", "Enter", "Tab", "Escape", "Backspace", "Delete", "Left", "Right", "Up", "Down", "Home", "End", "A", "C", "V", "X", "Z", "F1", "F2", "F3", "F4", "F5", "F6", "F7", "F8", "F9", "F10", "F11", "F12"]
                        },
                        "modifiers": {
                            "type": "array",
                            "items": {
                                "type": "string",
                                "enum": ["Ctrl", "Shift"]
                            }
                        },
                        "repeat": {
                            "type": "integer",
                            "minimum": 1,
                            "maximum": 12
                        }
                    }
                }
            }
        }
    })
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
        }
        WisprError::InvalidState(message) => message.contains("returned no structured output"),
        _ => false,
    }
}

fn validate_decision(
    request: &SegmentDecisionRequest,
    decision: SegmentDecision,
) -> Result<SegmentDecision> {
    if request.text_output_mode == TextOutputMode::Literal
        && decision.kind == DecisionKind::Literal
        && decision.text_to_emit.trim().is_empty()
    {
        return Ok(SegmentDecision::literal(request.literal_text.clone()));
    }

    let normalized_text = normalize_command_text(&decision.text_to_emit);

    for action in &decision.actions {
        validate_action(action, &request.action_scope)?;
    }

    if decision.kind == DecisionKind::Action && !decision.text_to_emit.is_empty() {
        return Ok(SegmentDecision {
            kind: DecisionKind::LiteralAndAction,
            text_to_emit: normalized_text,
            actions: decision.actions,
        });
    }

    Ok(SegmentDecision {
        text_to_emit: normalized_text,
        ..decision
    })
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

    match action.key {
        ActionKey::Space
        | ActionKey::Enter
        | ActionKey::Tab
        | ActionKey::Escape
        | ActionKey::Backspace
        | ActionKey::Delete
        | ActionKey::Left
        | ActionKey::Right
        | ActionKey::Up
        | ActionKey::Down
        | ActionKey::Home
        | ActionKey::End
        | ActionKey::F1
        | ActionKey::F2
        | ActionKey::F3
        | ActionKey::F4
        | ActionKey::F5
        | ActionKey::F6
        | ActionKey::F7
        | ActionKey::F8
        | ActionKey::F9
        | ActionKey::F10
        | ActionKey::F11
        | ActionKey::F12 => {
            if !modifiers.is_empty() {
                return Err(WisprError::InvalidState(format!(
                    "key action {} does not allow modifiers",
                    describe_action(action)
                )));
            }
        }
        ActionKey::A | ActionKey::C | ActionKey::V | ActionKey::X => {
            if modifiers != &[ModifierKey::Ctrl] {
                return Err(WisprError::InvalidState(format!(
                    "shortcut {} must use Ctrl only",
                    describe_action(action)
                )));
            }
        }
        ActionKey::Z => {
            if modifiers != &[ModifierKey::Ctrl]
                && modifiers != &[ModifierKey::Ctrl, ModifierKey::Shift]
            {
                return Err(WisprError::InvalidState(format!(
                    "shortcut {} must use Ctrl or Ctrl+Shift",
                    describe_action(action)
                )));
            }
        }
    }

    match action.action_type {
        ActionType::Key => {
            if !modifiers.is_empty() {
                return Err(WisprError::InvalidState(format!(
                    "key action {} cannot carry modifiers",
                    describe_action(action)
                )));
            }
        }
        ActionType::Shortcut => {
            if modifiers.is_empty() {
                return Err(WisprError::InvalidState(format!(
                    "shortcut {} requires at least one modifier",
                    describe_action(action)
                )));
            }
        }
    }

    Ok(())
}

fn describe_action(action: &ActionCommand) -> String {
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
