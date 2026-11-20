use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::State;

static ID_COUNTER: AtomicU64 = AtomicU64::new(1);

pub struct DbState {
    pub conn: Mutex<Connection>,
    pub app_data_dir: PathBuf,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Workspace {
    pub id: String,
    pub name: String,
    pub description: String,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Conversation {
    pub id: String,
    pub title: String,
    pub model: String,
    pub workspace_id: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct StoredMessage {
    pub id: i64,
    pub conversation_id: String,
    pub role: String,
    pub content: String,
    pub model: Option<String>,
    pub created_at: i64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct WorkspaceNote {
    pub id: String,
    pub workspace_id: String,
    pub title: String,
    pub content: String,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct WorkspaceMaterial {
    pub id: String,
    pub workspace_id: String,
    pub category: String, // "lectures", "assignments", "other"
    pub title: String,
    pub filename: String,
    pub content_preview: String,
    pub created_at: i64,
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn generate_id(prefix: &str) -> String {
    let ts = now_ms();
    let seq = ID_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{prefix}_{ts}_{seq}")
}

fn validate_safe_id(id: &str) -> Result<(), String> {
    if id.is_empty()
        || id.len() > 128
        || !id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return Err("Invalid identifier format.".to_string());
    }
    Ok(())
}

fn validate_file_path(path_str: &str, allowed_exts: &[&str]) -> Result<PathBuf, String> {
    let trimmed = path_str.trim();
    if trimmed.is_empty() || trimmed.contains('\0') {
        return Err("Invalid file path.".to_string());
    }

    let path = PathBuf::from(trimmed);
    for comp in path.components() {
        if matches!(comp, std::path::Component::ParentDir) {
            return Err("Path traversal ('..') is not allowed.".to_string());
        }
    }

    if !allowed_exts.is_empty() {
        let ext = path
            .extension()
            .map(|e| e.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        if !allowed_exts.iter().any(|allowed| *allowed == ext) {
            return Err(format!(
                "Unsupported file extension '.{ext}'. Allowed: {}",
                allowed_exts.join(", ")
            ));
        }
    }

    Ok(path)
}

fn ensure_schema(conn: &Connection) -> Result<(), String> {
    conn.execute_batch(
        "
            PRAGMA foreign_keys = ON;
            PRAGMA journal_mode = WAL;

            CREATE TABLE IF NOT EXISTS workspaces (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                description TEXT NOT NULL DEFAULT '',
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS conversations (
                id TEXT PRIMARY KEY,
                title TEXT NOT NULL,
                model TEXT NOT NULL,
                workspace_id TEXT,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL,
                FOREIGN KEY (workspace_id) REFERENCES workspaces(id) ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS messages (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                conversation_id TEXT NOT NULL,
                role TEXT NOT NULL,
                content TEXT NOT NULL,
                model TEXT,
                created_at INTEGER NOT NULL,
                FOREIGN KEY (conversation_id) REFERENCES conversations(id) ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS workspace_notes (
                id TEXT PRIMARY KEY,
                workspace_id TEXT NOT NULL,
                title TEXT NOT NULL,
                content TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL,
                FOREIGN KEY (workspace_id) REFERENCES workspaces(id) ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS workspace_materials (
                id TEXT PRIMARY KEY,
                workspace_id TEXT NOT NULL,
                category TEXT NOT NULL,
                title TEXT NOT NULL,
                filename TEXT NOT NULL,
                content_preview TEXT NOT NULL DEFAULT '',
                created_at INTEGER NOT NULL,
                FOREIGN KEY (workspace_id) REFERENCES workspaces(id) ON DELETE CASCADE
            );

            CREATE INDEX IF NOT EXISTS idx_conversations_workspace_id
                ON conversations(workspace_id, updated_at DESC);

            CREATE INDEX IF NOT EXISTS idx_messages_conversation_id
                ON messages(conversation_id, id);

            CREATE INDEX IF NOT EXISTS idx_conversations_updated_at
                ON conversations(updated_at DESC);

            CREATE INDEX IF NOT EXISTS idx_workspace_notes_ws
                ON workspace_notes(workspace_id, updated_at DESC);

            CREATE INDEX IF NOT EXISTS idx_workspace_materials_ws
                ON workspace_materials(workspace_id, category);

            CREATE TABLE IF NOT EXISTS usage_records (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                timestamp INTEGER NOT NULL,
                date TEXT NOT NULL,
                model TEXT NOT NULL,
                prompt_tokens INTEGER NOT NULL DEFAULT 0,
                completion_tokens INTEGER NOT NULL DEFAULT 0,
                total_tokens INTEGER NOT NULL DEFAULT 0,
                duration_ms INTEGER NOT NULL DEFAULT 0,
                tokens_per_sec REAL
            );

            CREATE INDEX IF NOT EXISTS idx_usage_records_date ON usage_records(date);
            CREATE INDEX IF NOT EXISTS idx_usage_records_timestamp ON usage_records(timestamp);

            CREATE VIRTUAL TABLE IF NOT EXISTS workspace_materials_fts USING fts5(
                material_id UNINDEXED,
                workspace_id UNINDEXED,
                title,
                filename,
                content_preview
            );

            CREATE TRIGGER IF NOT EXISTS trg_materials_fts_insert
            AFTER INSERT ON workspace_materials BEGIN
                INSERT INTO workspace_materials_fts(material_id, workspace_id, title, filename, content_preview)
                VALUES (new.id, new.workspace_id, new.title, new.filename, new.content_preview);
            END;

            CREATE TRIGGER IF NOT EXISTS trg_materials_fts_delete
            AFTER DELETE ON workspace_materials BEGIN
                DELETE FROM workspace_materials_fts WHERE material_id = old.id;
            END;
            ",
        )
        .map_err(|e| format!("Failed to initialize database schema: {e}"))?;

        // Sync existing workspace_materials into FTS5 index if empty
        let fts_count: i64 = conn
            .query_row("SELECT count(*) FROM workspace_materials_fts", [], |r| r.get(0))
            .unwrap_or(0);
        if fts_count == 0 {
            let _ = conn.execute(
                "INSERT INTO workspace_materials_fts(material_id, workspace_id, title, filename, content_preview)
                 SELECT id, workspace_id, title, filename, content_preview FROM workspace_materials",
                [],
            );
        }

        Ok(())
}

impl DbState {
    pub fn initialize(app_data_dir: PathBuf) -> Result<Self, String> {
        fs::create_dir_all(&app_data_dir)
            .map_err(|e| format!("Failed to create app data directory: {e}"))?;

        let db_path = app_data_dir.join("nexus.db");
        let conn = Connection::open(&db_path)
            .map_err(|e| format!("Failed to open SQLite database: {e}"))?;

        ensure_schema(&conn)?;

        Ok(Self {
            conn: Mutex::new(conn),
            app_data_dir,
        })
    }
}

// ================= WORKSPACES =================

#[tauri::command]
pub fn db_list_workspaces(state: State<'_, DbState>) -> Result<Vec<Workspace>, String> {
    let conn = state
        .conn
        .lock()
        .map_err(|_| "Database lock poisoned".to_string())?;

    let mut stmt = conn
        .prepare(
            "SELECT id, name, description, created_at, updated_at
             FROM workspaces
             ORDER BY updated_at DESC
             LIMIT 200",
        )
        .map_err(|e| format!("Failed to prepare workspaces query: {e}"))?;

    let rows = stmt
        .query_map([], |row| {
            Ok(Workspace {
                id: row.get(0)?,
                name: row.get(1)?,
                description: row.get(2)?,
                created_at: row.get(3)?,
                updated_at: row.get(4)?,
            })
        })
        .map_err(|e| format!("Failed to query workspaces: {e}"))?;

    let mut workspaces = Vec::new();
    for row in rows {
        workspaces.push(row.map_err(|e| format!("Failed to read workspace row: {e}"))?);
    }

    Ok(workspaces)
}

#[tauri::command]
pub fn db_create_workspace(
    state: State<'_, DbState>,
    name: String,
    description: String,
) -> Result<Workspace, String> {
    let clean_name = name.trim();
    if clean_name.is_empty() {
        return Err("Workspace name cannot be empty.".to_string());
    }

    let conn = state
        .conn
        .lock()
        .map_err(|_| "Database lock poisoned".to_string())?;

    let id = generate_id("ws");
    let now = now_ms();
    let desc = description.trim().to_string();

    conn.execute(
        "INSERT INTO workspaces (id, name, description, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![id, clean_name, desc, now, now],
    )
    .map_err(|e| format!("Failed to create workspace: {e}"))?;

    Ok(Workspace {
        id,
        name: clean_name.to_string(),
        description: desc,
        created_at: now,
        updated_at: now,
    })
}

#[tauri::command]
pub fn db_rename_workspace(
    state: State<'_, DbState>,
    id: String,
    name: String,
    description: Option<String>,
) -> Result<(), String> {
    let clean_name = name.trim();
    if clean_name.is_empty() {
        return Err("Workspace name cannot be empty.".to_string());
    }

    let conn = state
        .conn
        .lock()
        .map_err(|_| "Database lock poisoned".to_string())?;

    let now = now_ms();
    if let Some(desc) = description {
        conn.execute(
            "UPDATE workspaces SET name = ?1, description = ?2, updated_at = ?3 WHERE id = ?4",
            params![clean_name, desc.trim(), now, id],
        )
        .map_err(|e| format!("Failed to update workspace: {e}"))?;
    } else {
        conn.execute(
            "UPDATE workspaces SET name = ?1, updated_at = ?2 WHERE id = ?3",
            params![clean_name, now, id],
        )
        .map_err(|e| format!("Failed to rename workspace: {e}"))?;
    }

    Ok(())
}

#[tauri::command]
pub fn db_delete_workspace(state: State<'_, DbState>, id: String) -> Result<(), String> {
    validate_safe_id(&id)?;

    {
        let conn = state
            .conn
            .lock()
            .map_err(|_| "Database lock poisoned".to_string())?;

        conn.execute("DELETE FROM workspaces WHERE id = ?1", params![id])
            .map_err(|e| format!("Failed to delete workspace: {e}"))?;
    }

    let ws_dir = state.app_data_dir.join("workspaces").join(&id);
    if ws_dir.exists() {
        let _ = fs::remove_dir_all(&ws_dir);
    }

    Ok(())
}

// ================= CONVERSATIONS (ISOLATED) =================

#[tauri::command]
pub fn db_list_conversations(
    state: State<'_, DbState>,
    workspace_id: Option<String>,
) -> Result<Vec<Conversation>, String> {
    let conn = state
        .conn
        .lock()
        .map_err(|_| "Database lock poisoned".to_string())?;

    let mut conversations = Vec::new();

    if let Some(ws_id) = workspace_id {
        let mut stmt = conn
            .prepare(
                "SELECT id, title, model, workspace_id, created_at, updated_at
                 FROM conversations
                 WHERE workspace_id = ?1
                 ORDER BY updated_at DESC
                 LIMIT 200",
            )
            .map_err(|e| format!("Failed to prepare conversations query: {e}"))?;

        let rows = stmt
            .query_map(params![ws_id], |row| {
                Ok(Conversation {
                    id: row.get(0)?,
                    title: row.get(1)?,
                    model: row.get(2)?,
                    workspace_id: row.get(3)?,
                    created_at: row.get(4)?,
                    updated_at: row.get(5)?,
                })
            })
            .map_err(|e| format!("Failed to query workspace conversations: {e}"))?;

        for row in rows {
            conversations.push(row.map_err(|e| format!("Failed to read conversation: {e}"))?);
        }
    } else {
        let mut stmt = conn
            .prepare(
                "SELECT id, title, model, workspace_id, created_at, updated_at
                 FROM conversations
                 WHERE workspace_id IS NULL
                 ORDER BY updated_at DESC
                 LIMIT 200",
            )
            .map_err(|e| format!("Failed to prepare conversations query: {e}"))?;

        let rows = stmt
            .query_map([], |row| {
                Ok(Conversation {
                    id: row.get(0)?,
                    title: row.get(1)?,
                    model: row.get(2)?,
                    workspace_id: row.get(3)?,
                    created_at: row.get(4)?,
                    updated_at: row.get(5)?,
                })
            })
            .map_err(|e| format!("Failed to query global conversations: {e}"))?;

        for row in rows {
            conversations.push(row.map_err(|e| format!("Failed to read conversation: {e}"))?);
        }
    }

    Ok(conversations)
}

#[tauri::command]
pub fn db_create_conversation(
    state: State<'_, DbState>,
    title: String,
    model: String,
    workspace_id: Option<String>,
) -> Result<Conversation, String> {
    let conn = state
        .conn
        .lock()
        .map_err(|_| "Database lock poisoned".to_string())?;

    let id = generate_id("conv");
    let now = now_ms();
    let clean_title = if title.trim().is_empty() {
        "New chat".to_string()
    } else {
        title.trim().to_string()
    };

    conn.execute(
        "INSERT INTO conversations (id, title, model, workspace_id, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![id, clean_title, model, workspace_id, now, now],
    )
    .map_err(|e| format!("Failed to create conversation: {e}"))?;

    Ok(Conversation {
        id,
        title: clean_title,
        model,
        workspace_id,
        created_at: now,
        updated_at: now,
    })
}

#[tauri::command]
pub fn db_rename_conversation(
    state: State<'_, DbState>,
    id: String,
    title: String,
) -> Result<(), String> {
    let clean_title = title.trim();
    if clean_title.is_empty() {
        return Err("Conversation title cannot be empty.".to_string());
    }

    let conn = state
        .conn
        .lock()
        .map_err(|_| "Database lock poisoned".to_string())?;

    conn.execute(
        "UPDATE conversations SET title = ?1, updated_at = ?2 WHERE id = ?3",
        params![clean_title, now_ms(), id],
    )
    .map_err(|e| format!("Failed to rename conversation: {e}"))?;

    Ok(())
}

#[tauri::command]
pub fn db_delete_conversation(state: State<'_, DbState>, id: String) -> Result<(), String> {
    let conn = state
        .conn
        .lock()
        .map_err(|_| "Database lock poisoned".to_string())?;

    conn.execute("DELETE FROM conversations WHERE id = ?1", params![id])
        .map_err(|e| format!("Failed to delete conversation: {e}"))?;

    Ok(())
}

#[tauri::command]
pub fn db_get_messages(
    state: State<'_, DbState>,
    conversation_id: String,
) -> Result<Vec<StoredMessage>, String> {
    let conn = state
        .conn
        .lock()
        .map_err(|_| "Database lock poisoned".to_string())?;

    let mut stmt = conn
        .prepare(
            "SELECT id, conversation_id, role, content, model, created_at
             FROM messages
             WHERE conversation_id = ?1
             ORDER BY id ASC
             LIMIT 500",
        )
        .map_err(|e| format!("Failed to prepare messages query: {e}"))?;

    let rows = stmt
        .query_map(params![conversation_id], |row| {
            Ok(StoredMessage {
                id: row.get(0)?,
                conversation_id: row.get(1)?,
                role: row.get(2)?,
                content: row.get(3)?,
                model: row.get(4)?,
                created_at: row.get(5)?,
            })
        })
        .map_err(|e| format!("Failed to query messages: {e}"))?;

    let mut messages = Vec::new();
    for row in rows {
        messages.push(row.map_err(|e| format!("Failed to read message row: {e}"))?);
    }

    Ok(messages)
}

#[tauri::command]
pub fn db_add_message(
    state: State<'_, DbState>,
    conversation_id: String,
    role: String,
    content: String,
    model: Option<String>,
) -> Result<StoredMessage, String> {
    let conn = state
        .conn
        .lock()
        .map_err(|_| "Database lock poisoned".to_string())?;

    let now = now_ms();

    conn.execute(
        "INSERT INTO messages (conversation_id, role, content, model, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![conversation_id, role, content, model, now],
    )
    .map_err(|e| format!("Failed to save message: {e}"))?;

    let id = conn.last_insert_rowid();

    if let Some(ref model_name) = model {
        conn.execute(
            "UPDATE conversations SET updated_at = ?1, model = ?2 WHERE id = ?3",
            params![now, model_name, conversation_id],
        )
        .map_err(|e| format!("Failed to update conversation metadata: {e}"))?;
    } else {
        conn.execute(
            "UPDATE conversations SET updated_at = ?1 WHERE id = ?2",
            params![now, conversation_id],
        )
        .map_err(|e| format!("Failed to update conversation timestamp: {e}"))?;
    }

    Ok(StoredMessage {
        id,
        conversation_id,
        role,
        content,
        model,
        created_at: now,
    })
}

// ================= WORKSPACE NOTES (ISOLATED) =================

#[tauri::command]
pub fn db_list_workspace_notes(
    state: State<'_, DbState>,
    workspace_id: String,
) -> Result<Vec<WorkspaceNote>, String> {
    let conn = state
        .conn
        .lock()
        .map_err(|_| "Database lock poisoned".to_string())?;

    let mut stmt = conn
        .prepare(
            "SELECT id, workspace_id, title, content, created_at, updated_at
             FROM workspace_notes
             WHERE workspace_id = ?1
             ORDER BY updated_at DESC
             LIMIT 200",
        )
        .map_err(|e| format!("Failed to prepare notes query: {e}"))?;

    let rows = stmt
        .query_map(params![workspace_id], |row| {
            Ok(WorkspaceNote {
                id: row.get(0)?,
                workspace_id: row.get(1)?,
                title: row.get(2)?,
                content: row.get(3)?,
                created_at: row.get(4)?,
                updated_at: row.get(5)?,
            })
        })
        .map_err(|e| format!("Failed to query notes: {e}"))?;

    let mut notes = Vec::new();
    for row in rows {
        notes.push(row.map_err(|e| format!("Failed to read note row: {e}"))?);
    }

    Ok(notes)
}

#[tauri::command]
pub fn db_save_workspace_note(
    state: State<'_, DbState>,
    id: Option<String>,
    workspace_id: String,
    title: String,
    content: String,
) -> Result<WorkspaceNote, String> {
    let conn = state
        .conn
        .lock()
        .map_err(|_| "Database lock poisoned".to_string())?;

    let now = now_ms();
    let clean_title = if title.trim().is_empty() {
        "Untitled Note".to_string()
    } else {
        title.trim().to_string()
    };

    if let Some(existing_id) = id {
        conn.execute(
            "UPDATE workspace_notes
             SET title = ?1, content = ?2, updated_at = ?3
             WHERE id = ?4 AND workspace_id = ?5",
            params![clean_title, content, now, existing_id, workspace_id],
        )
        .map_err(|e| format!("Failed to update note: {e}"))?;

        Ok(WorkspaceNote {
            id: existing_id,
            workspace_id,
            title: clean_title,
            content,
            created_at: now,
            updated_at: now,
        })
    } else {
        let new_id = generate_id("note");
        conn.execute(
            "INSERT INTO workspace_notes (id, workspace_id, title, content, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![new_id, workspace_id, clean_title, content, now, now],
        )
        .map_err(|e| format!("Failed to insert note: {e}"))?;

        Ok(WorkspaceNote {
            id: new_id,
            workspace_id,
            title: clean_title,
            content,
            created_at: now,
            updated_at: now,
        })
    }
}

#[tauri::command]
pub fn db_delete_workspace_note(
    state: State<'_, DbState>,
    workspace_id: String,
    note_id: String,
) -> Result<(), String> {
    let conn = state
        .conn
        .lock()
        .map_err(|_| "Database lock poisoned".to_string())?;

    conn.execute(
        "DELETE FROM workspace_notes WHERE id = ?1 AND workspace_id = ?2",
        params![note_id, workspace_id],
    )
    .map_err(|e| format!("Failed to delete note: {e}"))?;

    Ok(())
}

// ================= WORKSPACE MATERIALS =================

#[tauri::command]
pub fn db_list_workspace_materials(
    state: State<'_, DbState>,
    workspace_id: String,
) -> Result<Vec<WorkspaceMaterial>, String> {
    let conn = state
        .conn
        .lock()
        .map_err(|_| "Database lock poisoned".to_string())?;

    let mut stmt = conn
        .prepare(
            "SELECT id, workspace_id, category, title, filename, content_preview, created_at
             FROM workspace_materials
             WHERE workspace_id = ?1
             ORDER BY created_at DESC
             LIMIT 200",
        )
        .map_err(|e| format!("Failed to prepare materials query: {e}"))?;

    let rows = stmt
        .query_map(params![workspace_id], |row| {
            Ok(WorkspaceMaterial {
                id: row.get(0)?,
                workspace_id: row.get(1)?,
                category: row.get(2)?,
                title: row.get(3)?,
                filename: row.get(4)?,
                content_preview: row.get(5)?,
                created_at: row.get(6)?,
            })
        })
        .map_err(|e| format!("Failed to query materials: {e}"))?;

    let mut materials = Vec::new();
    for row in rows {
        materials.push(row.map_err(|e| format!("Failed to read material row: {e}"))?);
    }

    Ok(materials)
}

#[tauri::command]
pub fn db_add_workspace_material(
    state: State<'_, DbState>,
    workspace_id: String,
    category: String,
    title: String,
    filename: String,
    content_preview: String,
) -> Result<WorkspaceMaterial, String> {
    let conn = state
        .conn
        .lock()
        .map_err(|_| "Database lock poisoned".to_string())?;

    let id = generate_id("mat");
    let now = now_ms();
    let clean_title = if title.trim().is_empty() {
        filename.clone()
    } else {
        title.trim().to_string()
    };

    conn.execute(
        "INSERT INTO workspace_materials (id, workspace_id, category, title, filename, content_preview, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![id, workspace_id, category, clean_title, filename, content_preview, now],
    )
    .map_err(|e| format!("Failed to add material: {e}"))?;

    Ok(WorkspaceMaterial {
        id,
        workspace_id,
        category,
        title: clean_title,
        filename,
        content_preview,
        created_at: now,
    })
}

#[tauri::command]
pub async fn db_import_workspace_file(
    state: State<'_, DbState>,
    workspace_id: String,
    category: String,
    source_path: String,
) -> Result<WorkspaceMaterial, String> {
    validate_safe_id(&workspace_id)?;
    let src = validate_file_path(&source_path, &[])?;

    if !src.is_file() {
        return Err("Source file does not exist or is not a regular file.".to_string());
    }

    // Verify workspace exists before writing to disk
    {
        let conn = state
            .conn
            .lock()
            .map_err(|_| "Database lock poisoned".to_string())?;
        let exists: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM workspaces WHERE id = ?1)",
                params![workspace_id],
                |r| r.get(0),
            )
            .map_err(|e| format!("Failed to verify workspace: {e}"))?;
        if !exists {
            return Err("Workspace does not exist.".to_string());
        }
    }

    let mat_id = generate_id("mat");
    let app_data_dir = state.app_data_dir.clone();
    let ws_id_clone = workspace_id.clone();
    let mat_id_clone = mat_id.clone();

    // Offload file copy and PDF/text extraction off the main command thread without holding DB lock
    let (filename, title, preview) = tokio::task::spawn_blocking(move || -> Result<(String, String, String), String> {
        let filename = src
            .file_name()
            .map(|f| f.to_string_lossy().to_string())
            .filter(|f| !f.trim().is_empty() && !f.contains(['/', '\\']))
            .unwrap_or_else(|| "imported_file".to_string());

        let ext = src
            .extension()
            .map(|e| e.to_string_lossy().to_lowercase())
            .unwrap_or_default();

        let target_dir = app_data_dir
            .join("workspaces")
            .join(&ws_id_clone)
            .join("materials");

        fs::create_dir_all(&target_dir)
            .map_err(|e| format!("Failed to create materials directory: {e}"))?;

        let stored_filename = format!("{mat_id_clone}_{filename}");
        let target_file = target_dir.join(&stored_filename);
        fs::copy(&src, &target_file)
            .map_err(|e| format!("Failed to copy file to materials: {e}"))?;

        let preview = match ext.as_str() {
            "txt" | "md" | "json" | "csv" | "log" | "rs" | "py" | "js" | "ts" => {
                use std::io::Read;
                fs::File::open(&target_file)
                    .map(|file| {
                        let mut buf = Vec::new();
                        let _ = file.take(64 * 1024).read_to_end(&mut buf);
                        let text = String::from_utf8_lossy(&buf);
                        let truncated: String = text.chars().take(1500).collect();
                        if text.chars().count() > 1500 {
                            format!("{truncated}...")
                        } else {
                            truncated
                        }
                    })
                    .unwrap_or_default()
            }
            "pdf" => match lopdf::Document::load(&target_file) {
                Ok(doc) => {
                    let mut text = String::new();
                    let pages = doc.get_pages();
                    for (&page_num, _) in pages.iter().take(3) {
                        if let Ok(page_text) = doc.extract_text(&[page_num]) {
                            text.push_str(&page_text);
                            text.push('\n');
                        }
                    }
                    if text.trim().is_empty() {
                        "PDF uploaded (visual/scanned PDF - text extraction will require OCR in indexing).".to_string()
                    } else {
                        let truncated: String = text.chars().take(1500).collect();
                        if text.chars().count() > 1500 {
                            format!("{truncated}...")
                        } else {
                            truncated
                        }
                    }
                }
                Err(e) => format!("PDF file uploaded (parsing error: {e})"),
            },
            _ => format!("File uploaded: {filename}"),
        };

        let title = filename
            .rsplit_once('.')
            .map(|(base, _)| base.replace(['_', '-'], " "))
            .unwrap_or_else(|| filename.clone());

        Ok((filename, title, preview))
    })
    .await
    .map_err(|e| format!("File import worker panicked: {e}"))??;

    let now = now_ms();
    let conn = state
        .conn
        .lock()
        .map_err(|_| "Database lock poisoned".to_string())?;

    conn.execute(
        "INSERT INTO workspace_materials (id, workspace_id, category, title, filename, content_preview, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![mat_id, workspace_id, category, title, filename, preview, now],
    )
    .map_err(|e| format!("Failed to add material: {e}"))?;

    Ok(WorkspaceMaterial {
        id: mat_id,
        workspace_id,
        category,
        title,
        filename,
        content_preview: preview,
        created_at: now,
    })
}

#[derive(Debug, Serialize, Deserialize)]
pub struct WorkspaceContextResult {
    pub available: bool,
    pub context: String,
    pub sources: Vec<String>,
}

#[tauri::command]
pub fn db_get_workspace_context(
    state: State<'_, DbState>,
    workspace_id: String,
    query: String,
) -> Result<WorkspaceContextResult, String> {
    let clean_query = query.trim().to_lowercase();
    if clean_query.is_empty() {
        return Ok(WorkspaceContextResult {
            available: false,
            context: String::new(),
            sources: Vec::new(),
        });
    }

    let conn = state
        .conn
        .lock()
        .map_err(|_| "Database lock poisoned".to_string())?;

    let mut matched_sections = Vec::new();
    let mut sources = Vec::new();

    // Build sanitized FTS5 query from alphanumeric words
    let fts_tokens: Vec<String> = clean_query
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| t.len() > 1)
        .take(12)
        .map(|t| format!("\"{t}\""))
        .collect();

    if !fts_tokens.is_empty() {
        let fts_query = fts_tokens.join(" OR ");
        if let Ok(mut fts_stmt) = conn.prepare(
            "SELECT m.category, m.title, m.filename, m.content_preview
             FROM workspace_materials_fts f
             JOIN workspace_materials m ON m.id = f.material_id
             WHERE f.workspace_id = ?1 AND workspace_materials_fts MATCH ?2
             ORDER BY bm25(workspace_materials_fts) ASC
             LIMIT 3",
        ) {
            if let Ok(fts_rows) = fts_stmt.query_map(params![workspace_id, fts_query], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                ))
            }) {
                for r in fts_rows.flatten() {
                    let (category, title, filename, preview) = r;
                    if preview.trim().is_empty() {
                        continue;
                    }
                    let excerpt = if preview.chars().count() > 600 {
                        let truncated: String = preview.chars().take(600).collect();
                        format!("{truncated}...")
                    } else {
                        preview
                    };
                    matched_sections.push(format!(
                        "[{category}: {filename} - {title}]\n{excerpt}"
                    ));
                    sources.push(filename);
                }
            }
        }
    }

    // Fallback to bounded workspace scan only if FTS5 returned no matches
    if matched_sections.is_empty() {
        let mut stmt = conn
            .prepare(
                "SELECT category, title, filename, content_preview
                 FROM workspace_materials
                 WHERE workspace_id = ?1
                 ORDER BY created_at DESC
                 LIMIT 100",
            )
            .map_err(|e| format!("Failed to query workspace materials: {e}"))?;

        let rows = stmt
            .query_map(params![workspace_id], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                ))
            })
            .map_err(|e| format!("Failed to read materials: {e}"))?;

        let query_terms: Vec<&str> = clean_query
            .split_whitespace()
            .filter(|t| t.len() > 1)
            .collect();

        for r in rows {
            let (category, title, filename, preview) =
                r.map_err(|e| format!("Error reading material row: {e}"))?;

            if preview.trim().is_empty() {
                continue;
            }

            let preview_lower = preview.to_lowercase();
            let title_lower = title.to_lowercase();

            let mut score = 0;
            for term in &query_terms {
                if title_lower.contains(term) || filename.to_lowercase().contains(term) {
                    score += 3;
                }
                if preview_lower.contains(term) {
                    score += 1;
                }
            }

            if score > 0 || query_terms.is_empty() || matched_sections.is_empty() {
                let excerpt = if preview.chars().count() > 600 {
                    let truncated: String = preview.chars().take(600).collect();
                    format!("{truncated}...")
                } else {
                    preview
                };

                matched_sections.push(format!(
                    "[{category}: {filename} - {title}]\n{excerpt}"
                ));
                sources.push(filename);

                if matched_sections.len() >= 3 {
                    break;
                }
            }
        }
    }

    let available = !matched_sections.is_empty();
    let context = matched_sections.join("\n\n---\n\n");

    Ok(WorkspaceContextResult {
        available,
        context,
        sources,
    })
}

#[tauri::command]
pub fn db_delete_workspace_material(
    state: State<'_, DbState>,
    workspace_id: String,
    material_id: String,
) -> Result<(), String> {
    validate_safe_id(&workspace_id)?;
    validate_safe_id(&material_id)?;

    let filename: Option<String> = {
        let conn = state
            .conn
            .lock()
            .map_err(|_| "Database lock poisoned".to_string())?;

        let existing_filename: Option<String> = conn
            .query_row(
                "SELECT filename FROM workspace_materials WHERE id = ?1 AND workspace_id = ?2",
                params![material_id, workspace_id],
                |r| r.get(0),
            )
            .ok();

        conn.execute(
            "DELETE FROM workspace_materials WHERE id = ?1 AND workspace_id = ?2",
            params![material_id, workspace_id],
        )
        .map_err(|e| format!("Failed to delete material: {e}"))?;

        existing_filename
    };

    if let Some(fname) = filename {
        if !fname.contains(['/', '\\']) && !fname.contains("..") {
            let materials_dir = state
                .app_data_dir
                .join("workspaces")
                .join(&workspace_id)
                .join("materials");
            let prefixed = materials_dir.join(format!("{material_id}_{fname}"));
            let legacy = materials_dir.join(&fname);
            if prefixed.is_file() {
                let _ = fs::remove_file(&prefixed);
            } else if legacy.is_file() {
                let _ = fs::remove_file(&legacy);
            }
        }
    }

    Ok(())
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlatformStats {
    pub total_conversations: i64,
    pub total_messages: i64,
    pub total_workspaces: i64,
    pub total_materials: i64,
    pub total_notes: i64,
    pub db_size_bytes: u64,
    pub db_path: String,
}

#[tauri::command]
pub fn db_get_platform_stats(state: State<'_, DbState>) -> Result<PlatformStats, String> {
    let conn = state
        .conn
        .lock()
        .map_err(|_| "Database lock poisoned".to_string())?;

    let total_conversations: i64 = conn
        .query_row("SELECT count(*) FROM conversations", [], |r| r.get(0))
        .unwrap_or(0);

    let total_messages: i64 = conn
        .query_row("SELECT count(*) FROM messages", [], |r| r.get(0))
        .unwrap_or(0);

    let total_workspaces: i64 = conn
        .query_row("SELECT count(*) FROM workspaces", [], |r| r.get(0))
        .unwrap_or(0);

    let total_materials: i64 = conn
        .query_row("SELECT count(*) FROM workspace_materials", [], |r| r.get(0))
        .unwrap_or(0);

    let total_notes: i64 = conn
        .query_row("SELECT count(*) FROM workspace_notes", [], |r| r.get(0))
        .unwrap_or(0);

    let db_path = state.app_data_dir.join("nexus.db");
    let db_size_bytes = fs::metadata(&db_path)
        .map(|m| m.len())
        .unwrap_or(0);

    Ok(PlatformStats {
        total_conversations,
        total_messages,
        total_workspaces,
        total_materials,
        total_notes,
        db_size_bytes,
        db_path: db_path.to_string_lossy().to_string(),
    })
}

#[tauri::command]
pub fn db_export_backup(state: State<'_, DbState>, destination_path: String) -> Result<String, String> {
    let dest = validate_file_path(&destination_path, &["sqlite", "db", "bak"])?;

    let active_db = state.app_data_dir.join("nexus.db");
    if dest == active_db {
        return Err("Cannot overwrite the active database file directly.".to_string());
    }
    if let (Ok(canon_dest), Ok(canon_active)) = (fs::canonicalize(&dest), fs::canonicalize(&active_db)) {
        if canon_dest == canon_active {
            return Err("Cannot overwrite the active database file directly.".to_string());
        }
    }

    let conn = state
        .conn
        .lock()
        .map_err(|_| "Database lock poisoned".to_string())?;

    if dest.is_file() {
        let _ = fs::remove_file(&dest);
    }

    let dest_str = dest.to_string_lossy().to_string();
    conn.execute("VACUUM INTO ?1", params![dest_str])
        .map_err(|e| format!("VACUUM backup failed: {e}"))?;

    Ok(dest_str)
}

#[tauri::command]
pub fn db_restore_backup(state: State<'_, DbState>, source_path: String) -> Result<String, String> {
    let src = validate_file_path(&source_path, &["sqlite", "db", "bak"])?;
    if !src.is_file() {
        return Err("Selected backup file does not exist.".to_string());
    }

    let mut conn = state
        .conn
        .lock()
        .map_err(|_| "Database lock poisoned".to_string())?;

    {
        let test_conn = Connection::open(&src)
            .map_err(|e| format!("Invalid SQLite backup file: {e}"))?;
        let _test: i64 = test_conn
            .query_row("SELECT count(*) FROM conversations", [], |r| r.get(0))
            .map_err(|e| format!("Backup file is missing Nexus tables: {e}"))?;
    }

    let db_path = state.app_data_dir.join("nexus.db");
    let safety_backup_path = state.app_data_dir.join("nexus.db.pre_restore.bak");
    let wal_path = state.app_data_dir.join("nexus.db-wal");
    let shm_path = state.app_data_dir.join("nexus.db-shm");

    // Flush any pending WAL frames before releasing the connection
    let _ = conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);");

    *conn = Connection::open_in_memory().map_err(|e| format!("Error releasing database: {e}"))?;

    // Remove stale WAL/SHM sidecar files so the restored DB does not replay old journal frames
    if wal_path.exists() {
        let _ = fs::remove_file(&wal_path);
    }
    if shm_path.exists() {
        let _ = fs::remove_file(&shm_path);
    }

    // Move existing database to a safety rollback file before overwriting
    let had_existing = db_path.exists();
    if had_existing {
        if safety_backup_path.exists() {
            let _ = fs::remove_file(&safety_backup_path);
        }
        fs::rename(&db_path, &safety_backup_path)
            .map_err(|e| format!("Failed to stage pre-restore safety backup: {e}"))?;
    }

    let restore_result: Result<Connection, String> = (|| {
        fs::copy(&src, &db_path)
            .map_err(|e| format!("Failed to restore database file: {e}"))?;
        let new_conn = Connection::open(&db_path)
            .map_err(|e| format!("Failed to reopen restored database: {e}"))?;
        ensure_schema(&new_conn)?;
        Ok(new_conn)
    })();

    match restore_result {
        Ok(new_conn) => {
            *conn = new_conn;
            if safety_backup_path.exists() {
                let _ = fs::remove_file(&safety_backup_path);
            }
            Ok("Database restored successfully.".to_string())
        }
        Err(err_msg) => {
            // Roll back to pre-restore safety copy if anything failed
            if had_existing && safety_backup_path.exists() {
                let _ = fs::remove_file(&db_path);
                let _ = fs::rename(&safety_backup_path, &db_path);
                if let Ok(reopened) = Connection::open(&db_path) {
                    let _ = reopened.execute_batch("PRAGMA foreign_keys = ON; PRAGMA journal_mode = WAL;");
                    *conn = reopened;
                }
            }
            Err(err_msg)
        }
    }
}

#[derive(Serialize)]
struct ExportJsonPayload<'a> {
    conversation_id: &'a str,
    title: &'a str,
    model: &'a str,
    created_at: i64,
    exported_at: i64,
    messages: Vec<StoredMessage>,
}

