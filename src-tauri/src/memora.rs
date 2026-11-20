use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::io::Read;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x08000000;

use std::path::PathBuf;

fn resolve_memora_paths() -> (String, Option<PathBuf>) {
    let dir_env = std::env::var("MEMORA_DIR")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .map(PathBuf::from);

    let default_dir = dir_env.or_else(|| {
        std::env::var("USERPROFILE")
            .or_else(|_| std::env::var("HOME"))
            .ok()
            .map(|home| PathBuf::from(home).join("Downloads").join("MEMORA-master"))
            .filter(|p| p.exists())
    });

    if let Ok(py_env) = std::env::var("MEMORA_PYTHON") {
        if !py_env.trim().is_empty() {
            return (py_env, default_dir);
        }
    }

    if let Some(ref dir) = default_dir {
        let win_venv = dir.join(".venv").join("Scripts").join("python.exe");
        if win_venv.exists() {
            return (win_venv.to_string_lossy().to_string(), default_dir);
        }
        let unix_venv = dir.join(".venv").join("bin").join("python");
        if unix_venv.exists() {
            return (unix_venv.to_string_lossy().to_string(), default_dir);
        }
    }

    ("python".to_string(), default_dir)
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MemoraContextResult {
    pub available: bool,
    pub context: String,
    pub sources: Vec<String>,
    pub error: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MemoraSourceItem {
    pub path: String,
    pub status: String,
    pub size: u64,
    pub chunk_count: u32,
    pub indexed_at: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct PyChunk {
    file_path: Option<String>,
}

#[derive(Debug, Deserialize)]
struct PyContextResponse {
    context: Option<String>,
    chunks: Option<Vec<PyChunk>>,
    chunk_count: Option<usize>,
}

fn memora_command() -> Command {
    let (py_path, memora_dir) = resolve_memora_paths();
    let mut cmd = Command::new(py_path);
    if let Some(dir) = memora_dir {
        cmd.current_dir(dir);
    }
    cmd.env("PYTHONUTF8", "1")
        .env("PYTHONIOENCODING", "utf-8");

    #[cfg(target_os = "windows")]
    cmd.creation_flags(CREATE_NO_WINDOW);

    cmd
}

fn read_pipe_tail<R: Read>(mut reader: R, max_bytes: usize) -> String {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];

    loop {
        match reader.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => {
                buf.extend_from_slice(&chunk[..n]);
                if buf.len() > max_bytes {
                    let excess = buf.len() - max_bytes;
                    buf.drain(..excess);
                }
            }
            Err(_) => break,
        }
    }

    String::from_utf8_lossy(&buf).to_string()
}

fn run_memora_with_timeout(mut cmd: Command, timeout: Duration) -> Result<(bool, String, String), String> {
    const MAX_STDOUT_BYTES: usize = 2 * 1024 * 1024;
    const MAX_STDERR_BYTES: usize = 512 * 1024;

    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("Failed to spawn MEMORA process: {e}"))?;

    let stdout_pipe = child.stdout.take();
    let stderr_pipe = child.stderr.take();

    let stdout_handle = std::thread::spawn(move || {
        stdout_pipe
            .map(|p| read_pipe_tail(p, MAX_STDOUT_BYTES))
            .unwrap_or_default()
    });

    let stderr_handle = std::thread::spawn(move || {
        stderr_pipe
            .map(|p| read_pipe_tail(p, MAX_STDERR_BYTES))
            .unwrap_or_default()
    });

    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let stdout = stdout_handle.join().unwrap_or_default();
                let stderr = stderr_handle.join().unwrap_or_default();
                return Ok((status.success(), stdout, stderr));
            }
            Ok(None) => {
                if start.elapsed() >= timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    let _ = stdout_handle.join();
                    let _ = stderr_handle.join();
                    return Err(format!(
                        "MEMORA process timed out after {} seconds.",
                        timeout.as_secs()
                    ));
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = stdout_handle.join();
                let _ = stderr_handle.join();
                return Err(format!("MEMORA process wait error: {e}"));
            }
        }
    }
}

#[tauri::command]
pub async fn memora_check_status() -> Result<bool, String> {
    tokio::task::spawn_blocking(|| {
        let mut cmd = memora_command();
        cmd.args([
            "-c",
            "from memora.config import get_config; c = get_config(); assert c.vault_path.exists()",
        ]);
        let (success, _, _) = run_memora_with_timeout(cmd, Duration::from_secs(10))?;
        Ok(success)
    })
    .await
    .map_err(|e| format!("MEMORA status task failed: {e}"))?
}

