import { useEffect, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import {
  exportDatabaseBackup,
  getAppSetting,
  getPlatformStats,
  resetSandboxDatabase,
  restoreDatabaseBackup,
  setAppSetting,
  type PlatformStats,
} from "../services/settings";
import { fetchLiveGroqModels, type CloudModelInfo } from "../services/cloudModels";

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

  // Cloud API settings state
  const [groqKey, setGroqKey] = useState("");
  const [savedGroqKey, setSavedGroqKey] = useState("");
  const [showGroqKey, setShowGroqKey] = useState(false);
  const [savingKey, setSavingKey] = useState(false);
  const [autoFallback, setAutoFallback] = useState(true);
  const [activeCloudModels, setActiveCloudModels] = useState<CloudModelInfo[]>([]);

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

  const loadCloudSettings = async () => {
    try {
      const masked = await getAppSetting("groq_api_key");
      setSavedGroqKey(masked || "");
      setGroqKey("");
      const fallbackSetting = await getAppSetting("cloud_auto_fallback");
      if (fallbackSetting !== null) {
        setAutoFallback(fallbackSetting === "true");
      }
      const live = await fetchLiveGroqModels();
      setActiveCloudModels(live);
    } catch (err) {
      console.error("Failed to load cloud settings:", err);
    }
  };

  useEffect(() => {
    void loadStats();
    void loadCloudSettings();
  }, []);

  const handleSaveGroqKey = async () => {
    const cleanInput = groqKey.trim();
    if (!cleanInput) return;
    try {
      setSavingKey(true);
      setFeedback(null);
      await setAppSetting("groq_api_key", cleanInput);
      const masked = await getAppSetting("groq_api_key");
      setGroqKey("");
      setSavedGroqKey(masked || "");
      const live = await fetchLiveGroqModels();
      setActiveCloudModels(live);
      await onRefreshAll();
      setFeedback({
        type: "success",
        message: `Groq key saved! Loaded ${live.length} live active models from Groq.`,
      });
    } catch (err) {
      setFeedback({
        type: "error",
        message: err instanceof Error ? err.message : "Failed to save API key.",
      });
    } finally {
      setSavingKey(false);
    }
  };

  const handleClearGroqKey = async () => {
    try {
      setSavingKey(true);
      setFeedback(null);
      await setAppSetting("groq_api_key", "");
      setGroqKey("");
      setSavedGroqKey("");
      setActiveCloudModels([]);
      await onRefreshAll();
      setFeedback({
        type: "success",
        message: "Groq API key removed.",
      });
    } catch (err) {
      setFeedback({
        type: "error",
        message: err instanceof Error ? err.message : "Failed to clear API key.",
      });
    } finally {
      setSavingKey(false);
    }
  };

  const handleToggleAutoFallback = async (enabled: boolean) => {
    setAutoFallback(enabled);
    try {
      await setAppSetting("cloud_auto_fallback", enabled ? "true" : "false");
    } catch (err) {
      console.error("Failed to persist auto fallback:", err);
    }
  };

  const handleOpenGroqPortal = async () => {
    try {
      await openUrl("https://console.groq.com/keys");
    } catch {
      window.open("https://console.groq.com/keys", "_blank");
    }
  };

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
        await loadCloudSettings();
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
      "Reset the SQLite code execution sandbox? Any tables created during your SQL runs will be reset to default sample schemas."
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
        message: err instanceof Error ? err.message : "Reset failed.",
      });
    } finally {
      setBusyAction(null);
    }
  };

  return (
    <div className="settings-page">
      <div className="settings-header">
        <div>
          <h1>Settings & Administration</h1>
          <p className="settings-subtitle">
            Configure local runtimes, cloud free-tier models, atomic backups, and security boundaries.
          </p>
        </div>
      </div>

      {feedback && (
        <div className={`settings-feedback-banner ${feedback.type}`}>
          <span>{feedback.message}</span>
          <button
            type="button"
            className="feedback-dismiss"
            onClick={() => setFeedback(null)}
          >
            ✕
          </button>
        </div>
      )}

      {/* CLOUD INFERENCE & FREE-TIER MODELS */}
      <div className="settings-section">
        <h2>⚡ Cloud Free-Tier Inference (Groq)</h2>
        <div className="settings-card">
          <div className="settings-action-row" style={{ alignItems: "flex-start" }}>
            <div style={{ flex: 1, paddingRight: "1.5rem" }}>
              <h3>Groq Cloud Free Tier</h3>
              <p>
                Run frontier open-source models at ~300+ tokens/sec using Groq LPUs. Completely free (30 requests/minute limit).
                Excerpts retrieved during search will be sent to Groq only when a Groq model is selected.
              </p>
              <div style={{ marginTop: "0.6rem", display: "flex", gap: "0.5rem", flexWrap: "wrap" }}>
                {activeCloudModels.length > 0 ? (
                  activeCloudModels.map((m) => (
                    <span
                      key={m.id}
                      className="status-pill connected"
                      style={{ fontSize: "0.75rem", padding: "0.2rem 0.5rem" }}
                    >
                      ⚡ {m.name} ({m.speed})
                    </span>
                  ))
                ) : (
                  <span style={{ fontSize: "0.8rem", color: "var(--text-secondary)" }}>
                    Enter and save your free Groq key below to dynamically load active models.
                  </span>
                )}
              </div>
            </div>

            <div style={{ display: "flex", flexDirection: "column", gap: "0.5rem", minWidth: "260px" }}>
              <button
                type="button"
                className="settings-action-btn primary"
                onClick={() => void handleOpenGroqPortal()}
                style={{ textAlign: "center", display: "flex", alignItems: "center", justifyContent: "center", gap: "0.4rem" }}
              >
                <span>🔗</span>
                <span>Get Free Groq Key</span>
              </button>
              <span style={{ fontSize: "0.75rem", color: "var(--text-secondary)", textAlign: "center" }}>
                Free forever · Instant login · No credit card
              </span>
            </div>
          </div>

          <div className="settings-divider" />

          <div className="settings-row" style={{ alignItems: "center" }}>
            <span className="settings-row-label">Groq API Key</span>
            <div className="settings-row-right" style={{ flex: 1, maxWidth: "550px", justifyContent: "flex-end" }}>
              <input
                type={showGroqKey ? "text" : "password"}
                className="settings-input"
                placeholder={savedGroqKey ? `Active key: ${savedGroqKey} (paste new key to replace)` : "gsk_..."}
                value={groqKey}
                onChange={(e) => setGroqKey(e.target.value)}
                style={{
                  background: "var(--bg-secondary)",
                  color: "var(--text-primary)",
                  border: "1px solid var(--border-color)",
                  borderRadius: "6px",
                  padding: "0.45rem 0.65rem",
                  fontFamily: "monospace",
                  fontSize: "0.85rem",
                  flex: 1,
                }}
              />
              <button
                type="button"
                className="settings-action-btn secondary"
                onClick={() => setShowGroqKey(!showGroqKey)}
                style={{ padding: "0.4rem 0.6rem", fontSize: "0.8rem" }}
              >
                {showGroqKey ? "Hide" : "Show"}
              </button>
              <button
                type="button"
                className="settings-action-btn primary"
                onClick={() => void handleSaveGroqKey()}
                disabled={savingKey || !groqKey.trim()}
                style={{ padding: "0.4rem 0.8rem", fontSize: "0.8rem" }}
              >
                {savingKey ? "Saving..." : "Save Key"}
              </button>
              {savedGroqKey && (
                <button
                  type="button"
                  className="settings-action-btn danger"
                  onClick={() => void handleClearGroqKey()}
                  disabled={savingKey}
                  style={{ padding: "0.4rem 0.6rem", fontSize: "0.8rem" }}
                >
                  Clear
                </button>
              )}
            </div>
          </div>

          <div className="settings-row">
            <span className="settings-row-label">Key Status & Storage</span>
            <div className="settings-row-right">
              <span className={`status-pill ${savedGroqKey ? "connected" : "disconnected"}`}>
                ● {savedGroqKey ? "Configured & Active" : "No Key Configured"}
              </span>
              <span className="path-text" style={{ fontSize: "0.75rem" }}>
                Stored locally in SQLite (never uploaded)
              </span>
            </div>
          </div>

          <div className="settings-divider" />

          <div className="settings-row">
            <div>
              <span className="settings-row-label" style={{ display: "block" }}>
                Auto-Fallback to Local Model
              </span>
              <span style={{ fontSize: "0.75rem", color: "var(--text-secondary)" }}>
                Automatically switches to your local Ollama model if Groq rate limits (429) or internet drops.
              </span>
            </div>
            <div className="settings-row-right">
              <label style={{ display: "flex", alignItems: "center", gap: "0.5rem", cursor: "pointer" }}>
                <input
                  type="checkbox"
                  checked={autoFallback}
                  onChange={(e) => void handleToggleAutoFallback(e.target.checked)}
                  style={{ width: "16px", height: "16px", cursor: "pointer" }}
                />
                <span style={{ fontSize: "0.85rem", color: "var(--text-primary)" }}>
                  {autoFallback ? "Enabled" : "Disabled"}
                </span>
              </label>
            </div>
          </div>
        </div>
      </div>

      {/* PLATFORM OVERVIEW & STATS */}
      <div className="settings-section">
        <h2>Database Metrics & Health</h2>
        <div className="settings-stats-grid">
          <div className="settings-stat-card">
            <span className="stat-label">Conversations</span>
            <span className="stat-value">
              {loading ? "..." : stats?.totalConversations ?? 0}
            </span>
            <span className="stat-sub">
              {loading ? "..." : `${stats?.totalMessages ?? 0} total messages`}
            </span>
          </div>

          <div className="settings-stat-card">
            <span className="stat-label">Workspaces</span>
            <span className="stat-value">
              {loading ? "..." : stats?.totalWorkspaces ?? 0}
            </span>
            <span className="stat-sub">
              {loading ? "..." : `${stats?.totalMaterials ?? 0} indexed materials`}
            </span>
          </div>

          <div className="settings-stat-card">
            <span className="stat-label">Workspace Notes</span>
            <span className="stat-value">
              {loading ? "..." : stats?.totalNotes ?? 0}
            </span>
            <span className="stat-sub">Saved student / project notes</span>
          </div>

          <div className="settings-stat-card">
            <span className="stat-label">SQLite Size</span>
            <span className="stat-value">
              {loading ? "..." : formatBytes(stats?.dbSizeBytes ?? 0)}
            </span>
            <span className="stat-sub" title={stats?.dbPath}>
              WAL mode active
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
              <optgroup label="Local Models (Ollama)">
                {availableModels.map((m) => (
                  <option key={m} value={m}>
                    💻 {m}
                  </option>
                ))}
              </optgroup>
              <optgroup label="Cloud Models (Groq Free - Live)">
                {activeCloudModels.length > 0 ? (
                  activeCloudModels.map((m) => (
                    <option key={m.id} value={m.id}>
                      ⚡ {m.name}
                    </option>
                  ))
                ) : (
                  <option disabled value="">
                    Paste Groq key to load models
                  </option>
                )}
              </optgroup>
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
              100% Local SQLite · Cloud only invoked when Groq model explicitly chosen
            </span>
          </div>
        </div>
      </div>
    </div>
  );
}
