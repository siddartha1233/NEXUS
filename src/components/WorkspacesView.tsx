import { useState } from "react";
import { type Workspace } from "../services/workspaces";

interface WorkspacesViewProps {
  workspaces: Workspace[];
  activeWorkspaceId: string | null;
  onSelectWorkspace: (ws: Workspace) => void;
  onCreateWorkspace: (name: string, description: string) => Promise<void>;
  onDeleteWorkspace: (id: string) => Promise<void>;
}

export function WorkspacesView({
  workspaces,
  activeWorkspaceId,
  onSelectWorkspace,
  onCreateWorkspace,
  onDeleteWorkspace,
}: WorkspacesViewProps) {
  const [showCreateModal, setShowCreateModal] = useState(false);
  const [name, setName] = useState("");
  const [description, setDescription] = useState("");
  const [isSubmitting, setIsSubmitting] = useState(false);

  async function handleSubmit(e: React.FormEvent) {
    e.preventDefault();
    if (!name.trim() || isSubmitting) return;

    setIsSubmitting(true);
    try {
      await onCreateWorkspace(name.trim(), description.trim());
      setName("");
      setDescription("");
      setShowCreateModal(false);
    } catch (err) {
      console.error("Failed to create workspace:", err);
    } finally {
      setIsSubmitting(false);
    }
  }

  return (
    <div className="workspaces-page">
      <div className="workspaces-header">
        <div>
          <h1>Workspaces</h1>
          <p className="workspaces-subtitle">
            Isolated environments for courses, projects, and research.
            Conversations, notes, and materials never cross boundaries.
          </p>
        </div>

        <button
          type="button"
          className="ws-primary-btn"
          onClick={() => setShowCreateModal(true)}
        >
          ＋ New Workspace
        </button>
      </div>

      {showCreateModal && (
        <div className="ws-modal-backdrop">
          <div className="ws-modal">
            <h2>Create New Workspace</h2>
            <form onSubmit={handleSubmit}>
              <div className="ws-form-group">
                <label>Workspace Name</label>
                <input
                  type="text"
                  placeholder="e.g. CSIT205 Generative AI, Compiler Design"
                  value={name}
                  onChange={(e) => setName(e.currentTarget.value)}
                  autoFocus
                  required
                />
              </div>

              <div className="ws-form-group">
                <label>Description (Optional)</label>
                <textarea
                  rows={3}
                  placeholder="What is this workspace focused on?"
                  value={description}
                  onChange={(e) => setDescription(e.currentTarget.value)}
                />
              </div>

              <div className="ws-modal-actions">
                <button
                  type="button"
                  className="ws-secondary-btn"
                  onClick={() => setShowCreateModal(false)}
                >
                  Cancel
                </button>
                <button
                  type="submit"
                  className="ws-primary-btn"
                  disabled={!name.trim() || isSubmitting}
                >
                  {isSubmitting ? "Creating..." : "Create Workspace"}
                </button>
              </div>
            </form>
          </div>
        </div>
      )}

      {workspaces.length === 0 ? (
        <div className="ws-empty-container">
          <div className="ws-empty-icon">▦</div>
          <h3>No Workspaces Yet</h3>
          <p>
            Create your first workspace to start organizing study materials,
            lecture notes, and isolated AI chats.
          </p>
          <button
            type="button"
            className="ws-primary-btn"
            onClick={() => setShowCreateModal(true)}
          >
            Create Your First Workspace
          </button>
        </div>
      ) : (
        <div className="ws-grid">
          {workspaces.map((ws) => {
            const isActive = ws.id === activeWorkspaceId;
            return (
              <div
                key={ws.id}
                className={`ws-card-item ${isActive ? "active" : ""}`}
                onClick={() => onSelectWorkspace(ws)}
              >
                <div className="ws-card-header">
                  <span className="ws-card-icon">▦</span>
                  <div className="ws-card-meta">
                    <span className="ws-card-title">{ws.name}</span>
                    <span className="ws-card-date">
                      {new Date(ws.updated_at).toLocaleDateString()}
                    </span>
                  </div>
                </div>

                <p className="ws-card-desc">
                  {ws.description || "No description provided."}
                </p>

                <div className="ws-card-footer">
                  <span className="ws-isolation-tag">Isolated</span>
                  <button
                    type="button"
                    className="ws-card-del-btn"
                    title="Delete workspace"
                    onClick={(e) => {
                      e.stopPropagation();
                      if (
                        confirm(
                          `Delete "${ws.name}" and all its isolated conversations, notes, and materials?`
                        )
                      ) {
                        void onDeleteWorkspace(ws.id);
                      }
                    }}
                  >
                    ×
                  </button>
                </div>
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}
