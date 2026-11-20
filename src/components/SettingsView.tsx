import { useEffect, useState } from "react";
import {
  exportDatabaseBackup,
  getPlatformStats,
  resetSandboxDatabase,
  restoreDatabaseBackup,
  type PlatformStats,
} from "../services/settings";

interface SettingsViewProps {
  selectedModel: string;
  onSelectModel: (model: string) => void;
  availableModels: string[];
  ollamaStatus: "connected" | "disconnected" | "checking";
  memoraReady: boolean;
  onRefreshAll: () => Promise<void>;
}

function formatBytes(bytes: number): string {
  if (!bytes || bytes <= 0) return "0 KB";
  const kb = bytes / 1024;
  if (kb >= 1024) {
    return `${(kb / 1024).toFixed(2)} MB`;
  }
  return `${kb.toFixed(1)} KB`;
}

export function SettingsView({
  selectedModel,
  onSelectModel,
  availableModels,
  ollamaStatus,
  memoraReady,
  onRefreshAll,
}: SettingsViewProps) {
  const [stats, setStats] = useState<PlatformStats | null>(null);
  const [loading, setLoading] = useState(true);
  const [busyAction, setBusyAction] = useState<string | null>(null);
  const [feedback, setFeedback] = useState<{
    type: "success" | "error";
    message: string;
  } | null>(null);

  const loadStats = async () => {
    try {
      setLoading(true);
      const data = await getPlatformStats();
      setStats(data);
    } catch (e) {
      console.error("Failed to load platform stats:", e);
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    void loadStats();
  }, []);

  const handleExportBackup = async () => {
    try {
      setBusyAction("export");
      setFeedback(null);
      const path = await exportDatabaseBackup();
      if (path) {
        setFeedback({
          type: "success",
          message: `Backup successfully created at: ${path}`,
        });
        await loadStats();
      }
    } catch (err) {
      setFeedback({
        type: "error",
        message: err instanceof Error ? err.message : "Backup failed.",
      });
    } finally {
      setBusyAction(null);
    }
  };

  const handleRestoreBackup = async () => {
    const confirmed = window.confirm(
      "Are you sure you want to restore a database backup? This will replace your current conversations and workspaces."
    );
    if (!confirmed) return;

    try {
      setBusyAction("restore");
      setFeedback(null);
      const path = await restoreDatabaseBackup();
      if (path) {
        setFeedback({
          type: "success",
          message: "Database restored successfully! Reloading platform state...",
        });
        await onRefreshAll();
        await loadStats();
      }
    } catch (err) {
      setFeedback({
        type: "error",
        message: err instanceof Error ? err.message : "Restore failed.",
      });
    } finally {
      setBusyAction(null);
    }
  };

  const handleResetSandbox = async () => {
    const confirmed = window.confirm(
      "Reset SQLite code runner sandbox? Any custom tables or data created during code execution will be reset to default schemas."
    );
    if (!confirmed) return;

    try {
      setBusyAction("sandbox");
      setFeedback(null);
      const msg = await resetSandboxDatabase();
      setFeedback({
        type: "success",
        message: msg,
      });
    } catch (err) {
      setFeedback({
        type: "error",
        message: err instanceof Error ? err.message : "Sandbox reset failed.",
      });
    } finally {
      setBusyAction(null);
    }
  };

  return (
    <div className="settings-page">
      <div className="settings-header">
        <div>
          <h1>Settings & System Controls</h1>
          <p className="settings-subtitle">
            Manage your local database, automated backups, hardware configurations, and connected storage vaults.
          </p>
        </div>
      </div>

      {feedback && (
        <div className={`settings-feedback-banner ${feedback.type}`}>
          <span>
            {feedback.type === "success" ? "✓" : "⚠"} {feedback.message}
          </span>
          <button
            type="button"
            className="feedback-dismiss"
            onClick={() => setFeedback(null)}
          >
            ×
          </button>
        </div>
      )}

      {/* PLATFORM STORAGE STATS */}
      <div className="settings-section">
        <h2>Local Storage & Platform Health</h2>
        <div className="settings-stats-grid">
          <div className="settings-stat-card">
            <span className="stat-label">CONVERSATIONS</span>
            <span className="stat-value">
              {loading ? "…" : stats?.totalConversations ?? 0}
            </span>
            <span className="stat-sub">Persisted in SQLite</span>
          </div>

          <div className="settings-stat-card">
            <span className="stat-label">STORED MESSAGES</span>
            <span className="stat-value">
              {loading ? "…" : stats?.totalMessages ?? 0}
            </span>
            <span className="stat-sub">Indexed across all chats</span>
          </div>

          <div className="settings-stat-card">
            <span className="stat-label">ISOLATED WORKSPACES</span>
            <span className="stat-value">
              {loading ? "…" : stats?.totalWorkspaces ?? 0}
            </span>
            <span className="stat-sub">
              {stats?.totalMaterials ?? 0} materials · {stats?.totalNotes ?? 0} notes
            </span>
          </div>

          <div className="settings-stat-card">
            <span className="stat-label">DATABASE SIZE</span>
            <span className="stat-value">
              {loading ? "…" : formatBytes(stats?.dbSizeBytes ?? 0)}
            </span>
            <span className="stat-sub">
              nexus.db (WAL enabled)
            </span>
          </div>
        </div>
      </div>

      {/* BACKUP & RESTORE ACTIONS */}
      <div className="settings-section">
        <h2>Data Preservation & Backups</h2>
        <div className="settings-card">
          <div className="settings-action-row">
            <div>
              <h3>Export Database Backup</h3>
              <p>
                Creates an atomic, consistent snapshot of your entire Nexus SQLite database (including all workspaces, notes, materials, and conversations).
              </p>
            </div>
            <button
              type="button"
              className="settings-action-btn primary"
              onClick={() => void handleExportBackup()}
              disabled={busyAction !== null}
            >
              {busyAction === "export" ? "Exporting..." : "Export .sqlite Backup"}
            </button>
          </div>

          <div className="settings-divider" />

          <div className="settings-action-row">
            <div>
              <h3>Restore from Backup</h3>
              <p>
                Restore platform data from a previously created Nexus backup file. Existing data will be replaced.
              </p>
            </div>
            <button
              type="button"
              className="settings-action-btn secondary"
              onClick={() => void handleRestoreBackup()}
              disabled={busyAction !== null}
            >
              {busyAction === "restore" ? "Restoring..." : "Restore Backup"}
            </button>
          </div>

          <div className="settings-divider" />

          <div className="settings-action-row">
            <div>
              <h3>Reset Code Execution Sandbox</h3>
              <p>
                Cleans the local SQLite sandbox database (`sandbox.db`) used by the Code Runner, restoring pre-seeded university and employee sample tables.
              </p>
            </div>
            <button
              type="button"
              className="settings-action-btn danger"
              onClick={() => void handleResetSandbox()}
              disabled={busyAction !== null}
            >
              {busyAction === "sandbox" ? "Resetting..." : "Reset Sandbox DB"}
            </button>
          </div>
        </div>
      </div>

      {/* ENVIRONMENT & RUNTIME */}
      <div className="settings-section">
        <h2>Local Runtimes & Engine Configuration</h2>
        <div className="settings-card">
          <div className="settings-row">
            <span className="settings-row-label">Default Model for New Chats</span>
            <select
              className="settings-select"
              value={selectedModel}
              onChange={(e) => onSelectModel(e.target.value)}
            >
              {availableModels.map((m) => (
                <option key={m} value={m}>
                  {m}
                </option>
              ))}
            </select>
          </div>

          <div className="settings-row">
            <span className="settings-row-label">Ollama Local API Endpoint</span>
            <div className="settings-row-right">
              <code>http://localhost:11434</code>
              <span
                className={`status-pill ${
                  ollamaStatus === "connected" ? "connected" : "disconnected"
                }`}
              >
                ● {ollamaStatus === "connected" ? "Connected" : "Disconnected"}
              </span>
            </div>
          </div>

          <div className="settings-row">
            <span className="settings-row-label">Connected Obsidian Vault</span>
            <div className="settings-row-right">
              <span className="path-text">
                Configured via MEMORA vault settings
              </span>
              <span className={`status-pill ${memoraReady ? "connected" : "disconnected"}`}>
                ● {memoraReady ? "Active" : "Unreachable"}
              </span>
            </div>
          </div>

          <div className="settings-row">
            <span className="settings-row-label">MEMORA Retrieval Engine</span>
            <div className="settings-row-right">
              <span className="path-text">
                Python 3.13 Virtualenv (BM25 + ChromaDB)
              </span>
              <span className={`status-pill ${memoraReady ? "connected" : "disconnected"}`}>
                ● {memoraReady ? "Ready" : "Offline"}
              </span>
            </div>
          </div>

          <div className="settings-row">
            <span className="settings-row-label">Privacy & Telemetry Boundary</span>
            <span className="settings-row-value">
              100% Local · Zero External Telemetry · Zero Cloud Sync
            </span>
          </div>
        </div>
      </div>
    </div>
  );
}