#[tauri::command]
pub async fn memora_list_sources() -> Result<Vec<MemoraSourceItem>, String> {
    tokio::task::spawn_blocking(|| {
        let py_script = r#"
import json, logging
logging.disable(logging.CRITICAL)
from memora.memory.manager import MemoryManager
sources = MemoryManager().list_sources(limit=100)
print(json.dumps(sources, ensure_ascii=False))
"#;

        let mut cmd = memora_command();
        cmd.args(["-c", py_script]);
        let (success, stdout, stderr) = run_memora_with_timeout(cmd, Duration::from_secs(15))?;

        if !success {
            return Err(format!("MEMORA list_sources error: {}", stderr.trim()));
        }

        let json_line = stdout
            .lines()
            .rev()
            .find(|line| line.trim().starts_with('['))
            .unwrap_or("[]");

        let items: Vec<MemoraSourceItem> = serde_json::from_str(json_line)
            .map_err(|e| format!("Failed to parse MEMORA sources: {e}"))?;

        Ok(items)
    })
    .await
    .map_err(|e| format!("MEMORA list_sources task failed: {e}"))?
}

#[tauri::command]
pub async fn memora_get_context(topic: String) -> Result<MemoraContextResult, String> {
    let clean_topic = topic.trim().to_string();
    if clean_topic.is_empty() {
        return Ok(MemoraContextResult {
            available: false,
            context: String::new(),
            sources: Vec::new(),
            error: None,
        });
    }

    tokio::task::spawn_blocking(move || {
        let py_script = r#"
import json, sys, logging
logging.disable(logging.CRITICAL)
from memora.memory.manager import MemoryManager
res = MemoryManager().get_context(sys.argv[1], k=4)
print(json.dumps(res, ensure_ascii=False))
"#;

        let mut cmd = memora_command();
        cmd.args(["-c", py_script, &clean_topic]);

        let (success, stdout, stderr) = match run_memora_with_timeout(cmd, Duration::from_secs(15)) {
            Ok(res) => res,
            Err(timeout_err) => {
                return Ok(MemoraContextResult {
                    available: false,
                    context: String::new(),
                    sources: Vec::new(),
                    error: Some(timeout_err),
                });
            }
        };

        if !success {
            let err_msg = stderr.trim().to_string();
            return Ok(MemoraContextResult {
                available: false,
                context: String::new(),
                sources: Vec::new(),
                error: Some(if err_msg.is_empty() {
                    "MEMORA exited with an error.".to_string()
                } else {
                    err_msg
                }),
            });
        }

        let json_line = stdout
            .lines()
            .rev()
            .find(|line| line.trim().starts_with('{'))
            .unwrap_or("");

        if json_line.is_empty() {
            return Ok(MemoraContextResult {
                available: false,
                context: String::new(),
                sources: Vec::new(),
                error: Some("MEMORA returned no structured response.".to_string()),
            });
        }

        let parsed: PyContextResponse = match serde_json::from_str(json_line) {
            Ok(v) => v,
            Err(e) => {
                return Ok(MemoraContextResult {
                    available: false,
                    context: String::new(),
                    sources: Vec::new(),
                    error: Some(format!("Failed to parse MEMORA response: {e}")),
                });
            }
        };

        let context = parsed.context.unwrap_or_default();
        let chunk_count = parsed.chunk_count.unwrap_or(0);
        let mut unique_sources = BTreeSet::new();
        if let Some(chunks) = parsed.chunks {
            for chunk in chunks {
                if let Some(path) = chunk.file_path {
                    if !path.trim().is_empty() {
                        unique_sources.insert(path);
                    }
                }
            }
        }

        let has_results = chunk_count > 0 && !context.trim().is_empty();

        Ok(MemoraContextResult {
            available: has_results,
            context,
            sources: unique_sources.into_iter().collect(),
            error: None,
        })
    })
    .await
    .map_err(|e| format!("MEMORA context task failed: {e}"))?
}

#[tauri::command]
pub async fn memora_memorize(content: String, source: Option<String>) -> Result<bool, String> {
    let clean_content = content.trim().to_string();
    if clean_content.is_empty() {
        return Err("Content to memorize cannot be empty.".to_string());
    }
    let src = source.unwrap_or_else(|| "nexus".to_string());

    tokio::task::spawn_blocking(move || {
        let py_script = r#"
import sys, logging
logging.disable(logging.CRITICAL)
from memora.memory.manager import MemoryManager
res = MemoryManager().memorize(sys.argv[1], source=sys.argv[2])
assert getattr(res, "success", True)
"#;

        let mut cmd = memora_command();
        cmd.args(["-c", py_script, &clean_content, &src]);
        let (success, _, stderr) = run_memora_with_timeout(cmd, Duration::from_secs(20))?;

        if !success {
            return Err(format!("MEMORA memorize failed: {}", stderr.trim()));
        }

        Ok(true)
    })
    .await
    .map_err(|e| format!("MEMORA memorize task failed: {e}"))?
}

#[tauri::command]
pub async fn memora_ingest() -> Result<String, String> {
    tokio::task::spawn_blocking(|| {
        let mut cmd = memora_command();
        cmd.args(["-m", "memora.cli", "ingest"]);
        let (success, stdout, stderr) = run_memora_with_timeout(cmd, Duration::from_secs(120))?;

        if success {
            Ok(stdout)
        } else {
            Err(stderr)
        }
    })
    .await
    .map_err(|e| format!("MEMORA ingest task failed: {e}"))?
}