#[tauri::command]
pub fn db_export_conversation_markdown(
    state: State<'_, DbState>,
    conversation_id: String,
    destination_path: String,
) -> Result<(), String> {
    let dest = validate_file_path(&destination_path, &["md", "markdown"])?;
    let conn = state
        .conn
        .lock()
        .map_err(|_| "Database lock poisoned".to_string())?;

    let (title, model, created_at): (String, String, i64) = conn
        .query_row(
            "SELECT title, model, created_at FROM conversations WHERE id = ?1",
            params![conversation_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .map_err(|e| format!("Conversation not found: {e}"))?;

    let mut stmt = conn
        .prepare("SELECT id, conversation_id, role, content, model, created_at FROM messages WHERE conversation_id = ?1 ORDER BY id ASC")
        .map_err(|e| format!("Query error: {e}"))?;

    let msgs: Vec<StoredMessage> = stmt
        .query_map(params![conversation_id], |row| {
            Ok(StoredMessage {
                id: row.get(0)?,
                conversation_id: row.get(1)?,
                role: row.get(2)?,
                content: row.get(3)?,
                model: row.get(4)?,
                created_at: row.get(5)?,
            })
        })
        .map_err(|e| format!("Failed to map messages: {e}"))?
        .filter_map(Result::ok)
        .collect();

    let mut md = String::new();
    md.push_str(&format!("# {}\n\n", title));
    md.push_str(&format!("- **Model:** {}\n", model));
    md.push_str(&format!("- **Created Timestamp:** {}\n", created_at));
    md.push_str(&format!("- **Exported from:** Nexus AI Platform\n\n---\n\n"));

    for msg in msgs {
        let speaker = if msg.role == "user" {
            "### You".to_string()
        } else {
            format!("### Nexus ({})", msg.model.unwrap_or_else(|| model.clone()))
        };

        md.push_str(&format!("{}\n\n{}\n\n---\n\n", speaker, msg.content.trim()));
    }

    fs::write(&dest, md)
        .map_err(|e| format!("Failed to write export file: {e}"))?;

    Ok(())
}

#[tauri::command]
pub fn db_export_conversation_json(
    state: State<'_, DbState>,
    conversation_id: String,
    destination_path: String,
) -> Result<(), String> {
    let dest = validate_file_path(&destination_path, &["json"])?;
    let conn = state
        .conn
        .lock()
        .map_err(|_| "Database lock poisoned".to_string())?;

    let (title, model, created_at): (String, String, i64) = conn
        .query_row(
            "SELECT title, model, created_at FROM conversations WHERE id = ?1",
            params![conversation_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .map_err(|e| format!("Conversation not found: {e}"))?;

    let mut stmt = conn
        .prepare("SELECT id, conversation_id, role, content, model, created_at FROM messages WHERE conversation_id = ?1 ORDER BY id ASC")
        .map_err(|e| format!("Query error: {e}"))?;

    let msgs: Vec<StoredMessage> = stmt
        .query_map(params![conversation_id], |row| {
            Ok(StoredMessage {
                id: row.get(0)?,
                conversation_id: row.get(1)?,
                role: row.get(2)?,
                content: row.get(3)?,
                model: row.get(4)?,
                created_at: row.get(5)?,
            })
        })
        .map_err(|e| format!("Failed to map messages: {e}"))?
        .filter_map(Result::ok)
        .collect();

    let payload = ExportJsonPayload {
        conversation_id: &conversation_id,
        title: &title,
        model: &model,
        created_at,
        exported_at: now_ms(),
        messages: msgs,
    };

    let json = serde_json::to_string_pretty(&payload)
        .map_err(|e| format!("JSON serialization failed: {e}"))?;

    fs::write(&dest, json)
        .map_err(|e| format!("Failed to write export file: {e}"))?;

    Ok(())
}

#[tauri::command]
pub fn db_reset_sandbox() -> Result<String, String> {
    let app_dir = std::env::var("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir())
        .join("nexus");
    let sandbox_path = app_dir.join("sandbox.db");

    if sandbox_path.exists() {
        let _ = fs::remove_file(&sandbox_path);
    }

    Ok("Sandbox database reset successfully.".to_string())
}

// ================= TELEMETRY & USAGE =================

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DailyUsageSummary {
    pub date: String,
    pub prompts: i64,
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
    pub total_tokens: i64,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TelemetryStats {
    pub today_tokens: i64,
    pub today_prompts: i64,
    pub today_prompt_tokens: i64,
    pub today_completion_tokens: i64,
    pub all_time_tokens: i64,
    pub all_time_prompts: i64,
    pub all_time_prompt_tokens: i64,
    pub all_time_completion_tokens: i64,
    pub daily_history: Vec<DailyUsageSummary>,
}

#[tauri::command]
pub fn db_record_telemetry(
    state: State<'_, DbState>,
    model: String,
    prompt_tokens: i64,
    completion_tokens: i64,
    duration_ms: i64,
    tokens_per_sec: Option<f64>,
    local_date: Option<String>,
) -> Result<(), String> {
    let conn = state
        .conn
        .lock()
        .map_err(|_| "Database lock poisoned".to_string())?;

    let ts = now_ms();
    let total_tokens = prompt_tokens + completion_tokens;

    let date = match local_date {
        Some(d) if !d.trim().is_empty() => d,
        _ => conn
            .query_row(
                "SELECT date(?1 / 1000, 'unixepoch', 'localtime')",
                params![ts],
                |r| r.get(0),
            )
            .unwrap_or_else(|_| "2026-11-20".to_string()),
    };

    conn.execute(
        "INSERT INTO usage_records (timestamp, date, model, prompt_tokens, completion_tokens, total_tokens, duration_ms, tokens_per_sec)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            ts,
            date,
            model,
            prompt_tokens,
            completion_tokens,
            total_tokens,
            duration_ms,
            tokens_per_sec
        ],
    )
    .map_err(|e| format!("Failed to record telemetry: {e}"))?;

    Ok(())
}

#[tauri::command]
pub fn db_get_telemetry_stats(
    state: State<'_, DbState>,
    local_date: Option<String>,
) -> Result<TelemetryStats, String> {
    let conn = state
        .conn
        .lock()
        .map_err(|_| "Database lock poisoned".to_string())?;

    let today_date: String = match local_date {
        Some(d) if !d.trim().is_empty() => d,
        _ => {
            let ts = now_ms();
            conn.query_row(
                "SELECT date(?1 / 1000, 'unixepoch', 'localtime')",
                params![ts],
                |r| r.get(0),
            )
            .unwrap_or_else(|_| "2026-11-20".to_string())
        }
    };

    let (today_tokens, today_prompts, today_prompt_tokens, today_completion_tokens): (
        i64,
        i64,
        i64,
        i64,
    ) = conn
        .query_row(
            "SELECT
                coalesce(sum(total_tokens), 0),
                coalesce(count(*), 0),
                coalesce(sum(prompt_tokens), 0),
                coalesce(sum(completion_tokens), 0)
             FROM usage_records
             WHERE date = ?1",
            params![today_date],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .unwrap_or((0, 0, 0, 0));

    let (all_time_tokens, all_time_prompts, all_time_prompt_tokens, all_time_completion_tokens): (
        i64,
        i64,
        i64,
        i64,
    ) = conn
        .query_row(
            "SELECT
                coalesce(sum(total_tokens), 0),
                coalesce(count(*), 0),
                coalesce(sum(prompt_tokens), 0),
                coalesce(sum(completion_tokens), 0)
             FROM usage_records",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .unwrap_or((0, 0, 0, 0));

    let mut stmt = conn
        .prepare(
            "SELECT
                date,
                count(*) as prompts,
                coalesce(sum(prompt_tokens), 0) as p_tokens,
                coalesce(sum(completion_tokens), 0) as c_tokens,
                coalesce(sum(total_tokens), 0) as t_tokens
             FROM usage_records
             GROUP BY date
             ORDER BY date DESC
             LIMIT 14",
        )
        .map_err(|e| format!("Failed to prepare daily query: {e}"))?;

    let daily_history: Vec<DailyUsageSummary> = stmt
        .query_map([], |row| {
            Ok(DailyUsageSummary {
                date: row.get(0)?,
                prompts: row.get(1)?,
                prompt_tokens: row.get(2)?,
                completion_tokens: row.get(3)?,
                total_tokens: row.get(4)?,
            })
        })
        .map_err(|e| format!("Failed to map daily query: {e}"))?
        .filter_map(Result::ok)
        .collect();

    Ok(TelemetryStats {
        today_tokens,
        today_prompts,
        today_prompt_tokens,
        today_completion_tokens,
        all_time_tokens,
        all_time_prompts,
        all_time_prompt_tokens,
        all_time_completion_tokens,
        daily_history,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_safe_id() {
        assert!(validate_safe_id("ws_1732123456_1").is_ok());
        assert!(validate_safe_id("mat-abc-123").is_ok());
        assert!(validate_safe_id("").is_err());
        assert!(validate_safe_id("../etc/passwd").is_err());
        assert!(validate_safe_id("ws\\evil").is_err());
        assert!(validate_safe_id("ws/evil").is_err());
    }

    #[test]
    fn test_validate_file_path_blocks_traversal_and_bad_ext() {
        assert!(validate_file_path("C:\\backups\\nexus.sqlite", &["sqlite", "db"]).is_ok());
        assert!(validate_file_path("..\\secret.sqlite", &["sqlite"]).is_err());
        assert!(validate_file_path("C:\\backups\\notes.exe", &["md", "json"]).is_err());
        assert!(validate_file_path("", &["md"]).is_err());
    }

    #[test]
    fn test_ensure_schema_initializes_fts5_and_syncs_triggers() {
        let conn = Connection::open_in_memory().expect("in-memory sqlite");
        assert!(ensure_schema(&conn).is_ok());
        conn.execute(
            "INSERT INTO workspaces (id, name, description, created_at, updated_at) VALUES ('ws_1', 'DBMS', '', 1, 1)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO workspace_materials (id, workspace_id, category, title, filename, content_preview, created_at)
             VALUES ('mat_1', 'ws_1', 'lectures', 'Normalization', 'lec1.pdf', 'Third normal form removes transitive dependencies', 1)",
            [],
        )
        .unwrap();

        let matched: i64 = conn
            .query_row(
                "SELECT count(*) FROM workspace_materials_fts WHERE workspace_id = 'ws_1' AND workspace_materials_fts MATCH '\"transitive\"'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(matched, 1);
    }
}
