use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::{Duration, Instant};
use tauri::State;

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

const CREATE_NO_WINDOW: u32 = 0x08000000;
const EXECUTION_TIMEOUT_SECS: u64 = 10;
const MAX_SQL_ROWS: usize = 500;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionResult {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: Option<i32>,
    pub execution_time_ms: u64,
    pub success: bool,
}

#[cfg(test)]
fn get_sandbox_db_path() -> PathBuf {
    std::env::temp_dir().join("nexus_test_sandbox.db")
}

fn init_sandbox_db(conn: &Connection) -> Result<(), rusqlite::Error> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS students (
            id INTEGER PRIMARY KEY,
            name TEXT NOT NULL,
            major TEXT NOT NULL,
            gpa REAL,
            enrollment_year INTEGER
        );

        CREATE TABLE IF NOT EXISTS courses (
            id INTEGER PRIMARY KEY,
            code TEXT NOT NULL UNIQUE,
            title TEXT NOT NULL,
            credits INTEGER,
            department TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS enrollments (
            student_id INTEGER,
            course_id INTEGER,
            grade TEXT,
            PRIMARY KEY (student_id, course_id)
        );

        CREATE TABLE IF NOT EXISTS employees (
            id INTEGER PRIMARY KEY,
            name TEXT NOT NULL,
            department TEXT NOT NULL,
            role TEXT NOT NULL,
            salary INTEGER,
            hire_date TEXT
        );
        ",
    )?;

    // Seed sample data if empty
    let count: i64 = conn
        .query_row("SELECT count(*) FROM students", [], |r| r.get(0))
        .unwrap_or(0);

    if count == 0 {
        let _ = conn.execute_batch(
            "
            INSERT INTO students (name, major, gpa, enrollment_year) VALUES
                ('Alex Chen', 'Computer Science', 3.85, 2023),
                ('Maya Lin', 'Data Science', 3.92, 2022),
                ('Jordan Hayes', 'Software Engineering', 3.65, 2024),
                ('Priya Patel', 'Artificial Intelligence', 3.98, 2023),
                ('Liam O''Connor', 'Computer Systems', 3.40, 2021);

            INSERT INTO courses (code, title, credits, department) VALUES
                ('CS101', 'Intro to Programming', 4, 'CS'),
                ('CS202', 'Data Structures & Algorithms', 4, 'CS'),
                ('DB301', 'Database Systems', 3, 'CS'),
                ('AI401', 'Machine Learning Foundations', 4, 'AI');

            INSERT INTO enrollments (student_id, course_id, grade) VALUES
                (1, 1, 'A'),
                (1, 2, 'A-'),
                (1, 3, 'A'),
                (2, 3, 'A'),
                (2, 4, 'A+'),
                (3, 1, 'B+'),
                (4, 3, 'A+'),
                (4, 4, 'A');

            INSERT INTO employees (name, department, role, salary, hire_date) VALUES
                ('Elena Rostova', 'Engineering', 'Lead Architect', 145000, '2021-03-15'),
                ('Marcus Vance', 'Engineering', 'Senior Backend Dev', 120000, '2022-07-01'),
                ('Sarah Jenkins', 'Data Science', 'ML Engineer', 130000, '2023-01-10'),
                ('David Kim', 'Product', 'Product Manager', 115000, '2020-11-20');
            ",
        );
    }

    Ok(())
}

