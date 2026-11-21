use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::ipc::Channel;
use tokio::sync::Notify;

use crate::ollama::{ChatMessage, StreamEvent};

const GROQ_COMPLETIONS_URL: &str = "https://api.groq.com/openai/v1/chat/completions";
const GROQ_MODELS_URL: &str = "https://api.groq.com/openai/v1/models";
const GROQ_MODELS_TIMEOUT: Duration = Duration::from_secs(15);
const GROQ_CONNECT_SEND_TIMEOUT: Duration = Duration::from_secs(20);
const GROQ_STREAM_IDLE_TIMEOUT: Duration = Duration::from_secs(45);

#[derive(Debug, Deserialize, Serialize)]
pub struct GroqModelItem {
    pub id: String,
    pub object: Option<String>,
    pub created: Option<u64>,
    pub owned_by: Option<String>,
    pub active: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct GroqModelsListResponse {
    data: Option<Vec<GroqModelItem>>,
}

/// Queries Groq API dynamically for the currently available active models
pub async fn fetch_active_groq_models(
    client: &reqwest::Client,
    api_key: &str,
) -> Result<Vec<GroqModelItem>, String> {
    let clean_key = api_key.trim();
    if clean_key.is_empty() {
        return Err("AUTH_ERROR: No Groq API key configured.".to_string());
    }

    let response = client
        .get(GROQ_MODELS_URL)
        .timeout(GROQ_MODELS_TIMEOUT)
        .header("Authorization", format!("Bearer {clean_key}"))
        .header("Content-Type", "application/json")
        .send()
        .await
        .map_err(|e| format!("Failed to reach Groq models API: {e}"))?;

    if !response.status().is_success() {
        let status = response.status();
        let body: String = response
            .text()
            .await
            .unwrap_or_default()
            .chars()
            .take(300)
            .collect();
        return Err(format!("Groq models query returned status {status}: {body}"));
    }

    let list_response: GroqModelsListResponse = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse Groq models JSON: {e}"))?;

    let raw_models = list_response.data.unwrap_or_default();

    // Filter strictly to chat-capable active flagship models (discard audio, whisper, orpheus, safety guardrails, niche language models)
    let mut chat_models: Vec<GroqModelItem> = raw_models
        .into_iter()
        .filter(|m| {
            if m.active == Some(false) {
                return false;
            }
            let id = m.id.to_lowercase();
            !id.contains("whisper")
                && !id.contains("orpheus")
                && !id.contains("guard")
                && !id.contains("safeguard")
                && !id.contains("embed")
                && !id.contains("allam")
        })
        .collect();

    // Sort with flagship frontier models first
    chat_models.sort_by(|a, b| {
        let rank = |id: &str| -> i32 {
            let lower = id.to_lowercase();
            if lower.contains("120b") {
                1
            } else if lower.contains("70b") {
                2
            } else if lower.contains("20b") {
                3
            } else if lower.contains("qwen") {
                4
            } else {
                5
            }
        };
        rank(&a.id).cmp(&rank(&b.id)).then_with(|| a.id.cmp(&b.id))
    });

    Ok(chat_models)
}

#[derive(Debug, Serialize)]
struct GroqChatRequest<'a> {
    model: &'a str,
    messages: &'a [ChatMessage],
    stream: bool,
}

