use rusqlite::{params, Connection, OpenFlags};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::State;

static ID_COUNTER: AtomicU64 = AtomicU64::new(1);

pub struct DbState {
    pub conn: Arc<Mutex<Connection>>,
    pub read_conn: Arc<Mutex<Connection>>,
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

            CREATE TABLE IF NOT EXISTS app_settings (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL,
                updated_at INTEGER NOT NULL
            );
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

fn open_readonly_connection(db_path: &std::path::Path) -> Result<Connection, String> {
    let conn = Connection::open_with_flags(db_path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|e| format!("Failed to open read-only SQLite connection: {e}"))?;
    let _ = conn.busy_timeout(Duration::from_secs(5));
    let _ = conn.execute_batch("PRAGMA query_only = ON;");
    Ok(conn)
}

impl DbState {
    pub fn initialize(app_data_dir: PathBuf) -> Result<Self, String> {
        fs::create_dir_all(&app_data_dir)
            .map_err(|e| format!("Failed to create app data directory: {e}"))?;

        let db_path = app_data_dir.join("nexus.db");
        let conn = Connection::open(&db_path)
            .map_err(|e| format!("Failed to open SQLite database: {e}"))?;
        let _ = conn.busy_timeout(Duration::from_secs(5));

        ensure_schema(&conn)?;
        let read_conn = open_readonly_connection(&db_path)?;

        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
            read_conn: Arc::new(Mutex::new(read_conn)),
            app_data_dir,
        })
    }
}

// ================= WORKSPACES =================