fn split_sql_statements(code: &str) -> Vec<String> {
    let mut statements = Vec::new();
    let mut current = String::new();
    let chars: Vec<char> = code.chars().collect();
    let len = chars.len();
    let mut i = 0;
    let mut in_single_quote = false;
    let mut in_double_quote = false;
    let mut in_line_comment = false;
    let mut in_block_comment = false;

    while i < len {
        let ch = chars[i];
        let next = if i + 1 < len { Some(chars[i + 1]) } else { None };

        if in_line_comment {
            current.push(ch);
            if ch == '\n' {
                in_line_comment = false;
            }
            i += 1;
            continue;
        }

        if in_block_comment {
            current.push(ch);
            if ch == '*' && next == Some('/') {
                current.push('/');
                in_block_comment = false;
                i += 2;
            } else {
                i += 1;
            }
            continue;
        }

        if in_single_quote {
            current.push(ch);
            if ch == '\'' {
                if next == Some('\'') {
                    current.push('\'');
                    i += 2;
                    continue;
                }
                in_single_quote = false;
            }
            i += 1;
            continue;
        }

        if in_double_quote {
            current.push(ch);
            if ch == '"' {
                if next == Some('"') {
                    current.push('"');
                    i += 2;
                    continue;
                }
                in_double_quote = false;
            }
            i += 1;
            continue;
        }

        if ch == '-' && next == Some('-') {
            current.push('-');
            current.push('-');
            in_line_comment = true;
            i += 2;
            continue;
        }

        if ch == '/' && next == Some('*') {
            current.push('/');
            current.push('*');
            in_block_comment = true;
            i += 2;
            continue;
        }

        if ch == '\'' {
            in_single_quote = true;
            current.push(ch);
            i += 1;
            continue;
        }

        if ch == '"' {
            in_double_quote = true;
            current.push(ch);
            i += 1;
            continue;
        }

        if ch == ';' {
            let trimmed = current.trim().to_string();
            if !trimmed.is_empty() {
                statements.push(trimmed);
            }
            current.clear();
            i += 1;
            continue;
        }

        current.push(ch);
        i += 1;
    }

    let trailing = current.trim().to_string();
    if !trailing.is_empty() {
        statements.push(trailing);
    }

    statements
}

/// Strips leading SQL single-line (`--`) and block (`/* ... */`) comments so statement keyword inspection cannot be bypassed
fn strip_leading_sql_comments(stmt: &str) -> &str {
    let mut s = stmt.trim();
    loop {
        if s.starts_with("--") {
            if let Some(nl) = s.find('\n') {
                s = s[nl + 1..].trim();
            } else {
                return "";
            }
        } else if s.starts_with("/*") {
            if let Some(end) = s.find("*/") {
                s = s[end + 2..].trim();
            } else {
                return "";
            }
        } else {
            break;
        }
    }
    s
}

#[cfg(test)]
fn execute_sql_sandbox(code: &str) -> ExecutionResult {
    execute_sql_sandbox_with_options(
        code,
        get_sandbox_db_path(),
        Duration::from_secs(EXECUTION_TIMEOUT_SECS),
    )
}