#[derive(Debug, Deserialize)]
struct GroqDelta {
    content: Option<String>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct GroqChoice {
    delta: Option<GroqDelta>,
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct GroqUsage {
    prompt_tokens: Option<u32>,
    completion_tokens: Option<u32>,
    total_tokens: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct GroqChunk {
    choices: Option<Vec<GroqChoice>>,
    usage: Option<GroqUsage>,
}

#[derive(Debug, Deserialize)]
struct GroqErrorResponse {
    error: Option<GroqErrorDetail>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct GroqErrorDetail {
    message: Option<String>,
    #[serde(rename = "type")]
    error_type: Option<String>,
    code: Option<String>,
}

/// Normalizes model identifier by removing provider prefix if present
/// e.g. "groq:llama-3.3-70b-versatile" -> "llama-3.3-70b-versatile"
pub fn normalize_groq_model_name(raw_model: &str) -> &str {
    raw_model.strip_prefix("groq:").unwrap_or(raw_model)
}

/// Checks whether a given model identifier targets a cloud provider
pub fn is_cloud_model(model: &str) -> bool {
    model.starts_with("groq:")
}

/// Streams chat completions from the Groq OpenAI-compatible API
pub async fn stream_groq_chat(
    client: &reqwest::Client,
    api_key: &str,
    raw_model: &str,
    messages: &[ChatMessage],
    on_event: &Channel<StreamEvent>,
    notify: Arc<Notify>,
) -> Result<(), String> {
    let clean_key = api_key.trim();
    if clean_key.is_empty() {
        return Err("AUTH_ERROR: No Groq API key found. Please open Settings -> Cloud Providers to paste your free Groq API key.".to_string());
    }

    let actual_model = normalize_groq_model_name(raw_model);
    let request_body = GroqChatRequest {
        model: actual_model,
        messages,
        stream: true,
    };

    let start_time = Instant::now();

    // Send HTTP POST request with cancellation guard & transient retry
    let mut attempt = 0;
    let response = loop {
        attempt += 1;
        let req = tokio::time::timeout(
            GROQ_CONNECT_SEND_TIMEOUT,
            client
                .post(GROQ_COMPLETIONS_URL)
                .header("Authorization", format!("Bearer {clean_key}"))
                .header("Content-Type", "application/json")
                .json(&request_body)
                .send(),
        );

        let res = tokio::select! {
            _ = notify.notified() => return Err("ABORTED".to_string()),
            r = req => r,
        };

        match res {
            Ok(Ok(resp)) => break resp,
            Ok(Err(_e)) if attempt < 2 => {
                tokio::time::sleep(Duration::from_millis(500)).await;
                continue;
            }
            Err(_) if attempt < 2 => {
                tokio::time::sleep(Duration::from_millis(500)).await;
                continue;
            }
            Ok(Err(e)) => {
                return Err(format!("Failed to connect to Groq cloud API: {e}"));
            }
            Err(_) => {
                return Err("NETWORK_ERROR: Groq cloud API request timed out.".to_string());
            }
        }
    };

    let status = response.status();
    if !status.is_success() {
        let err_text = response.text().await.unwrap_or_default();
        let parsed_err: Option<GroqErrorResponse> = serde_json::from_str(&err_text).ok();
        let detail_msg = parsed_err
            .and_then(|e| e.error)
            .and_then(|ed| ed.message)
            .unwrap_or_else(|| err_text.chars().take(200).collect());

        if status.as_u16() == 429 {
            return Err(format!(
                "RATE_LIMIT: Groq free tier limit reached (429). {detail_msg}"
            ));
        } else if status.as_u16() == 401 {
            return Err(format!(
                "AUTH_ERROR: Invalid Groq API key (401). {detail_msg}"
            ));
        } else {
            return Err(format!("Groq API error ({status}): {detail_msg}"));
        }
    }

    let mut stream = response.bytes_stream();
    let mut buffer: Vec<u8> = Vec::new();
    let mut generated_tokens: u32 = 0;
    let mut prompt_tokens_reported: Option<u32> = None;
    let mut completion_tokens_reported: Option<u32> = None;

    loop {
        let next_chunk = tokio::select! {
            _ = notify.notified() => return Err("ABORTED".to_string()),
            chunk = tokio::time::timeout(GROQ_STREAM_IDLE_TIMEOUT, stream.next()) => match chunk {
                Ok(item) => item,
                Err(_) => return Err("NETWORK_ERROR: Stream timed out waiting for Groq cloud response.".to_string()),
            },
        };

        match next_chunk {
            Some(Ok(bytes)) => {
                buffer.extend_from_slice(&bytes);

                while let Some(pos) = buffer.iter().position(|&b| b == b'\n') {
                    let line_bytes: Vec<u8> = buffer.drain(..=pos).collect();
                    let line = String::from_utf8_lossy(&line_bytes[..pos]);
                    let trimmed = line.trim();

                    if trimmed.is_empty() || trimmed.starts_with(':') {
                        continue;
                    }

                    if let Some(payload) = trimmed.strip_prefix("data:") {
                        let payload_trimmed = payload.trim();
                        if payload_trimmed == "[DONE]" {
                            break;
                        }

                        if let Ok(chunk) = serde_json::from_str::<GroqChunk>(payload_trimmed) {
                            if let Some(usage) = chunk.usage {
                                prompt_tokens_reported = usage.prompt_tokens;
                                completion_tokens_reported = usage.completion_tokens;
                            }

                            if let Some(choices) = chunk.choices {
                                for choice in choices {
                                    if let Some(delta) = choice.delta {
                                        if let Some(content) = delta.content {
                                            if !content.is_empty() {
                                                generated_tokens += 1;
                                                on_event
                                                    .send(StreamEvent::Token { content })
                                                    .map_err(|e| format!("Failed to send token to UI: {e}"))?;
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            Some(Err(e)) => {
                return Err(format!("Stream interrupted from Groq API: {e}"));
            }
            None => {
                if !buffer.is_empty() {
                    let trailing = String::from_utf8_lossy(&buffer);
                    let trimmed = trailing.trim();
                    if let Some(payload) = trimmed.strip_prefix("data:") {
                        let payload_trimmed = payload.trim();
                        if payload_trimmed != "[DONE]" {
                            if let Ok(chunk) = serde_json::from_str::<GroqChunk>(payload_trimmed) {
                                if let Some(usage) = chunk.usage {
                                    prompt_tokens_reported = usage.prompt_tokens;
                                    completion_tokens_reported = usage.completion_tokens;
                                }
                                if let Some(choices) = chunk.choices {
                                    for choice in choices {
                                        if let Some(content) = choice.delta.and_then(|d| d.content) {
                                            if !content.is_empty() {
                                                generated_tokens += 1;
                                                let _ = on_event.send(StreamEvent::Token { content });
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                break;
            }
        }
    }

    let elapsed = start_time.elapsed();
    let elapsed_nanos: u64 = elapsed.as_nanos().try_into().unwrap_or(u64::MAX);
    let final_completion_count = completion_tokens_reported.unwrap_or(generated_tokens);

    // Send Done event to match Ollama telemetry format
    let _ = on_event.send(StreamEvent::Done {
        total_duration: Some(elapsed_nanos),
        load_duration: Some(0),
        prompt_eval_count: prompt_tokens_reported,
        eval_count: Some(final_completion_count),
        eval_duration: Some(elapsed_nanos),
    });

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_groq_model_name() {
        assert_eq!(
            normalize_groq_model_name("groq:llama-3.3-70b-versatile"),
            "llama-3.3-70b-versatile"
        );
        assert_eq!(
            normalize_groq_model_name("llama-3.3-70b-versatile"),
            "llama-3.3-70b-versatile"
        );
    }

    #[test]
    fn test_is_cloud_model() {
        assert!(is_cloud_model("groq:llama-3.3-70b-versatile"));
        assert!(is_cloud_model("groq:deepseek-r1-distill-llama-70b"));
        assert!(!is_cloud_model("qwen2.5-coder:7b"));
        assert!(!is_cloud_model("llama3.2:latest"));
    }
}
