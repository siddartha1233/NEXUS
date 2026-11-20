use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;
use tauri::ipc::Channel;
use tauri::State;
use tokio::sync::{Mutex, Notify};

const OLLAMA_BASE_URL: &str = "http://localhost:11434";

pub struct OllamaState {
    pub client: reqwest::Client,
    pub cancel_notify: Mutex<Option<Arc<Notify>>>,
}

impl OllamaState {
    pub fn new() -> Self {
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(5))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        Self {
            client,
            cancel_notify: Mutex::new(None),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct OllamaModel {
    pub name: String,
    pub size: u64,
    pub modified_at: String,
}

#[derive(Debug, Deserialize)]
struct OllamaTagsResponse {
    models: Option<Vec<OllamaModel>>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RunningOllamaModel {
    pub name: String,
    #[serde(default)]
    pub size: u64,
    #[serde(default)]
    pub size_vram: u64,
}

#[derive(Debug, Deserialize)]
struct OllamaPsResponse {
    models: Option<Vec<RunningOllamaModel>>,
}

#[derive(Debug, Serialize)]
struct OllamaUnloadRequest {
    model: String,
    keep_alive: u8,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Serialize)]
struct OllamaChatRequest {
    model: String,
    messages: Vec<ChatMessage>,
    stream: bool,
}

#[derive(Debug, Deserialize)]
struct OllamaStreamMessage {
    content: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OllamaStreamLine {
    message: Option<OllamaStreamMessage>,
    done: Option<bool>,
    total_duration: Option<u64>,
    load_duration: Option<u64>,
    prompt_eval_count: Option<u32>,
    eval_count: Option<u32>,
    eval_duration: Option<u64>,
    error: Option<String>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase", tag = "event", content = "data")]
pub enum StreamEvent {
    Token {
        content: String,
    },
    #[serde(rename_all = "camelCase")]
    Done {
        total_duration: Option<u64>,
        load_duration: Option<u64>,
        prompt_eval_count: Option<u32>,
        eval_count: Option<u32>,
        eval_duration: Option<u64>,
    },
}

#[tauri::command]
pub async fn ollama_get_models(state: State<'_, OllamaState>) -> Result<Vec<OllamaModel>, String> {
    let url = format!("{OLLAMA_BASE_URL}/api/tags");

    let response = state
        .client
        .get(&url)
        .timeout(Duration::from_secs(15))
        .send()
        .await
        .map_err(|e| format!("Unable to connect to Ollama: {e}"))?;

    if !response.status().is_success() {
        return Err(format!("Ollama returned status {}", response.status()));
    }

    let data: OllamaTagsResponse = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse Ollama models response: {e}"))?;

    Ok(data.models.unwrap_or_default())
}

#[tauri::command]
pub async fn ollama_get_running_models(
    state: State<'_, OllamaState>,
) -> Result<Vec<RunningOllamaModel>, String> {
    let url = format!("{OLLAMA_BASE_URL}/api/ps");

    let response = state
        .client
        .get(&url)
        .timeout(Duration::from_secs(15))
        .send()
        .await
        .map_err(|e| format!("Unable to connect to Ollama: {e}"))?;

    if !response.status().is_success() {
        return Err(format!("Ollama returned status {}", response.status()));
    }

    let data: OllamaPsResponse = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse running models response: {e}"))?;

    Ok(data.models.unwrap_or_default())
}

#[tauri::command]
pub async fn ollama_unload_model(
    state: State<'_, OllamaState>,
    model: String,
) -> Result<(), String> {
    let url = format!("{OLLAMA_BASE_URL}/api/generate");
    let body = OllamaUnloadRequest {
        model,
        keep_alive: 0,
    };

    let response = state
        .client
        .post(&url)
        .timeout(Duration::from_secs(15))
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("Unable to unload model: {e}"))?;

    if !response.status().is_success() {
        return Err(format!("Ollama unload failed: {}", response.status()));
    }

    Ok(())
}

#[tauri::command]
pub async fn ollama_stop_generation(state: State<'_, OllamaState>) -> Result<(), String> {
    let guard = state.cancel_notify.lock().await;
    if let Some(notify) = guard.as_ref() {
        notify.notify_waiters();
    }
    Ok(())
}

fn process_json_line(line: &str, on_event: &Channel<StreamEvent>) -> Result<(), String> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return Ok(());
    }

    let parsed: OllamaStreamLine =
        serde_json::from_str(trimmed).map_err(|e| format!("Invalid Ollama stream JSON: {e}"))?;

    if let Some(err) = parsed.error {
        return Err(err);
    }

    if let Some(content) = parsed.message.and_then(|m| m.content) {
        if !content.is_empty() {
            on_event
                .send(StreamEvent::Token { content })
                .map_err(|e| format!("Failed to send token to UI: {e}"))?;
        }
    }

    if parsed.done.unwrap_or(false) {
        on_event
            .send(StreamEvent::Done {
                total_duration: parsed.total_duration,
                load_duration: parsed.load_duration,
                prompt_eval_count: parsed.prompt_eval_count,
                eval_count: parsed.eval_count,
                eval_duration: parsed.eval_duration,
            })
            .map_err(|e| format!("Failed to send done event to UI: {e}"))?;
    }

    Ok(())
}

#[tauri::command]
pub async fn ollama_chat_stream(
    state: State<'_, OllamaState>,
    model: String,
    messages: Vec<ChatMessage>,
    on_event: Channel<StreamEvent>,
) -> Result<(), String> {
    let notify = Arc::new(Notify::new());
    {
        let mut guard = state.cancel_notify.lock().await;
        if let Some(prev) = guard.take() {
            prev.notify_waiters();
        }
        *guard = Some(notify.clone());
    }

    let result = run_stream(&state.client, model, messages, &on_event, notify.clone()).await;

    {
        let mut guard = state.cancel_notify.lock().await;
        if guard
            .as_ref()
            .is_some_and(|current| Arc::ptr_eq(current, &notify))
        {
            *guard = None;
        }
    }

    result
}

async fn run_stream(
    client: &reqwest::Client,
    model: String,
    messages: Vec<ChatMessage>,
    on_event: &Channel<StreamEvent>,
    notify: Arc<Notify>,
) -> Result<(), String> {
    let url = format!("{OLLAMA_BASE_URL}/api/chat");
    let body = OllamaChatRequest {
        model,
        messages,
        stream: true,
    };

    let response = tokio::select! {
        _ = notify.notified() => return Err("ABORTED".to_string()),
        res = client.post(&url).json(&body).send() => {
            res.map_err(|e| format!("Unable to connect to Ollama: {e}"))?
        }
    };

    if !response.status().is_success() {
        return Err(format!("Ollama request failed: {}", response.status()));
    }

    let mut stream = response.bytes_stream();
    let mut buffer: Vec<u8> = Vec::new();

    loop {
        let next_chunk = tokio::select! {
            _ = notify.notified() => return Err("ABORTED".to_string()),
            chunk = stream.next() => chunk,
        };

        match next_chunk {
            Some(Ok(bytes)) => {
                buffer.extend_from_slice(&bytes);

                while let Some(pos) = buffer.iter().position(|&b| b == b'\n') {
                    let line_bytes: Vec<u8> = buffer.drain(..=pos).collect();
                    let line = String::from_utf8_lossy(&line_bytes[..pos]);
                    process_json_line(&line, on_event)?;
                }
            }
            Some(Err(e)) => {
                return Err(format!("Stream error from Ollama: {e}"));
            }
            None => {
                if !buffer.is_empty() {
                    let trailing = String::from_utf8_lossy(&buffer);
                    if !trailing.trim().is_empty() {
                        process_json_line(&trailing, on_event)?;
                    }
                }
                break;
            }
        }
    }

    Ok(())
}