fn execute_sql_sandbox_with_options(
    code: &str,
    db_path: PathBuf,
    timeout: Duration,
) -> ExecutionResult {
    let start = Instant::now();

    // Split code by quote/comment-aware statement boundaries
    let statements = split_sql_statements(code);

    if statements.is_empty() {
        return ExecutionResult {
            stdout: "(No SQL statements provided)".to_string(),
            stderr: String::new(),
            exit_code: Some(0),
            execution_time_ms: start.elapsed().as_millis() as u64,
            success: true,
        };
    }

    for stmt in &statements {
        let cleaned_lower = strip_leading_sql_comments(stmt).to_lowercase();
        if cleaned_lower.starts_with("attach")
            || cleaned_lower.starts_with("detach")
            || cleaned_lower.contains("load_extension")
            || cleaned_lower.contains("vacuum into")
            || cleaned_lower.contains("attach ")
            || cleaned_lower.contains("detach ")
        {
            return ExecutionResult {
                stdout: String::new(),
                stderr: "Blocked: ATTACH, DETACH, LOAD_EXTENSION, and VACUUM INTO are disallowed in the SQL sandbox.".to_string(),
                exit_code: Some(1),
                execution_time_ms: start.elapsed().as_millis() as u64,
                success: false,
            };
        }
    }

    if let Some(parent) = db_path.parent() {
        let _ = fs::create_dir_all(parent);
    }

    let mut conn = match Connection::open(&db_path) {
        Ok(c) => c,
        Err(e) => {
            return ExecutionResult {
                stdout: String::new(),
                stderr: format!("Failed to open SQLite sandbox: {e}"),
                exit_code: Some(1),
                execution_time_ms: start.elapsed().as_millis() as u64,
                success: false,
            };
        }
    };

    if let Err(e) = init_sandbox_db(&conn) {
        return ExecutionResult {
            stdout: String::new(),
            stderr: format!("Failed to initialize SQLite sandbox schema: {e}"),
            exit_code: Some(1),
            execution_time_ms: start.elapsed().as_millis() as u64,
            success: false,
        };
    }

    // Lock down SQLite sandbox: prevent ATTACH DATABASE and external file access
    conn.set_limit(rusqlite::limits::Limit::SQLITE_LIMIT_ATTACHED, 0);

    // Watchdog thread interrupts runaway queries (e.g. infinite recursive CTEs) after timeout
    let interrupt_handle = conn.get_interrupt_handle();
    let watchdog_signal = Arc::new((Mutex::new((false, false)), Condvar::new()));
    let watchdog_signal_clone = Arc::clone(&watchdog_signal);
    let watchdog_thread = std::thread::spawn(move || {
        let (lock, cvar) = &*watchdog_signal_clone;
        let guard = lock.lock().unwrap_or_else(|e| e.into_inner());
        let (mut state, wait_res) = cvar
            .wait_timeout_while(guard, timeout, |s| !s.0)
            .unwrap_or_else(|e| e.into_inner());
        if !state.0 && wait_res.timed_out() {
            state.1 = true;
            interrupt_handle.interrupt();
        }
    });

    let mut output_lines = Vec::new();
    let mut had_error = false;
    let mut err_msg = String::new();

    let tx = match conn.transaction() {
        Ok(t) => t,
        Err(e) => {
            {
                let (lock, cvar) = &*watchdog_signal;
                let mut state = lock.lock().unwrap_or_else(|e| e.into_inner());
                state.0 = true;
                cvar.notify_one();
            }
            let _ = watchdog_thread.join();
            return ExecutionResult {
                stdout: String::new(),
                stderr: format!("Failed to start SQL transaction: {e}"),
                exit_code: Some(1),
                execution_time_ms: start.elapsed().as_millis() as u64,
                success: false,
            };
        }
    };

    for (stmt_idx, stmt) in statements.iter().enumerate() {
        let cleaned_lower = strip_leading_sql_comments(stmt).to_lowercase();
        let is_query = cleaned_lower.starts_with("select")
            || cleaned_lower.starts_with("pragma")
            || cleaned_lower.starts_with("explain")
            || cleaned_lower.starts_with("with");

        if is_query {
            match tx.prepare(stmt) {
                Ok(mut prepared) => {
                    let col_count = prepared.column_count();
                    let col_names: Vec<String> = prepared
                        .column_names()
                        .into_iter()
                        .map(|s| s.to_string())
                        .collect();

                    // Cap row materialization at MAX_SQL_ROWS + 1 so large cross-joins cannot OOM the process
                    let rows_res: Result<Vec<Vec<String>>, rusqlite::Error> = prepared
                        .query_map([], |row| {
                            let mut row_vals = Vec::with_capacity(col_count);
                            for i in 0..col_count {
                                let val_res: Result<String, _> = row.get(i).map(|v: rusqlite::types::Value| {
                                    match v {
                                        rusqlite::types::Value::Null => "NULL".to_string(),
                                        rusqlite::types::Value::Integer(n) => n.to_string(),
                                        rusqlite::types::Value::Real(f) => format!("{:.2}", f),
                                        rusqlite::types::Value::Text(s) => s,
                                        rusqlite::types::Value::Blob(_) => "<BLOB>".to_string(),
                                    }
                                });
                                row_vals.push(val_res.unwrap_or_else(|_| "?".to_string()));
                            }
                            Ok(row_vals)
                        })
                        .and_then(|mapped| mapped.take(MAX_SQL_ROWS + 1).collect());

                    match rows_res {
                        Ok(mut rows) => {
                            let truncated = rows.len() > MAX_SQL_ROWS;
                            if truncated {
                                rows.truncate(MAX_SQL_ROWS);
                            }

                            if statements.len() > 1 {
                                output_lines.push(format!("--- Statement {} ---", stmt_idx + 1));
                            }

                            // Calculate column widths
                            let mut col_widths: Vec<usize> =
                                col_names.iter().map(|n| n.len()).collect();
                            for r in &rows {
                                for (i, cell) in r.iter().enumerate() {
                                    if i < col_widths.len() {
                                        col_widths[i] = col_widths[i].max(cell.len());
                                    }
                                }
                            }

                            // Build ASCII table header
                            let header = col_names
                                .iter()
                                .enumerate()
                                .map(|(i, name)| format!("{:<width$}", name, width = col_widths[i]))
                                .collect::<Vec<_>>()
                                .join(" | ");

                            let divider = col_widths
                                .iter()
                                .map(|w| "-".repeat(*w))
                                .collect::<Vec<_>>()
                                .join("-+-");

                            output_lines.push(header);
                            output_lines.push(divider);

                            // Build table rows
                            for r in &rows {
                                let row_str = r
                                    .iter()
                                    .enumerate()
                                    .map(|(i, cell)| {
                                        format!("{:<width$}", cell, width = col_widths[i])
                                    })
                                    .collect::<Vec<_>>()
                                    .join(" | ");
                                output_lines.push(row_str);
                            }

                            if truncated {
                                output_lines.push(format!(
                                    "({} rows shown; result truncated at {} rows)\n",
                                    rows.len(),
                                    MAX_SQL_ROWS
                                ));
                            } else {
                                output_lines.push(format!("({} row(s) returned)\n", rows.len()));
                            }
                        }
                        Err(e) => {
                            had_error = true;
                            err_msg = format!("SQL query error: {e}");
                            break;
                        }
                    }
                }
                Err(e) => {
                    had_error = true;
                    err_msg = format!("SQL syntax/prepare error: {e}");
                    break;
                }
            }
        } else {
            // DDL or DML execution
            match tx.execute(stmt, []) {
                Ok(changes) => {
                    if statements.len() > 1 {
                        output_lines.push(format!("--- Statement {} ---", stmt_idx + 1));
                    }
                    if changes > 0 {
                        output_lines.push(format!("Query OK, {changes} row(s) affected.\n"));
                    } else {
                        output_lines.push("Query OK, statement executed successfully.\n".to_string());
                    }
                }
                Err(e) => {
                    had_error = true;
                    err_msg = format!("SQL execution error: {e}");
                    break;
                }
            }
        }
    }

    if had_error {
        let _ = tx.rollback();
    } else if let Err(e) = tx.commit() {
        had_error = true;
        err_msg = format!("SQL transaction commit error: {e}");
    }

    let timed_out = {
        let (lock, cvar) = &*watchdog_signal;
        let mut state = lock.lock().unwrap_or_else(|e| e.into_inner());
        state.0 = true;
        cvar.notify_one();
        state.1
    };
    let _ = watchdog_thread.join();

    if timed_out {
        had_error = true;
        err_msg = format!(
            "SQL execution timed out after {} seconds (query interrupted).",
            timeout.as_secs().max(1)
        );
    }

    let elapsed = start.elapsed().as_millis() as u64;
    ExecutionResult {
        stdout: output_lines.join("\n"),
        stderr: err_msg,
        exit_code: if had_error { Some(if timed_out { -1 } else { 1 }) } else { Some(0) },
        execution_time_ms: elapsed,
        success: !had_error,
    }
}

