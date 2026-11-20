import {
  isEmbeddingModel,
  type OllamaModel,
  type RunningOllamaModel,
} from "../services/ollama";

interface ModelManagerProps {
  models: OllamaModel[];
  runningModels: RunningOllamaModel[];
  selectedModel: string;
  connectionStatus: "checking" | "connected" | "disconnected";
  isRefreshing: boolean;
  isGenerating: boolean;
  busyModel: string | null;
  error: string;
  onSelectModel: (modelName: string) => void;
  onUnloadModel: (modelName: string) => void;
  onRefresh: () => void;
}

function formatBytes(bytes: number): string {
  if (!bytes || bytes <= 0) return "—";
  const gb = bytes / (1024 * 1024 * 1024);
  if (gb >= 1) return `${gb.toFixed(2)} GB`;
  const mb = bytes / (1024 * 1024);
  return `${mb.toFixed(0)} MB`;
}

function formatDate(iso: string): string {
  if (!iso) return "—";
  const date = new Date(iso);
  if (Number.isNaN(date.getTime())) return "—";
  return date.toLocaleDateString(undefined, {
    year: "numeric",
    month: "short",
    day: "numeric",
  });
}

export function ModelManager({
  models,
  runningModels,
  selectedModel,
  connectionStatus,
  isRefreshing,
  isGenerating,
  busyModel,
  error,
  onSelectModel,
  onUnloadModel,
  onRefresh,
}: ModelManagerProps) {
  const chatModels = models.filter((m) => !isEmbeddingModel(m.name));
  const embeddingModels = models.filter((m) => isEmbeddingModel(m.name));
  const runningSet = new Set(runningModels.map((m) => m.name));

  const statusText =
    connectionStatus === "connected"
      ? "Connected"
      : connectionStatus === "checking"
        ? "Checking..."
        : "Disconnected";

  return (
    <section className="model-manager-page">
      <div className="model-manager-inner">
        <div className="model-manager-header">
          <div>
            <h1>Model Manager</h1>
            <p className="model-manager-subtitle">
              Manage local Ollama chat and embedding models. One chat model is
              active at a time.
            </p>
          </div>

          <button
            type="button"
            className="mm-button"
            onClick={onRefresh}
            disabled={isRefreshing}
          >
            {isRefreshing ? "Refreshing..." : "Refresh"}
          </button>
        </div>

        <div className="mm-summary-grid">
          <div className="mm-summary-card">
            <span className="mm-summary-label">RUNTIME STATUS</span>
            <span className="mm-summary-value">
              <span className="status-dot" />
              {statusText}
            </span>
          </div>

          <div className="mm-summary-card">
            <span className="mm-summary-label">ACTIVE CHAT MODEL</span>
            <span className="mm-summary-value">{selectedModel || "None"}</span>
          </div>

          <div className="mm-summary-card">
            <span className="mm-summary-label">LOADED IN MEMORY</span>
            <span className="mm-summary-value">
              {runningModels.length === 0
                ? "None"
                : `${runningModels.length} loaded`}
            </span>
          </div>
        </div>

        {isGenerating && (
          <div className="mm-notice">
            Generation is currently in progress. Wait for it to finish or stop
            it before switching or unloading models.
          </div>
        )}

        {error && <div className="mm-notice">{error}</div>}

        <div className="mm-section">
          <div className="mm-section-header">
            <h2>Chat Models</h2>
            <span className="mm-section-count">{chatModels.length}</span>
          </div>

          {chatModels.length === 0 ? (
            <div className="mm-empty">
              {connectionStatus === "disconnected"
                ? "Ollama is disconnected. Start Ollama and click Refresh."
                : "No local chat models found."}
            </div>
          ) : (
            <div className="mm-list">
              {chatModels.map((model) => {
                const isActive = model.name === selectedModel;
                const isLoaded = runningSet.has(model.name);
                const isBusy = busyModel === model.name;

                return (
                  <div
                    key={model.name}
                    className={`mm-card ${isActive ? "mm-card-active" : ""}`}
                  >
                    <div className="mm-card-info">
                      <div className="mm-card-title-row">
                        <span className="mm-model-name">{model.name}</span>
                        {isActive && <span className="mm-badge mm-badge-solid">Active</span>}
                        {isLoaded && <span className="mm-badge">Loaded</span>}
                      </div>

                      <div className="mm-model-meta">
                        <span>Size: {formatBytes(model.size)}</span>
                        <span>·</span>
                        <span>Updated: {formatDate(model.modified_at)}</span>
                      </div>
                    </div>

                    <div className="mm-card-actions">
                      {isLoaded && (
                        <button
                          type="button"
                          className="mm-button mm-button-ghost"
                          onClick={() => onUnloadModel(model.name)}
                          disabled={isGenerating || isBusy}
                        >
                          {isBusy ? "Working..." : "Unload"}
                        </button>
                      )}

                      <button
                        type="button"
                        className={`mm-button ${isActive ? "mm-button-ghost" : ""}`}
                        onClick={() => onSelectModel(model.name)}
                        disabled={isActive || isGenerating || isBusy}
                      >
                        {isActive ? "Selected" : isBusy ? "Switching..." : "Select"}
                      </button>
                    </div>
                  </div>
                );
              })}
            </div>
          )}
        </div>

        <div className="mm-section">
          <div className="mm-section-header">
            <h2>Embedding Models</h2>
            <span className="mm-section-count">{embeddingModels.length}</span>
          </div>

          <p className="mm-section-note">
            Embedding models run separately from chat models and are reserved
            for local knowledge indexing.
          </p>

          {embeddingModels.length === 0 ? (
            <div className="mm-empty">No embedding models detected.</div>
          ) : (
            <div className="mm-list">
              {embeddingModels.map((model) => {
                const isLoaded = runningSet.has(model.name);
                const isBusy = busyModel === model.name;

                return (
                  <div key={model.name} className="mm-card">
                    <div className="mm-card-info">
                      <div className="mm-card-title-row">
                        <span className="mm-model-name">{model.name}</span>
                        <span className="mm-badge">Embedding</span>
                        {isLoaded && <span className="mm-badge">Loaded</span>}
                      </div>

                      <div className="mm-model-meta">
                        <span>Size: {formatBytes(model.size)}</span>
                        <span>·</span>
                        <span>Updated: {formatDate(model.modified_at)}</span>
                      </div>
                    </div>

                    <div className="mm-card-actions">
                      {isLoaded && (
                        <button
                          type="button"
                          className="mm-button mm-button-ghost"
                          onClick={() => onUnloadModel(model.name)}
                          disabled={isBusy}
                        >
                          {isBusy ? "Working..." : "Unload"}
                        </button>
                      )}
                    </div>
                  </div>
                );
              })}
            </div>
          )}
        </div>
      </div>
    </section>
  );
}