#[tauri::command]
pub fn db_list_workspaces(state: State<'_, DbState>) -> Result<Vec<Workspace>, String> {
    let conn = state
        .read_conn
        .lock()
        .map_err(|_| "Database read lock poisoned".to_string())?;

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
        let offline_key = format!("ws_offline_{id}");
        let _ = conn.execute("DELETE FROM app_settings WHERE key = ?1", params![offline_key]);
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
        .read_conn
        .lock()
        .map_err(|_| "Database read lock poisoned".to_string())?;

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

fn fetch_messages_page(
    conn: &Connection,
    conversation_id: &str,
    before_id: Option<i64>,
    limit: i64,
) -> Result<Vec<StoredMessage>, String> {
    let clamped_limit = limit.clamp(1, 500);
    let mut messages = Vec::new();

    if let Some(cursor_id) = before_id {
        let mut stmt = conn
            .prepare(
                "SELECT id, conversation_id, role, content, model, created_at
                 FROM messages
                 WHERE conversation_id = ?1 AND id < ?2
                 ORDER BY id DESC
                 LIMIT ?3",
            )
            .map_err(|e| format!("Failed to prepare paginated messages query: {e}"))?;

        let rows = stmt
            .query_map(params![conversation_id, cursor_id, clamped_limit], |row| {
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

        for row in rows {
            messages.push(row.map_err(|e| format!("Failed to read message row: {e}"))?);
        }
    } else {
        let mut stmt = conn
            .prepare(
                "SELECT id, conversation_id, role, content, model, created_at
                 FROM messages
                 WHERE conversation_id = ?1
                 ORDER BY id DESC
                 LIMIT ?2",
            )
            .map_err(|e| format!("Failed to prepare messages query: {e}"))?;

        let rows = stmt
            .query_map(params![conversation_id, clamped_limit], |row| {
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

        for row in rows {
            messages.push(row.map_err(|e| format!("Failed to read message row: {e}"))?);
        }
    }

    // Reverse newest-first slice back to chronological order for display
    messages.reverse();
    Ok(messages)
}

#[tauri::command]
pub fn db_get_messages(
    state: State<'_, DbState>,
    conversation_id: String,
    before_id: Option<i64>,
    limit: Option<i64>,
) -> Result<Vec<StoredMessage>, String> {
    let conn = state
        .read_conn
        .lock()
        .map_err(|_| "Database read lock poisoned".to_string())?;

    fetch_messages_page(&conn, &conversation_id, before_id, limit.unwrap_or(500))
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
        .read_conn
        .lock()
        .map_err(|_| "Database read lock poisoned".to_string())?;

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
        .read_conn
        .lock()
        .map_err(|_| "Database read lock poisoned".to_string())?;

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

const MAX_IMPORT_FILE_BYTES: u64 = 32 * 1024 * 1024;

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

    let meta = fs::metadata(&src)
        .map_err(|e| format!("Unable to inspect source file metadata: {e}"))?;
    if meta.len() > MAX_IMPORT_FILE_BYTES {
        return Err(format!(
            "File exceeds the maximum allowed import size ({} MB).",
            MAX_IMPORT_FILE_BYTES / (1024 * 1024)
        ));
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
    let (filename, title, preview, target_file) = tokio::task::spawn_blocking(
        move || -> Result<(String, String, String, PathBuf), String> {
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
                        let _ = file.take(256 * 1024).read_to_end(&mut buf);
                        let text = String::from_utf8_lossy(&buf);
                        let truncated: String = text.chars().take(16000).collect();
                        if text.chars().count() > 16000 {
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
                    for (&page_num, _) in pages.iter().take(25) {
                        if let Ok(page_text) = doc.extract_text(&[page_num]) {
                            text.push_str(&page_text);
                            text.push('\n');
                        }
                    }
                    if text.trim().is_empty() {
                        "PDF uploaded (visual/scanned PDF - text extraction will require OCR in indexing).".to_string()
                    } else {
                        let truncated: String = text.chars().take(16000).collect();
                        if text.chars().count() > 16000 {
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

        Ok((filename, title, preview, target_file))
    },
    )
    .await
    .map_err(|e| format!("File import worker panicked: {e}"))??;

    let now = now_ms();
    let conn = state
        .conn
        .lock()
        .map_err(|_| "Database lock poisoned".to_string())?;

    if let Err(e) = conn.execute(
        "INSERT INTO workspace_materials (id, workspace_id, category, title, filename, content_preview, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![mat_id, workspace_id, category, title, filename, preview, now],
    ) {
        // Clean up copied file if database registration fails (e.g. workspace concurrently deleted)
        let _ = fs::remove_file(&target_file);
        return Err(format!("Failed to add material: {e}"));
    }

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
        .read_conn
        .lock()
        .map_err(|_| "Database read lock poisoned".to_string())?;

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
        .read_conn
        .lock()
        .map_err(|_| "Database read lock poisoned".to_string())?;

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

fn scrub_backup_secrets(backup_path: &std::path::Path) -> Result<(), String> {
    if !backup_path.is_file() {
        return Err("Backup file missing before secret scrubbing.".to_string());
    }
    let backup_conn = Connection::open(backup_path)
        .map_err(|e| format!("Failed to open backup snapshot for secret scrubbing: {e}"))?;
    backup_conn
        .execute(
            "DELETE FROM app_settings WHERE key = 'groq_api_key' OR key LIKE '%_api_key%'",
            [],
        )
        .map_err(|e| format!("Failed to scrub API keys from backup snapshot: {e}"))?;
    Ok(())
}

#[tauri::command]
pub async fn db_export_backup(
    state: State<'_, DbState>,
    destination_path: String,
) -> Result<String, String> {
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

    let dest_str = dest.to_string_lossy().to_string();
    let active_db_for_worker = active_db.clone();
    let dest_for_worker = dest.clone();
    let dest_str_for_worker = dest_str.clone();

    // Use a dedicated short-lived WAL snapshot connection so VACUUM INTO never blocks main DB mutex reads/writes
    tokio::task::spawn_blocking(move || -> Result<(), String> {
        {
            let snapshot_conn = Connection::open(&active_db_for_worker)
                .map_err(|e| format!("Failed to open database snapshot connection: {e}"))?;
            let _ = snapshot_conn.busy_timeout(std::time::Duration::from_secs(5));

            if dest_for_worker.is_file() {
                fs::remove_file(&dest_for_worker)
                    .map_err(|e| format!("Failed to replace existing backup file: {e}"))?;
            }

            snapshot_conn
                .execute("VACUUM INTO ?1", params![dest_str_for_worker])
                .map_err(|e| format!("VACUUM backup failed: {e}"))?;
        }

        // Fail closed: remove the backup snapshot if API key scrubbing fails
        if let Err(scrub_err) = scrub_backup_secrets(&dest_for_worker) {
            let _ = fs::remove_file(&dest_for_worker);
            return Err(format!(
                "Backup aborted because secret scrubbing failed ({scrub_err})."
            ));
        }

        Ok(())
    })
    .await
    .map_err(|e| format!("Backup worker panicked: {e}"))??;

    Ok(dest_str)
}

#[tauri::command]
pub async fn db_restore_backup(
    state: State<'_, DbState>,
    source_path: String,
) -> Result<String, String> {
    let src = validate_file_path(&source_path, &["sqlite", "db", "bak"])?;
    if !src.is_file() {
        return Err("Selected backup file does not exist.".to_string());
    }

    let active_db = state.app_data_dir.join("nexus.db");
    if src == active_db {
        return Err("Cannot restore directly from the active database file.".to_string());
    }
    if let (Ok(canon_src), Ok(canon_active)) = (fs::canonicalize(&src), fs::canonicalize(&active_db)) {
        if canon_src == canon_active {
            return Err("Cannot restore directly from the active database file.".to_string());
        }
    }

    let conn_arc = Arc::clone(&state.conn);
    let read_conn_arc = Arc::clone(&state.read_conn);
    let app_data_dir = state.app_data_dir.clone();

    tokio::task::spawn_blocking(move || -> Result<String, String> {
        let mut conn = conn_arc
            .lock()
            .map_err(|_| "Database lock poisoned".to_string())?;
        let mut read_conn = read_conn_arc
            .lock()
            .map_err(|_| "Database read lock poisoned".to_string())?;

        {
            let test_conn = Connection::open(&src)
                .map_err(|e| format!("Invalid SQLite backup file: {e}"))?;
            let _test: i64 = test_conn
                .query_row("SELECT count(*) FROM conversations", [], |r| r.get(0))
                .map_err(|e| format!("Backup file is missing Nexus tables: {e}"))?;
        }

        let db_path = app_data_dir.join("nexus.db");
        let safety_backup_path = app_data_dir.join("nexus.db.pre_restore.bak");
        let wal_path = app_data_dir.join("nexus.db-wal");
        let shm_path = app_data_dir.join("nexus.db-shm");

        // Release read-only handle first so Windows file locks are cleared before checkpoint & rename
        *read_conn = Connection::open_in_memory()
            .map_err(|e| format!("Error releasing read connection: {e}"))?;

        // Flush any pending WAL frames before releasing the writer connection
        let _ = conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);");

        *conn = Connection::open_in_memory()
            .map_err(|e| format!("Error releasing database: {e}"))?;

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

        let restore_result: Result<(Connection, Connection), String> = (|| {
            fs::copy(&src, &db_path)
                .map_err(|e| format!("Failed to restore database file: {e}"))?;
            let new_conn = Connection::open(&db_path)
                .map_err(|e| format!("Failed to reopen restored database: {e}"))?;
            let _ = new_conn.busy_timeout(Duration::from_secs(5));
            ensure_schema(&new_conn)?;
            let new_read_conn = open_readonly_connection(&db_path)?;
            Ok((new_conn, new_read_conn))
        })();

        match restore_result {
            Ok((new_conn, new_read_conn)) => {
                *conn = new_conn;
                *read_conn = new_read_conn;
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
                        let _ = reopened.busy_timeout(Duration::from_secs(5));
                        let _ = reopened
                            .execute_batch("PRAGMA foreign_keys = ON; PRAGMA journal_mode = WAL;");
                        *conn = reopened;
                        if let Ok(reopened_read) = open_readonly_connection(&db_path) {
                            *read_conn = reopened_read;
                        }
                    }
                }
                Err(err_msg)
            }
        }
    })
    .await
    .map_err(|e| format!("Database restore worker panicked: {e}"))?
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
pub async fn db_export_conversation_markdown(
    state: State<'_, DbState>,
    conversation_id: String,
    destination_path: String,
) -> Result<(), String> {
    let dest = validate_file_path(&destination_path, &["md", "markdown"])?;
    let (title, model, created_at, msgs) = {
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

        (title, model, created_at, msgs)
    };

    // Format and write file off the main thread after releasing the DB lock
    tokio::task::spawn_blocking(move || -> Result<(), String> {
        let mut md = String::new();
        md.push_str(&format!("# {}\n\n", title));
        md.push_str(&format!("- **Model:** {}\n", model));
        md.push_str(&format!("- **Created Timestamp:** {}\n", created_at));
        md.push_str("- **Exported from:** Nexus AI Platform\n\n---\n\n");

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
    })
    .await
    .map_err(|e| format!("Markdown export worker panicked: {e}"))??;

    Ok(())
}

#[tauri::command]
pub async fn db_export_conversation_json(
    state: State<'_, DbState>,
    conversation_id: String,
    destination_path: String,
) -> Result<(), String> {
    let dest = validate_file_path(&destination_path, &["json"])?;
    let (title, model, created_at, msgs) = {
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

        (title, model, created_at, msgs)
    };

    tokio::task::spawn_blocking(move || -> Result<(), String> {
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
    })
    .await
    .map_err(|e| format!("JSON export worker panicked: {e}"))??;

    Ok(())
}

#[tauri::command]
pub fn db_reset_sandbox(state: State<'_, DbState>) -> Result<String, String> {
    let sandbox_path = state.app_data_dir.join("sandbox.db");
    if sandbox_path.exists() {
        let _ = fs::remove_file(&sandbox_path);
    }

    // Also clean up any legacy sandbox path from pre-v1 builds
    if let Ok(local_app_data) = std::env::var("LOCALAPPDATA") {
        let legacy_path = PathBuf::from(local_app_data).join("nexus").join("sandbox.db");
        if legacy_path.exists() {
            let _ = fs::remove_file(&legacy_path);
        }
    }

    Ok("Sandbox database reset successfully.".to_string())
}

// Howard Hinnant civil-from-days algorithm for dependency-free dynamic UTC date fallback
fn utc_date_from_ms(ts_ms: i64) -> String {
    let z = (ts_ms / 1000).div_euclid(86_400) + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if m <= 2 { y + 1 } else { y };
    format!("{year:04}-{m:02}-{d:02}")
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
            .unwrap_or_else(|_| utc_date_from_ms(ts)),
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
        .read_conn
        .lock()
        .map_err(|_| "Database read lock poisoned".to_string())?;

    let today_date: String = match local_date {
        Some(d) if !d.trim().is_empty() => d,
        _ => {
            let ts = now_ms();
            conn.query_row(
                "SELECT date(?1 / 1000, 'unixepoch', 'localtime')",
                params![ts],
                |r| r.get(0),
            )
            .unwrap_or_else(|_| utc_date_from_ms(ts))
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

// Validates that a setting key is in the explicit allowlist and value is within safe bounds
fn validate_setting_key_and_value(key: &str, value: &str) -> Result<(), String> {
    let is_allowed_key = matches!(key, "groq_api_key" | "cloud_auto_fallback")
        || (key.starts_with("ws_offline_") && validate_safe_id(&key["ws_offline_".len()..]).is_ok());

    if !is_allowed_key {
        return Err(format!("Setting key '{key}' is not permitted."));
    }

    if value.len() > 2048 {
        return Err("Setting value exceeds maximum allowed length (2048 bytes).".to_string());
    }

    Ok(())
}

// Masks secret API keys so raw credentials never cross the IPC boundary back to the webview
fn mask_secret_value(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let chars: Vec<char> = trimmed.chars().collect();
    if chars.len() <= 8 {
        Some("••••••••".to_string())
    } else {
        let prefix: String = chars[..4].iter().collect();
        let suffix: String = chars[chars.len() - 4..].iter().collect();
        Some(format!("{prefix}••••••••{suffix}"))
    }
}

fn is_secret_setting_key(key: &str) -> bool {
    key == "groq_api_key" || key.ends_with("_api_key")
}

#[cfg(target_os = "windows")]
mod secret_store {
    use std::ffi::c_void;
    use std::ptr;

    const DPAPI_PREFIX: &str = "dpapi:";
    const CRYPTPROTECT_UI_FORBIDDEN: u32 = 0x1;

    #[repr(C)]
    struct DataBlob {
        cb_data: u32,
        pb_data: *mut u8,
    }

    #[link(name = "crypt32")]
    unsafe extern "system" {
        fn CryptProtectData(
            p_data_in: *const DataBlob,
            sz_data_descr: *const u16,
            p_optional_entropy: *const DataBlob,
            pv_reserved: *mut c_void,
            p_prompt_struct: *mut c_void,
            dw_flags: u32,
            p_data_out: *mut DataBlob,
        ) -> i32;

        fn CryptUnprotectData(
            p_data_in: *const DataBlob,
            ppsz_data_descr: *mut *mut u16,
            p_optional_entropy: *const DataBlob,
            pv_reserved: *mut c_void,
            p_prompt_struct: *mut c_void,
            dw_flags: u32,
            p_data_out: *mut DataBlob,
        ) -> i32;
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn LocalFree(h_mem: *mut c_void) -> *mut c_void;
    }

    fn hex_encode(bytes: &[u8]) -> String {
        let mut out = String::with_capacity(bytes.len() * 2);
        for b in bytes {
            use std::fmt::Write;
            let _ = write!(&mut out, "{b:02x}");
        }
        out
    }

    fn hex_decode(hex: &str) -> Option<Vec<u8>> {
        if hex.len() % 2 != 0 {
            return None;
        }
        let mut out = Vec::with_capacity(hex.len() / 2);
        for pair in hex.as_bytes().chunks_exact(2) {
            let s = std::str::from_utf8(pair).ok()?;
            out.push(u8::from_str_radix(s, 16).ok()?);
        }
        Some(out)
    }

    /// Protects a secret at rest with Windows DPAPI (scoped to the current OS user profile).
    pub fn encrypt_secret(plaintext: &str) -> Result<String, String> {
        if plaintext.is_empty() {
            return Ok(String::new());
        }
        let mut input = plaintext.as_bytes().to_vec();
        let in_blob = DataBlob {
            cb_data: input.len() as u32,
            pb_data: input.as_mut_ptr(),
        };
        let mut out_blob = DataBlob {
            cb_data: 0,
            pb_data: ptr::null_mut(),
        };

        let ok = unsafe {
            CryptProtectData(
                &in_blob,
                ptr::null(),
                ptr::null(),
                ptr::null_mut(),
                ptr::null_mut(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut out_blob,
            )
        };

        if ok == 0 || out_blob.pb_data.is_null() {
            return Err("OS credential encryption (DPAPI) failed.".to_string());
        }

        let encrypted = unsafe {
            let slice = std::slice::from_raw_parts(out_blob.pb_data, out_blob.cb_data as usize);
            let copied = slice.to_vec();
            LocalFree(out_blob.pb_data as *mut c_void);
            copied
        };

        Ok(format!("{DPAPI_PREFIX}{}", hex_encode(&encrypted)))
    }

    /// Decrypts a DPAPI-protected value, falling back transparently for legacy plaintext rows.
    pub fn decrypt_secret(stored: &str) -> Option<String> {
        let Some(hex_payload) = stored.strip_prefix(DPAPI_PREFIX) else {
            return Some(stored.to_string());
        };
        let mut cipher = hex_decode(hex_payload)?;
        let in_blob = DataBlob {
            cb_data: cipher.len() as u32,
            pb_data: cipher.as_mut_ptr(),
        };
        let mut out_blob = DataBlob {
            cb_data: 0,
            pb_data: ptr::null_mut(),
        };

        let ok = unsafe {
            CryptUnprotectData(
                &in_blob,
                ptr::null_mut(),
                ptr::null(),
                ptr::null_mut(),
                ptr::null_mut(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut out_blob,
            )
        };

        if ok == 0 || out_blob.pb_data.is_null() {
            return None;
        }

        let decrypted = unsafe {
            let slice = std::slice::from_raw_parts(out_blob.pb_data, out_blob.cb_data as usize);
            let copied = slice.to_vec();
            LocalFree(out_blob.pb_data as *mut c_void);
            copied
        };

        String::from_utf8(decrypted).ok()
    }
}

#[cfg(not(target_os = "windows"))]
mod secret_store {
    pub fn encrypt_secret(plaintext: &str) -> Result<String, String> {
        Ok(plaintext.to_string())
    }

    pub fn decrypt_secret(stored: &str) -> Option<String> {
        Some(stored.to_string())
    }
}

// Internal helper for retrieving a key-value setting directly from SQLite (auto-decrypts API keys)
pub fn get_setting_internal(conn: &Connection, key: &str) -> Option<String> {
    let raw: String = conn
        .query_row(
            "SELECT value FROM app_settings WHERE key = ?1",
            params![key],
            |r| r.get(0),
        )
        .ok()?;
    if is_secret_setting_key(key) {
        secret_store::decrypt_secret(&raw)
    } else {
        Some(raw)
    }
}

// Internal helper for updating or inserting a key-value setting directly in SQLite (encrypts API keys at rest)
pub fn set_setting_internal(conn: &Connection, key: &str, value: &str) -> Result<(), String> {
    validate_setting_key_and_value(key, value)?;
    let stored_value = if is_secret_setting_key(key) {
        secret_store::encrypt_secret(value)?
    } else {
        value.to_string()
    };
    let ts = now_ms();
    conn.execute(
        "INSERT INTO app_settings (key, value, updated_at) VALUES (?1, ?2, ?3)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
        params![key, stored_value, ts],
    )
    .map_err(|e| format!("Failed to save setting '{key}': {e}"))?;
    Ok(())
}

#[tauri::command]
pub async fn db_get_setting(
    state: State<'_, DbState>,
    key: String,
) -> Result<Option<String>, String> {
    validate_setting_key_and_value(&key, "")?;
    let conn = state
        .read_conn
        .lock()
        .map_err(|_| "Database read lock poisoned".to_string())?;
    let raw = get_setting_internal(&conn, &key);
    if key == "groq_api_key" || key.ends_with("_api_key") {
        Ok(raw.and_then(|v| mask_secret_value(&v)))
    } else {
        Ok(raw)
    }
}

#[tauri::command]
pub async fn db_set_setting(
    state: State<'_, DbState>,
    key: String,
    value: String,
) -> Result<(), String> {
    validate_setting_key_and_value(&key, &value)?;
    // Ignore no-op saves of an already-masked key string from the UI
    if (key == "groq_api_key" || key.ends_with("_api_key")) && value.contains('•') {
        return Ok(());
    }
    let conn = state
        .conn
        .lock()
        .map_err(|_| "Database lock poisoned".to_string())?;
    set_setting_internal(&conn, &key, &value)
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

    #[test]
    fn test_app_settings_upsert_and_retrieve() {
        let conn = Connection::open_in_memory().expect("in-memory sqlite");
        assert!(ensure_schema(&conn).is_ok());

        assert_eq!(get_setting_internal(&conn, "groq_api_key"), None);
        assert!(set_setting_internal(&conn, "groq_api_key", "gsk_test123").is_ok());
        assert_eq!(
            get_setting_internal(&conn, "groq_api_key"),
            Some("gsk_test123".to_string())
        );

        // Test update
        assert!(set_setting_internal(&conn, "groq_api_key", "gsk_updated456").is_ok());
        assert_eq!(
            get_setting_internal(&conn, "groq_api_key"),
            Some("gsk_updated456".to_string())
        );

        // Verify allowlist and masking logic
        assert!(set_setting_internal(&conn, "unauthorized_key", "val").is_err());
        assert!(set_setting_internal(&conn, "ws_offline_ws_123", "true").is_ok());
        assert_eq!(
            mask_secret_value("gsk_1234567890abcdef"),
            Some("gsk_••••••••cdef".to_string())
        );
    }

    #[test]
    fn test_scrub_backup_secrets_fail_closed_and_removes_keys() {
        let temp_path = std::env::temp_dir().join(format!(
            "nexus_backup_scrub_test_{}.sqlite",
            generate_id("t")
        ));
        {
            let conn = Connection::open(&temp_path).expect("temp sqlite");
            ensure_schema(&conn).expect("schema");
            set_setting_internal(&conn, "groq_api_key", "gsk_super_secret").unwrap();
            set_setting_internal(&conn, "cloud_auto_fallback", "true").unwrap();
        }

        assert!(scrub_backup_secrets(&temp_path).is_ok());

        {
            let conn = Connection::open(&temp_path).expect("reopen temp sqlite");
            assert_eq!(get_setting_internal(&conn, "groq_api_key"), None);
            assert_eq!(
                get_setting_internal(&conn, "cloud_auto_fallback"),
                Some("true".to_string())
            );
        }

        let _ = fs::remove_file(&temp_path);
        // Fail closed when target file does not exist
        assert!(scrub_backup_secrets(&temp_path).is_err());
    }

    #[test]
    fn test_secret_encrypted_at_rest_and_paginated_messages_newest_first() {
        let conn = Connection::open_in_memory().expect("in-memory sqlite");
        ensure_schema(&conn).expect("schema");

        // Verify secret is encrypted at rest on Windows and transparently decrypted on read
        set_setting_internal(&conn, "groq_api_key", "gsk_dpapi_secret_999").unwrap();
        let raw_stored: String = conn
            .query_row(
                "SELECT value FROM app_settings WHERE key = 'groq_api_key'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        #[cfg(target_os = "windows")]
        {
            assert!(raw_stored.starts_with("dpapi:"));
            assert!(!raw_stored.contains("gsk_dpapi_secret_999"));
        }
        assert_eq!(
            get_setting_internal(&conn, "groq_api_key"),
            Some("gsk_dpapi_secret_999".to_string())
        );

        // Verify newest-first pagination returns latest messages in chronological order
        conn.execute(
            "INSERT INTO conversations (id, title, model, created_at, updated_at) VALUES ('conv_p', 'Long Chat', 'local', 1, 1)",
            [],
        )
        .unwrap();
        for idx in 1..=10 {
            conn.execute(
                "INSERT INTO messages (conversation_id, role, content, created_at) VALUES ('conv_p', 'user', ?1, ?2)",
                params![format!("msg-{idx}"), idx],
            )
            .unwrap();
        }

        let latest_three = fetch_messages_page(&conn, "conv_p", None, 3).unwrap();
        assert_eq!(latest_three.len(), 3);
        assert_eq!(latest_three[0].content, "msg-8");
        assert_eq!(latest_three[2].content, "msg-10");

        let older_three = fetch_messages_page(&conn, "conv_p", Some(latest_three[0].id), 3).unwrap();
        assert_eq!(older_three.len(), 3);
        assert_eq!(older_three[0].content, "msg-5");
        assert_eq!(older_three[2].content, "msg-7");
    }

    #[test]
    fn test_db_state_separates_wal_reader_and_writer() {
        let temp_dir = std::env::temp_dir().join(format!("nexus_wal_rw_test_{}", generate_id("d")));
        let state = DbState::initialize(temp_dir.clone()).expect("init db state");

        {
            let writer = state.conn.lock().unwrap();
            set_setting_internal(&writer, "cloud_auto_fallback", "true").unwrap();
        }
        {
            let reader = state.read_conn.lock().unwrap();
            assert_eq!(
                get_setting_internal(&reader, "cloud_auto_fallback"),
                Some("true".to_string())
            );
            // Read-only connection must reject writes
            assert!(set_setting_internal(&reader, "cloud_auto_fallback", "false").is_err());
        }

        drop(state);
        let _ = fs::remove_dir_all(&temp_dir);
    }
}