fn resolve_python_command() -> (String, Vec<String>) {
    static CACHED_PYTHON: OnceLock<(String, Vec<String>)> = OnceLock::new();
    CACHED_PYTHON
        .get_or_init(|| {
            let mut candidates: Vec<(String, Vec<String>)> = Vec::new();

            if let Ok(env_py) = std::env::var("NEXUS_PYTHON_PATH") {
                if !env_py.trim().is_empty() {
                    candidates.push((env_py, vec![]));
                }
            }

            if let Ok(local_app_data) = std::env::var("LOCALAPPDATA") {
                let launcher = PathBuf::from(local_app_data)
                    .join("Programs")
                    .join("Python")
                    .join("Launcher")
                    .join("py.exe");
                if launcher.exists() {
                    candidates.push((
                        launcher.to_string_lossy().to_string(),
                        vec!["-3".to_string()],
                    ));
                }
            }

            candidates.push(("py".to_string(), vec!["-3".to_string()]));
            candidates.push(("python3".to_string(), vec![]));
            candidates.push(("python".to_string(), vec![]));

            for (cmd, args) in candidates {
                let mut test_cmd = Command::new(&cmd);
                test_cmd.args(&args).arg("--version");
                #[cfg(target_os = "windows")]
                test_cmd.creation_flags(CREATE_NO_WINDOW);

                if let Ok(output) = test_cmd.output() {
                    if output.status.success() {
                        return (cmd, args);
                    }
                }
            }

            ("python".to_string(), vec![])
        })
        .clone()
}

