use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::{mpsc, Mutex, OnceLock};
use std::time::{Duration, Instant};

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x08000000;

fn resolve_memora_paths() -> (String, Option<PathBuf>) {
    static CACHED_PATHS: OnceLock<(String, Option<PathBuf>)> = OnceLock::new();
    CACHED_PATHS
        .get_or_init(|| {
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
        })
        .clone()
}

struct WarmMemoraWorker {
    child: Child,
    stdin: ChildStdin,
    stdout_rx: mpsc::Receiver<String>,
}

impl WarmMemoraWorker {
    fn spawn() -> Result<Self, String> {
        let worker_script = r#"
import json, sys, logging
logging.disable(logging.CRITICAL)
from memora.memory.manager import MemoryManager
mgr = MemoryManager()
for raw_line in sys.stdin:
    raw_line = raw_line.strip()
    if not raw_line:
        continue
    try:
        req = json.loads(raw_line)
        topic = str(req.get("topic", "")).strip()
        res = mgr.get_context(topic, k=4)
        print(json.dumps({"ok": True, "data": res}, ensure_ascii=False), flush=True)
    except Exception as exc:
        print(json.dumps({"ok": False, "error": str(exc)}, ensure_ascii=False), flush=True)
"#;

        let mut cmd = memora_command();
        cmd.args(["-u", "-c", worker_script])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());

        let mut child = cmd
            .spawn()
            .map_err(|e| format!("Failed to start MEMORA retrieval worker: {e}"))?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| "Failed to capture MEMORA worker stdin.".to_string())?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| "Failed to capture MEMORA worker stdout.".to_string())?;

        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let reader = BufReader::new(stdout);
            for line in reader.lines() {
                match line {
                    Ok(l) => {
                        if tx.send(l).is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
        });

        Ok(Self {
            child,
            stdin,
            stdout_rx: rx,
        })
    }

    fn is_alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    fn shutdown(mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for WarmMemoraWorker {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[derive(Debug, Deserialize)]
struct WorkerEnvelope {
    ok: bool,
    data: Option<PyContextResponse>,
    error: Option<String>,
}

fn warm_worker_slot() -> &'static Mutex<Option<WarmMemoraWorker>> {
    static WORKER: OnceLock<Mutex<Option<WarmMemoraWorker>>> = OnceLock::new();
    WORKER.get_or_init(|| Mutex::new(None))
}

pub fn reset_warm_worker() {
    if let Ok(mut slot) = warm_worker_slot().lock() {
        if let Some(worker) = slot.take() {
            worker.shutdown();
        }
    }
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
        let mut slot = warm_worker_slot()
            .lock()
            .unwrap_or_else(|e| e.into_inner());

        let needs_spawn = match slot.as_mut() {
            Some(w) => !w.is_alive(),
            None => true,
        };
        if needs_spawn {
            if let Some(dead) = slot.take() {
                dead.shutdown();
            }
            match WarmMemoraWorker::spawn() {
                Ok(worker) => *slot = Some(worker),
                Err(err) => {
                    return Ok(MemoraContextResult {
                        available: false,
                        context: String::new(),
                        sources: Vec::new(),
                        error: Some(err),
                    });
                }
            }
        }

        let payload = serde_json::json!({ "topic": clean_topic }).to_string();
        let line_result = if let Some(worker) = slot.as_mut() {
            let write_res = writeln!(worker.stdin, "{payload}").and_then(|_| worker.stdin.flush());
            if let Err(e) = write_res {
                Err(format!("Failed to write query to MEMORA worker: {e}"))
            } else {
                // First request may wait briefly for Python + ChromaDB warm-up; subsequent calls return in ms
                let deadline = Instant::now() + Duration::from_secs(20);
                loop {
                    let remaining = deadline.saturating_duration_since(Instant::now());
                    if remaining.is_zero() {
                        break Err("MEMORA retrieval timed out after 20 seconds.".to_string());
                    }
                    match worker.stdout_rx.recv_timeout(remaining) {
                        Ok(line) => {
                            if line.trim().starts_with('{') {
                                break Ok(line);
                            }
                        }
                        Err(mpsc::RecvTimeoutError::Timeout) => {
                            break Err("MEMORA retrieval timed out after 20 seconds.".to_string());
                        }
                        Err(mpsc::RecvTimeoutError::Disconnected) => {
                            break Err("MEMORA worker exited unexpectedly.".to_string());
                        }
                    }
                }
            }
        } else {
            Err("MEMORA worker unavailable.".to_string())
        };

        let json_line = match line_result {
            Ok(line) => line,
            Err(err) => {
                if let Some(broken) = slot.take() {
                    broken.shutdown();
                }
                return Ok(MemoraContextResult {
                    available: false,
                    context: String::new(),
                    sources: Vec::new(),
                    error: Some(err),
                });
            }
        };

        let envelope: WorkerEnvelope = match serde_json::from_str(&json_line) {
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

        if !envelope.ok {
            return Ok(MemoraContextResult {
                available: false,
                context: String::new(),
                sources: Vec::new(),
                error: Some(
                    envelope
                        .error
                        .unwrap_or_else(|| "MEMORA query failed.".to_string()),
                ),
            });
        }

        let Some(parsed) = envelope.data else {
            return Ok(MemoraContextResult {
                available: false,
                context: String::new(),
                sources: Vec::new(),
                error: None,
            });
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
            // Reset warm retrieval worker so newly ingested notes are immediately visible
            reset_warm_worker();
            Ok(stdout)
        } else {
            Err(stderr)
        }
    })
    .await
    .map_err(|e| format!("MEMORA ingest task failed: {e}"))?
}
