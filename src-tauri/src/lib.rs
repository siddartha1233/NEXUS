mod code_runner;
mod db;
mod memora;
mod ollama;

use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let app_data_dir = app
                .path()
                .app_data_dir()
                .map_err(|e| format!("Could not resolve app data directory: {e}"))?;
            let db_state = db::DbState::initialize(app_data_dir)?;
            app.manage(db_state);
            Ok(())
        })
        .manage(ollama::OllamaState::new())
        .invoke_handler(tauri::generate_handler![
            ollama::ollama_get_models,
            ollama::ollama_get_running_models,
            ollama::ollama_unload_model,
            ollama::ollama_chat_stream,
            ollama::ollama_stop_generation,
            db::db_list_workspaces,
            db::db_create_workspace,
            db::db_rename_workspace,
            db::db_delete_workspace,
            db::db_list_conversations,
            db::db_create_conversation,
            db::db_rename_conversation,
            db::db_delete_conversation,
            db::db_get_messages,
            db::db_add_message,
            db::db_list_workspace_notes,
            db::db_save_workspace_note,
            db::db_delete_workspace_note,
            db::db_list_workspace_materials,
            db::db_add_workspace_material,
            db::db_import_workspace_file,
            db::db_get_workspace_context,
            db::db_delete_workspace_material,
            memora::memora_check_status,
            memora::memora_list_sources,
            memora::memora_get_context,
            memora::memora_memorize,
            memora::memora_ingest,
            code_runner::execute_code_snippet,
            db::db_get_platform_stats,
            db::db_export_backup,
            db::db_restore_backup,
            db::db_export_conversation_markdown,
            db::db_export_conversation_json,
            db::db_reset_sandbox,
            db::db_record_telemetry,
            db::db_get_telemetry_stats
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