fn resolve_node_command() -> String {
    static CACHED_NODE: OnceLock<String> = OnceLock::new();
    CACHED_NODE
        .get_or_init(|| {
            let mut candidates: Vec<String> = Vec::new();

            if let Ok(env_node) = std::env::var("NEXUS_NODE_PATH") {
                if !env_node.trim().is_empty() {
                    candidates.push(env_node);
                }
            }

            candidates.push("node".to_string());

            if let Ok(prog_files) = std::env::var("ProgramFiles") {
                let node_exe = PathBuf::from(prog_files).join("nodejs").join("node.exe");
                if node_exe.exists() {
                    candidates.push(node_exe.to_string_lossy().to_string());
                }
            }

            for cmd in candidates {
                let mut test_cmd = Command::new(&cmd);
                test_cmd.arg("--version");
                #[cfg(target_os = "windows")]
                test_cmd.creation_flags(CREATE_NO_WINDOW);

                if let Ok(output) = test_cmd.output() {
                    if output.status.success() {
                        return cmd;
                    }
                }
            }

            "node".to_string()
        })
        .clone()
}

fn read_pipe_bounded<R: std::io::Read>(mut reader: R, max_bytes: usize) -> String {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    let mut truncated = false;

    loop {
        match reader.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => {
                if buf.len() < max_bytes {
                    let remaining = max_bytes - buf.len();
                    let take = n.min(remaining);
                    buf.extend_from_slice(&chunk[..take]);
                    if take < n {
                        truncated = true;
                    }
                } else {
                    truncated = true;
                }
            }
            Err(_) => break,
        }
    }

    let mut text = String::from_utf8_lossy(&buf).to_string();
    if truncated {
        text.push_str("\n... [output truncated at 512 KB]");
    }
    text
}

