import { invoke } from "@tauri-apps/api/core";
import { open, save } from "@tauri-apps/plugin-dialog";

export interface PlatformStats {
  totalConversations: number;
  totalMessages: number;
  totalWorkspaces: number;
  totalMaterials: number;
  totalNotes: number;
  dbSizeBytes: number;
  dbPath: string;
}

export async function getPlatformStats(): Promise<PlatformStats> {
  try {
    const raw: any = await invoke("db_get_platform_stats");
    return {
      totalConversations: Number(raw?.totalConversations ?? raw?.total_conversations ?? 0),
      totalMessages: Number(raw?.totalMessages ?? raw?.total_messages ?? 0),
      totalWorkspaces: Number(raw?.totalWorkspaces ?? raw?.total_workspaces ?? 0),
      totalMaterials: Number(raw?.totalMaterials ?? raw?.total_materials ?? 0),
      totalNotes: Number(raw?.totalNotes ?? raw?.total_notes ?? 0),
      dbSizeBytes: Number(raw?.dbSizeBytes ?? raw?.db_size_bytes ?? 0),
      dbPath: String(raw?.dbPath ?? raw?.db_path ?? ""),
    };
  } catch (error) {
    throw new Error(
      typeof error === "string" ? error : "Failed to load platform stats."
    );
  }
}

export async function exportDatabaseBackup(): Promise<string | null> {
  const defaultFileName = `nexus_backup_${new Date().toISOString().slice(0, 10)}.sqlite`;
  const selectedPath = await save({
    title: "Export SQLite Database Backup",
    defaultPath: defaultFileName,
    filters: [
      {
        name: "SQLite Database",
        extensions: ["sqlite", "db", "bak"],
      },
    ],
  });

  if (!selectedPath) return null;

  try {
    await invoke("db_export_backup", { destinationPath: selectedPath });
    return selectedPath;
  } catch (error) {
    throw new Error(
      typeof error === "string" ? error : "Failed to export database backup."
    );
  }
}

export async function restoreDatabaseBackup(): Promise<string | null> {
  const selectedPath = await open({
    title: "Select Nexus SQLite Backup to Restore",
    multiple: false,
    directory: false,
    filters: [
      {
        name: "SQLite Database",
        extensions: ["sqlite", "db", "bak"],
      },
    ],
  });

  if (!selectedPath || typeof selectedPath !== "string") return null;

  try {
    await invoke("db_restore_backup", { sourcePath: selectedPath });
    return selectedPath;
  } catch (error) {
    throw new Error(
      typeof error === "string" ? error : "Failed to restore database backup."
    );
  }
}

export async function exportConversationMarkdown(
  conversationId: string,
  title: string
): Promise<string | null> {
  const sanitized = title.replace(/[^a-zA-Z0-9_-]/g, "_").slice(0, 40);
  const defaultFileName = `${sanitized || "conversation"}_${new Date().toISOString().slice(0, 10)}.md`;

  const selectedPath = await save({
    title: "Export Conversation as Markdown",
    defaultPath: defaultFileName,
    filters: [
      {
        name: "Markdown Document",
        extensions: ["md", "markdown"],
      },
    ],
  });

  if (!selectedPath) return null;

  try {
    await invoke("db_export_conversation_markdown", {
      conversationId,
      destinationPath: selectedPath,
    });
    return selectedPath;
  } catch (error) {
    throw new Error(
      typeof error === "string" ? error : "Failed to export Markdown."
    );
  }
}

export async function exportConversationJson(
  conversationId: string,
  title: string
): Promise<string | null> {
  const sanitized = title.replace(/[^a-zA-Z0-9_-]/g, "_").slice(0, 40);
  const defaultFileName = `${sanitized || "conversation"}_${new Date().toISOString().slice(0, 10)}.json`;

  const selectedPath = await save({
    title: "Export Conversation as JSON",
    defaultPath: defaultFileName,
    filters: [
      {
        name: "JSON File",
        extensions: ["json"],
      },
    ],
  });

  if (!selectedPath) return null;

  try {
    await invoke("db_export_conversation_json", {
      conversationId,
      destinationPath: selectedPath,
    });
    return selectedPath;
  } catch (error) {
    throw new Error(
      typeof error === "string" ? error : "Failed to export JSON."
    );
  }
}

export async function resetSandboxDatabase(): Promise<string> {
  try {
    return await invoke<string>("db_reset_sandbox");
  } catch (error) {
    throw new Error(
      typeof error === "string" ? error : "Failed to reset sandbox database."
    );
  }
}

export async function getAppSetting(key: string): Promise<string | null> {
  try {
    return await invoke<string | null>("db_get_setting", { key });
  } catch (error) {
    console.error(`Failed to fetch app setting ${key}:`, error);
    return null;
  }
}

export async function setAppSetting(key: string, value: string): Promise<void> {
  try {
    await invoke<void>("db_set_setting", { key, value });
  } catch (error) {
    throw new Error(
      typeof error === "string" ? error : `Failed to save setting ${key}.`
    );
  }
}
