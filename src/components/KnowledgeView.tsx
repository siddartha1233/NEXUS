import { useState } from "react";
import {
  triggerMemoraIngest,
  type MemoraSourceItem,
} from "../services/memora";

interface KnowledgeViewProps {
  sources: MemoraSourceItem[];
  memoraReady: boolean;
  onRefreshSources: () => Promise<void>;
}

export function KnowledgeView({
  sources,
  memoraReady,
  onRefreshSources,
}: KnowledgeViewProps) {
  const [isSyncing, setIsSyncing] = useState(false);
  const [syncMessage, setSyncMessage] = useState<string | null>(null);

  async function handleSyncVault() {
    setIsSyncing(true);
    setSyncMessage("Syncing and indexing Obsidian vault...");
    try {
      await triggerMemoraIngest();
      await onRefreshSources();
      setSyncMessage("Vault successfully indexed into ChromaDB + SQLite FTS5!");
    } catch (err) {
      setSyncMessage(
        err instanceof Error ? err.message : "Vault indexing encountered an error."
      );
    } finally {
      setIsSyncing(false);
    }
  }

  return (
    <div className="knowledge-page">
      <div className="knowledge-header">
        <div>
          <h1>Knowledge & Memory Vault</h1>
          <p className="knowledge-subtitle">
            Connected to your Obsidian vault via MEMORA. Notes are automatically
            embedded into ChromaDB and indexed into SQLite FTS5 for zero-hallucination grounded retrieval.
          </p>
        </div>

        <button
          type="button"
          className="ws-primary-btn"
          onClick={() => void handleSyncVault()}
          disabled={isSyncing}
        >
          {isSyncing ? "Syncing..." : "↻ Sync / Reindex Vault"}
        </button>
      </div>

      <div className="mm-summary-grid" style={{ marginBottom: "24px" }}>
        <div className="mm-summary-card">
          <span className="mm-summary-label">MEMORA ENGINE</span>
          <span className="mm-summary-value">
            <span className="status-dot" />
            {memoraReady ? "Connected & Ready" : "Disconnected"}
          </span>
        </div>

        <div className="mm-summary-card">
          <span className="mm-summary-label">INDEXED NOTES</span>
          <span className="mm-summary-value">{sources.length} Notes</span>
        </div>

        <div className="mm-summary-card">
          <span className="mm-summary-label">HYBRID RETRIEVAL</span>
          <span className="mm-summary-value">ChromaDB + SQLite FTS5</span>
        </div>
      </div>

      {syncMessage && <div className="mm-notice">{syncMessage}</div>}

      <div className="knowledge-section">
        <div className="knowledge-section-header">
          <h2>Indexed Notes ({sources.length})</h2>
        </div>

        {sources.length === 0 ? (
          <div className="ws-empty-state">
            No notes found. Click "Sync / Reindex Vault" above to index your Obsidian notes.
          </div>
        ) : (
          <div className="knowledge-list">
            {sources.map((s) => (
              <div key={s.path} className="knowledge-card">
                <div className="knowledge-card-icon">▤</div>
                <div className="knowledge-card-info">
                  <div className="knowledge-card-title-row">
                    <span className="knowledge-title">{s.path}</span>
                    <span className="knowledge-status-badge">
                      {s.status === "indexed" ? "✓ Indexed" : s.status}
                    </span>
                  </div>
                  <div className="knowledge-meta">
                    <span>{s.chunk_count} chunk{s.chunk_count > 1 ? "s" : ""}</span>
                    <span>·</span>
                    <span>{s.size} bytes</span>
                  </div>
                </div>
              </div>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}