fn run_command_with_timeout(mut cmd: Command, timeout: Duration) -> ExecutionResult {
    let start = Instant::now();
    const MAX_PIPE_BYTES: usize = 512 * 1024;

    #[cfg(target_os = "windows")]
    cmd.creation_flags(CREATE_NO_WINDOW);

    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            return ExecutionResult {
                stdout: String::new(),
                stderr: format!("Failed to spawn process: {e}"),
                exit_code: Some(1),
                execution_time_ms: start.elapsed().as_millis() as u64,
                success: false,
            };
        }
    };

    let stdout_pipe = child.stdout.take();
    let stderr_pipe = child.stderr.take();

    let stdout_handle = std::thread::spawn(move || {
        stdout_pipe
            .map(|p| read_pipe_bounded(p, MAX_PIPE_BYTES))
            .unwrap_or_default()
    });

    let stderr_handle = std::thread::spawn(move || {
        stderr_pipe
            .map(|p| read_pipe_bounded(p, MAX_PIPE_BYTES))
            .unwrap_or_default()
    });

    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let stdout = stdout_handle.join().unwrap_or_default();
                let stderr = stderr_handle.join().unwrap_or_default();

                return ExecutionResult {
                    stdout,
                    stderr,
                    exit_code: status.code(),
                    execution_time_ms: start.elapsed().as_millis() as u64,
                    success: status.success(),
                };
            }
            Ok(None) => {
                if start.elapsed() >= timeout {
                    #[cfg(target_os = "windows")]
                    {
                        let pid = child.id().to_string();
                        let _ = Command::new("taskkill")
                            .args(["/F", "/T", "/PID", &pid])
                            .creation_flags(CREATE_NO_WINDOW)
                            .output();
                    }
                    let _ = child.kill();
                    let _ = child.wait();
                    let stdout = stdout_handle.join().unwrap_or_default();
                    let mut stderr = stderr_handle.join().unwrap_or_default();
                    if !stderr.is_empty() {
                        stderr.push('\n');
                    }
                    stderr.push_str(&format!(
                        "Execution timed out after {} seconds (process terminated).",
                        timeout.as_secs()
                    ));

                    return ExecutionResult {
                        stdout,
                        stderr,
                        exit_code: Some(-1),
                        execution_time_ms: start.elapsed().as_millis() as u64,
                        success: false,
                    };
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = stdout_handle.join();
                let _ = stderr_handle.join();
                return ExecutionResult {
                    stdout: String::new(),
                    stderr: format!("Process monitor error: {e}"),
                    exit_code: Some(1),
                    execution_time_ms: start.elapsed().as_millis() as u64,
                    success: false,
                };
            }
        }
    }
}

#[tauri::command]
pub async fn execute_code_snippet(
    db_state: State<'_, crate::db::DbState>,
    language: String,
    code: String,
) -> Result<ExecutionResult, String> {
    let sandbox_db_path = db_state.app_data_dir.join("sandbox.db");
    tokio::task::spawn_blocking(move || {
        let lang = language.to_lowercase().trim().to_string();
        let timeout = Duration::from_secs(EXECUTION_TIMEOUT_SECS);

        match lang.as_str() {
            "sql" | "sqlite" | "sqlite3" => Ok(execute_sql_sandbox_with_options(
                &code,
                sandbox_db_path,
                timeout,
            )),
            "python" | "py" | "python3" => {
                let (py_cmd, py_args) = resolve_python_command();
                let temp_dir = std::env::temp_dir().join("nexus_runner");
                let _ = fs::create_dir_all(&temp_dir);
                let script_path = temp_dir.join(format!(
                    "snippet_{}_{}.py",
                    std::process::id(),
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis()
                ));

                if let Err(e) = fs::write(&script_path, &code) {
                    return Ok(ExecutionResult {
                        stdout: String::new(),
                        stderr: format!("Failed to write temporary Python script: {e}"),
                        exit_code: Some(1),
                        execution_time_ms: 0,
                        success: false,
                    });
                }

                let mut cmd = Command::new(&py_cmd);
                for arg in &py_args {
                    cmd.arg(arg);
                }
                cmd.arg("-I");
                cmd.arg(&script_path);
                cmd.current_dir(&temp_dir);
                cmd.env("PYTHONUTF8", "1");
                cmd.env("PYTHONIOENCODING", "utf-8");

                let result = run_command_with_timeout(cmd, timeout);
                let _ = fs::remove_file(&script_path);
                Ok(result)
            }
            "javascript" | "js" | "node" => {
                let node_cmd = resolve_node_command();
                let temp_dir = std::env::temp_dir().join("nexus_runner");
                let _ = fs::create_dir_all(&temp_dir);
                let script_path = temp_dir.join(format!(
                    "snippet_{}_{}.js",
                    std::process::id(),
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis()
                ));

                if let Err(e) = fs::write(&script_path, &code) {
                    return Ok(ExecutionResult {
                        stdout: String::new(),
                        stderr: format!("Failed to write temporary JavaScript script: {e}"),
                        exit_code: Some(1),
                        execution_time_ms: 0,
                        success: false,
                    });
                }

                let mut cmd = Command::new(&node_cmd);
                cmd.arg(&script_path);
                cmd.current_dir(&temp_dir);

                let result = run_command_with_timeout(cmd, timeout);
                let _ = fs::remove_file(&script_path);
                Ok(result)
            }
            "powershell" | "ps1" | "pwsh" | "bash" | "sh" | "shell" | "cmd" | "batch" => {
                let shell_allowed = std::env::var("NEXUS_ALLOW_SHELL_EXEC")
                    .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
                    .unwrap_or(false);

                if !shell_allowed {
                    return Ok(ExecutionResult {
                        stdout: String::new(),
                        stderr: "Host shell execution (PowerShell/CMD/Bash) is disabled by default for security. Set NEXUS_ALLOW_SHELL_EXEC=1 to explicitly opt in.".to_string(),
                        exit_code: Some(1),
                        execution_time_ms: 0,
                        success: false,
                    });
                }

                let temp_dir = std::env::temp_dir().join("nexus_runner");
                let _ = fs::create_dir_all(&temp_dir);

                let mut cmd = if matches!(lang.as_str(), "powershell" | "ps1" | "pwsh") {
                    let mut c = Command::new("powershell.exe");
                    c.args(["-NoProfile", "-NonInteractive", "-Command", &code]);
                    c
                } else {
                    let mut c = Command::new("cmd.exe");
                    c.args(["/c", &code]);
                    c
                };
                cmd.current_dir(&temp_dir);

                Ok(run_command_with_timeout(cmd, timeout))
            }
            _ => Err(format!(
                "Language '{}' is not supported for interactive code execution.",
                language
            )),
        }
    })
    .await
    .map_err(|e| format!("Task execution thread panicked: {e}"))?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_split_sql_statements_respects_quotes_and_comments() {
        let sql = "SELECT 'a;b' AS val; -- comment with ;\n/* block ; comment */ SELECT \"col;1\" FROM t;";
        let stmts = split_sql_statements(sql);
        assert_eq!(stmts.len(), 2);
        assert_eq!(stmts[0], "SELECT 'a;b' AS val");
        assert!(stmts[1].contains("SELECT \"col;1\" FROM t"));
    }

    #[test]
    fn test_sql_sandbox_blocks_attach_and_load_extension() {
        let attach_res = execute_sql_sandbox("ATTACH DATABASE 'nexus.db' AS stolen;");
        assert!(!attach_res.success);
        assert!(attach_res.stderr.contains("Blocked"));

        let ext_res = execute_sql_sandbox("SELECT load_extension('evil.dll');");
        assert!(!ext_res.success);
        assert!(ext_res.stderr.contains("Blocked"));

        let commented_attach = execute_sql_sandbox("/* bypass */ -- line\nATTACH DATABASE 'nexus.db' AS stolen;");
        assert!(!commented_attach.success);
        assert!(commented_attach.stderr.contains("Blocked"));
    }

    #[test]
    fn test_sql_sandbox_caps_rows_and_interrupts_runaway_cte() {
        let temp_db = std::env::temp_dir().join(format!(
            "nexus_sql_bounds_test_{}.db",
            std::process::id()
        ));

        // 1. Verify row truncation at MAX_SQL_ROWS (500)
        let many_rows = execute_sql_sandbox_with_options(
            "WITH RECURSIVE cnt(x) AS (SELECT 1 UNION ALL SELECT x + 1 FROM cnt WHERE x < 1200) SELECT x FROM cnt;",
            temp_db.clone(),
            Duration::from_secs(5),
        );
        assert!(many_rows.success);
        assert!(many_rows.stdout.contains("truncated at 500 rows"));

        // 2. Verify watchdog interrupts an infinite recursive query cleanly
        let infinite_cte = execute_sql_sandbox_with_options(
            "WITH RECURSIVE inf(x) AS (SELECT 1 UNION ALL SELECT x + 1 FROM inf) SELECT count(*) FROM inf;",
            temp_db.clone(),
            Duration::from_millis(150),
        );
        assert!(!infinite_cte.success);
        assert!(infinite_cte.stderr.contains("timed out"));

        let _ = fs::remove_file(&temp_db);
    }
}
