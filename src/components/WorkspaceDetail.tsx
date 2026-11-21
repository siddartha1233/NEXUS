import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import {
  addWorkspaceMaterial,
  deleteWorkspaceMaterial,
  deleteWorkspaceNote,
  importWorkspaceFile,
  listWorkspaceMaterials,
  listWorkspaceNotes,
  saveWorkspaceNote,
  type Workspace,
  type WorkspaceMaterial,
  type WorkspaceNote,
} from "../services/workspaces";
import { type Conversation } from "../services/conversations";
import { getAppSetting, setAppSetting } from "../services/settings";

interface WorkspaceDetailProps {
  workspace: Workspace;
  conversations: Conversation[];
  selectedModel: string;
  onOpenConversation: (conv: Conversation) => void;
  onStartNewChat: (prefillPrompt?: string) => void;
  onBack: () => void;
  onStrictOfflineChange?: (isStrict: boolean) => void;
}

type TabType = "overview" | "chat" | "materials" | "notes" | "tools";

export function WorkspaceDetail({
  workspace,
  conversations,
  onOpenConversation,
  onStartNewChat,
  onBack,
  onStrictOfflineChange,
}: WorkspaceDetailProps) {
  const [activeTab, setActiveTab] = useState<TabType>("overview");

  // Notes state
  const [notes, setNotes] = useState<WorkspaceNote[]>([]);
  const [activeNoteId, setActiveNoteId] = useState<string | null>(null);
  const [noteTitle, setNoteTitle] = useState("");
  const [noteContent, setNoteContent] = useState("");
  const [isSavingNote, setIsSavingNote] = useState(false);
  const [noteSavedToast, setNoteSavedToast] = useState(false);
  const [noteError, setNoteError] = useState<string | null>(null);

  // Materials state
  const [materials, setMaterials] = useState<WorkspaceMaterial[]>([]);
  const [materialCategory, setMaterialCategory] = useState<
    "lectures" | "assignments" | "other"
  >("lectures");
  const [materialTitle, setMaterialTitle] = useState("");
  const [materialFilename, setMaterialFilename] = useState("");
  const [materialPreview, setMaterialPreview] = useState("");
  const [showAddMaterial, setShowAddMaterial] = useState(false);
  const [isUploading, setIsUploading] = useState(false);
  const [uploadError, setUploadError] = useState<string | null>(null);
  const [strictOffline, setStrictOffline] = useState(false);

  useEffect(() => {
    let cancelled = false;
    void getAppSetting(`ws_offline_${workspace.id}`).then((val) => {
      if (!cancelled) setStrictOffline(val === "true");
    });
    return () => {
      cancelled = true;
    };
  }, [workspace.id]);

  const handleToggleStrictOffline = async (enabled: boolean) => {
    setStrictOffline(enabled);
    try {
      await setAppSetting(`ws_offline_${workspace.id}`, enabled ? "true" : "false");
      onStrictOfflineChange?.(enabled);
    } catch (err) {
      setStrictOffline(!enabled);
      console.error("Failed to update workspace strict offline:", err);
    }
  };

  useEffect(() => {
    let cancelled = false;

    void listWorkspaceNotes(workspace.id)
      .then((loaded) => {
        if (cancelled) return;
        setNotes(loaded);
        if (loaded.length > 0) {
          setActiveNoteId(loaded[0].id);
          setNoteTitle(loaded[0].title);
          setNoteContent(loaded[0].content);
        } else {
          setActiveNoteId(null);
          setNoteTitle("");
          setNoteContent("");
        }
      })
      .catch((err) => {
        if (!cancelled) {
          console.error("Failed to load workspace notes:", err);
        }
      });

    void listWorkspaceMaterials(workspace.id)
      .then((loaded) => {
        if (!cancelled) setMaterials(loaded);
      })
      .catch((err) => {
        if (!cancelled) {
          console.error("Failed to load workspace materials:", err);
        }
      });

    return () => {
      cancelled = true;
    };
  }, [workspace.id]);

  // Note actions
  function selectNote(note: WorkspaceNote) {
    setActiveNoteId(note.id);
    setNoteTitle(note.title);
    setNoteContent(note.content);
  }

  function createNewNote() {
    setActiveNoteId(null);
    setNoteTitle("");
    setNoteContent("");
  }

  async function handleSaveNote() {
    if (!noteTitle.trim() && !noteContent.trim()) return;
    setIsSavingNote(true);
    setNoteError(null);
    try {
      const saved = await saveWorkspaceNote(
        workspace.id,
        noteTitle.trim() || "Untitled Note",
        noteContent,
        activeNoteId ?? undefined
      );

      setNotes((prev) => {
        const index = prev.findIndex((n) => n.id === saved.id);
        if (index >= 0) {
          const updated = [...prev];
          updated[index] = saved;
          return updated;
        }
        return [saved, ...prev];
      });
      setActiveNoteId(saved.id);
      setNoteSavedToast(true);
      setTimeout(() => setNoteSavedToast(false), 2000);
    } catch (err) {
      console.error("Failed to save note:", err);
      setNoteError(err instanceof Error ? err.message : "Failed to save note.");
    } finally {
      setIsSavingNote(false);
    }
  }

  async function handleDeleteNote(noteId: string) {
    if (!window.confirm("Delete this workspace note permanently?")) return;
    setNoteError(null);
    try {
      await deleteWorkspaceNote(workspace.id, noteId);
      const remaining = notes.filter((n) => n.id !== noteId);
      setNotes(remaining);
      if (activeNoteId === noteId) {
        if (remaining.length > 0) {
          selectNote(remaining[0]);
        } else {
          createNewNote();
        }
      }
    } catch (err) {
      console.error("Failed to delete note:", err);
      setNoteError(err instanceof Error ? err.message : "Failed to delete note.");
    }
  }

  async function handleNativeFileUpload(category: "lectures" | "assignments" | "other") {
    setUploadError(null);
    try {
      const selected = await open({
        multiple: false,
        filters: [
          {
            name: "Documents",
            extensions: ["pdf", "txt", "md", "json", "csv", "docx"],
          },
        ],
      });

      if (!selected) return;
      const filePath = typeof selected === "string" ? selected : selected[0];
      if (!filePath) return;

      setIsUploading(true);
      const imported = await importWorkspaceFile(workspace.id, category, filePath);
      setMaterials((prev) => [imported, ...prev]);
    } catch (err) {
      console.error("Failed to upload document:", err);
      const msg = typeof err === "string" ? err : err instanceof Error ? err.message : JSON.stringify(err);
      setUploadError(msg);
    } finally {
      setIsUploading(false);
    }
  }

  // Materials actions
  async function handleAddMaterial(e: React.FormEvent) {
    e.preventDefault();
    if (!materialTitle.trim() && !materialFilename.trim()) return;

    try {
      const added = await addWorkspaceMaterial(
        workspace.id,
        materialCategory,
        materialTitle.trim() || materialFilename.trim(),
        materialFilename.trim() || materialTitle.trim(),
        materialPreview.trim()
      );

      setMaterials((prev) => [added, ...prev]);
      setMaterialTitle("");
      setMaterialFilename("");
      setMaterialPreview("");
      setShowAddMaterial(false);
    } catch (err) {
      console.error("Failed to add material:", err);
    }
  }

  async function handleDeleteMaterial(matId: string) {
    if (!window.confirm("Delete this workspace material and its stored file permanently?")) {
      return;
    }
    setUploadError(null);
    try {
      await deleteWorkspaceMaterial(workspace.id, matId);
      setMaterials((prev) => prev.filter((m) => m.id !== matId));
    } catch (err) {
      console.error("Failed to delete material:", err);
      setUploadError(
        err instanceof Error ? err.message : "Failed to delete workspace material."
      );
    }
  }

  const lectures = materials.filter((m) => m.category === "lectures");
  const assignments = materials.filter((m) => m.category === "assignments");
  const otherMaterials = materials.filter((m) => m.category === "other");

  return (
    <div className="workspace-detail">
      {/* HEADER */}
      <div className="ws-detail-header">
        <div className="ws-detail-title-group">
          <button type="button" className="ws-back-button" onClick={onBack}>
            ← All Workspaces
          </button>
          <div className="ws-title-row">
            <h1>{workspace.name}</h1>
            <span className="ws-badge">Isolated Workspace</span>
          </div>
          {workspace.description && (
            <p className="ws-description">{workspace.description}</p>
          )}
        </div>

        {/* 5-SECTION NAV */}
        <div className="ws-tabs">
          <button
            type="button"
            className={`ws-tab ${activeTab === "overview" ? "active" : ""}`}
            onClick={() => setActiveTab("overview")}
          >
            Overview
          </button>
          <button
            type="button"
            className={`ws-tab ${activeTab === "chat" ? "active" : ""}`}
            onClick={() => setActiveTab("chat")}
          >
            Chat ({conversations.length})
          </button>
          <button
            type="button"
            className={`ws-tab ${activeTab === "materials" ? "active" : ""}`}
            onClick={() => setActiveTab("materials")}
          >
            Materials ({materials.length})
          </button>
          <button
            type="button"
            className={`ws-tab ${activeTab === "notes" ? "active" : ""}`}
            onClick={() => setActiveTab("notes")}
          >
            Notes ({notes.length})
          </button>
          <button
            type="button"
            className={`ws-tab ${activeTab === "tools" ? "active" : ""}`}
            onClick={() => setActiveTab("tools")}
          >
            Study Tools
          </button>
        </div>
      </div>

      {/* TAB CONTENT */}
      <div className="ws-tab-content">
        {/* OVERVIEW */}
        {activeTab === "overview" && (
          <div className="ws-overview">
            <div className="ws-stats-grid">
              <div className="ws-stat-card">
                <span className="ws-stat-label">CHATS</span>
                <span className="ws-stat-value">{conversations.length}</span>
                <button
                  type="button"
                  className="ws-card-action"
                  onClick={() => setActiveTab("chat")}
                >
                  View Chats →
                </button>
              </div>

              <div className="ws-stat-card">
                <span className="ws-stat-label">MATERIALS</span>
                <span className="ws-stat-value">{materials.length}</span>
                <button
                  type="button"
                  className="ws-card-action"
                  onClick={() => setActiveTab("materials")}
                >
                  View Materials →
                </button>
              </div>

              <div className="ws-stat-card">
                <span className="ws-stat-label">NOTES</span>
                <span className="ws-stat-value">{notes.length}</span>
                <button
                  type="button"
                  className="ws-card-action"
                  onClick={() => setActiveTab("notes")}
                >
                  Open Notes →
                </button>
              </div>
            </div>

            <div className="ws-section-box">
              <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center", gap: "1.5rem" }}>
                <div>
                  <h3>Workspace Isolation & Privacy Policy</h3>
                  <p>
                    All conversations, notes, and study materials in this workspace
                    are strictly isolated. AI queries in General Chat will never access
                    this workspace.
                  </p>
                </div>
                <label style={{ display: "flex", alignItems: "center", gap: "0.5rem", cursor: "pointer", whiteSpace: "nowrap" }}>
                  <input
                    type="checkbox"
                    checked={strictOffline}
                    onChange={(e) => void handleToggleStrictOffline(e.target.checked)}
                    style={{ width: "16px", height: "16px", cursor: "pointer" }}
                  />
                  <span style={{ fontSize: "0.85rem", fontWeight: 600, color: strictOffline ? "#34d399" : "var(--text-secondary)" }}>
                    {strictOffline ? "🔒 Strict Offline (Cloud Blocked)" : "⚡ Cloud Models Allowed"}
                  </span>
                </label>
              </div>
            </div>
          </div>
        )}

        {/* CHAT */}
        {activeTab === "chat" && (
          <div className="ws-chat-section">
            <div className="ws-chat-header">
              <h3>Workspace Conversations</h3>
              <button
                type="button"
                className="ws-primary-btn"
                onClick={() => onStartNewChat()}
              >
                ＋ New Workspace Chat
              </button>
            </div>

            {conversations.length === 0 ? (
              <div className="ws-empty-state">
                No conversations in this workspace yet. Start a new chat to talk
                with your models in this isolated scope.
              </div>
            ) : (
              <div className="ws-chat-list">
                {conversations.map((c) => (
                  <div
                    key={c.id}
                    className="ws-chat-card"
                    onClick={() => onOpenConversation(c)}
                  >
                    <div className="ws-chat-info">
                      <span className="ws-chat-title">{c.title}</span>
                      <span className="ws-chat-meta">Model: {c.model}</span>
                    </div>
                    <button type="button" className="ws-chat-open-btn">
                      Open →
                    </button>
                  </div>
                ))}
              </div>
            )}
          </div>
        )}

        {/* MATERIALS */}
        {activeTab === "materials" && (
          <div className="ws-materials-section">
            <div className="ws-section-header-row">
              <div>
                <h3>Study Materials</h3>
                <p className="ws-section-subtitle">
                  Upload lecture slides, assignment documents, or notes. Original files are preserved locally.
                </p>
              </div>
              <div style={{ display: "flex", gap: "8px" }}>
                <button
                  type="button"
                  className="ws-primary-btn"
                  onClick={() => void handleNativeFileUpload("lectures")}
                  disabled={isUploading}
                >
                  {isUploading ? "Uploading..." : "↑ Upload File"}
                </button>
                <button
                  type="button"
                  className="ws-secondary-btn"
                  onClick={() => setShowAddMaterial(!showAddMaterial)}
                >
                  {showAddMaterial ? "Cancel" : "＋ Manual Entry"}
                </button>
              </div>
            </div>

            {uploadError && <div className="mm-notice">{uploadError}</div>}

            {showAddMaterial && (
              <form className="ws-material-form" onSubmit={handleAddMaterial}>
                <div className="ws-form-row">
                  <div className="ws-form-group">
                    <label>Category</label>
                    <select
                      value={materialCategory}
                      onChange={(e) =>
                        setMaterialCategory(
                          e.currentTarget.value as
                            | "lectures"
                            | "assignments"
                            | "other"
                        )
                      }
                    >
                      <option value="lectures">Lectures</option>
                      <option value="assignments">Assignments</option>
                      <option value="other">Other Materials</option>
                    </select>
                  </div>

                  <div className="ws-form-group flex-1">
                    <label>Title</label>
                    <input
                      type="text"
                      placeholder="e.g. Lecture 1: Introduction to Networks"
                      value={materialTitle}
                      onChange={(e) => setMaterialTitle(e.currentTarget.value)}
                    />
                  </div>
                </div>

                <div className="ws-form-row">
                  <div className="ws-form-group flex-1">
                    <label>Filename / Reference</label>
                    <input
                      type="text"
                      placeholder="e.g. lecture_01.pdf"
                      value={materialFilename}
                      onChange={(e) =>
                        setMaterialFilename(e.currentTarget.value)
                      }
                    />
                  </div>
                </div>

                <div className="ws-form-group">
                  <label>Notes / Key Takeaways</label>
                  <textarea
                    rows={3}
                    placeholder="Brief description or excerpt..."
                    value={materialPreview}
                    onChange={(e) => setMaterialPreview(e.currentTarget.value)}
                  />
                </div>

                <button type="submit" className="ws-primary-btn">
                  Save Material
                </button>
              </form>
            )}

            <div className="ws-category-blocks">
              <div className="ws-category-block">
                <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center", marginBottom: "12px" }}>
                  <h4 style={{ margin: 0 }}>Lectures ({lectures.length})</h4>
                  <button
                    type="button"
                    className="ws-mini-btn"
                    onClick={() => void handleNativeFileUpload("lectures")}
                    disabled={isUploading}
                    title="Upload to Lectures"
                  >
                    ↑ Upload
                  </button>
                </div>
                {lectures.length === 0 ? (
                  <span className="ws-empty-hint">No lecture materials added yet.</span>
                ) : (
                  <div className="ws-items-list">
                    {lectures.map((m) => (
                      <div key={m.id} className="ws-material-item">
                        <div>
                          <span className="ws-item-title">{m.title}</span>
                          <span className="ws-item-meta">{m.filename}</span>
                          {m.content_preview && (
                            <p className="ws-item-preview">{m.content_preview}</p>
                          )}
                        </div>
                        <button
                          type="button"
                          className="ws-item-del-btn"
                          onClick={() => handleDeleteMaterial(m.id)}
                        >
                          ×
                        </button>
                      </div>
                    ))}
                  </div>
                )}
              </div>

              <div className="ws-category-block">
                <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center", marginBottom: "12px" }}>
                  <h4 style={{ margin: 0 }}>Assignments ({assignments.length})</h4>
                  <button
                    type="button"
                    className="ws-mini-btn"
                    onClick={() => void handleNativeFileUpload("assignments")}
                    disabled={isUploading}
                    title="Upload to Assignments"
                  >
                    ↑ Upload
                  </button>
                </div>
                {assignments.length === 0 ? (
                  <span className="ws-empty-hint">No assignment materials added yet.</span>
                ) : (
                  <div className="ws-items-list">
                    {assignments.map((m) => (
                      <div key={m.id} className="ws-material-item">
                        <div>
                          <span className="ws-item-title">{m.title}</span>
                          <span className="ws-item-meta">{m.filename}</span>
                          {m.content_preview && (
                            <p className="ws-item-preview">{m.content_preview}</p>
                          )}
                        </div>
                        <button
                          type="button"
                          className="ws-item-del-btn"
                          onClick={() => handleDeleteMaterial(m.id)}
                        >
                          ×
                        </button>
                      </div>
                    ))}
                  </div>
                )}
              </div>

              <div className="ws-category-block">
                <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center", marginBottom: "12px" }}>
                  <h4 style={{ margin: 0 }}>Other Materials ({otherMaterials.length})</h4>
                  <button
                    type="button"
                    className="ws-mini-btn"
                    onClick={() => void handleNativeFileUpload("other")}
                    disabled={isUploading}
                    title="Upload to Other Materials"
                  >
                    ↑ Upload
                  </button>
                </div>
                {otherMaterials.length === 0 ? (
                  <span className="ws-empty-hint">No other materials added yet.</span>
                ) : (
                  <div className="ws-items-list">
                    {otherMaterials.map((m) => (
                      <div key={m.id} className="ws-material-item">
                        <div>
                          <span className="ws-item-title">{m.title}</span>
                          <span className="ws-item-meta">{m.filename}</span>
                          {m.content_preview && (
                            <p className="ws-item-preview">{m.content_preview}</p>
                          )}
                        </div>
                        <button
                          type="button"
                          className="ws-item-del-btn"
                          onClick={() => handleDeleteMaterial(m.id)}
                        >
                          ×
                        </button>
                      </div>
                    ))}
                  </div>
                )}
              </div>
            </div>
          </div>
        )}

        {/* NOTES */}
        {activeTab === "notes" && (
          <div className="ws-notes-section">
            <div className="ws-notes-sidebar">
              <div className="ws-notes-sidebar-header">
                <span>Notes ({notes.length})</span>
                <button
                  type="button"
                  className="ws-mini-btn"
                  onClick={createNewNote}
                >
                  ＋
                </button>
              </div>

              <div className="ws-notes-list">
                {notes.map((n) => (
                  <div
                    key={n.id}
                    className={`ws-note-item ${n.id === activeNoteId ? "active" : ""}`}
                    onClick={() => selectNote(n)}
                  >
                    <span className="ws-note-item-title">{n.title}</span>
                    <button
                      type="button"
                      className="ws-note-item-del"
                      onClick={(e) => {
                        e.stopPropagation();
                        void handleDeleteNote(n.id);
                      }}
                    >
                      ×
                    </button>
                  </div>
                ))}
              </div>
            </div>

            <div className="ws-note-editor">
              {noteError && (
                <div className="mm-notice" style={{ marginBottom: "10px" }}>
                  {noteError}
                </div>
              )}
              <div className="ws-note-editor-toolbar">
                <input
                  type="text"
                  className="ws-note-title-input"
                  placeholder="Note Title..."
                  value={noteTitle}
                  onChange={(e) => setNoteTitle(e.currentTarget.value)}
                />
                <button
                  type="button"
                  className="ws-primary-btn"
                  onClick={() => void handleSaveNote()}
                  disabled={isSavingNote}
                >
                  {isSavingNote
                    ? "Saving..."
                    : noteSavedToast
                      ? "✓ Saved"
                      : "Save Note"}
                </button>
              </div>

              <textarea
                className="ws-note-content-input"
                placeholder="Write your Markdown notes here..."
                value={noteContent}
                onChange={(e) => setNoteContent(e.currentTarget.value)}
              />
            </div>
          </div>
        )}

        {/* STUDY TOOLS */}
        {activeTab === "tools" && (
          <div className="ws-tools-section">
            <div className="ws-tools-grid">
              <div
                className="ws-tool-card"
                style={{ cursor: "pointer" }}
                onClick={() =>
                  onStartNewChat(
                    `Generate a 5-question practice quiz with answer explanations based on the study materials in ${workspace.name}.`
                  )
                }
              >
                <h4>Quiz Generator</h4>
                <p>
                  Generate practice questions and flashcards directly from this
                  workspace's notes and materials.
                </p>
                <span className="ws-tool-tag">Launch Quiz Prompt →</span>
              </div>

              <div
                className="ws-tool-card"
                style={{ cursor: "pointer" }}
                onClick={() =>
                  onStartNewChat(
                    `Create a structured concept summary and key-takeaways study guide from the uploaded materials in ${workspace.name}.`
                  )
                }
              >
                <h4>Concept Summarizer</h4>
                <p>
                  Create high-level study guides and cheat sheets grounded in
                  lecture materials.
                </p>
                <span className="ws-tool-tag">Launch Summary Prompt →</span>
              </div>

              <div
                className="ws-tool-card"
                style={{ cursor: "pointer" }}
                onClick={() =>
                  onStartNewChat(
                    `What are the core topics covered in ${workspace.name}? Cite the exact lecture or assignment filenames.`
                  )
                }
              >
                <h4>Source Grounded Q&A</h4>
                <p>
                  Ask questions with strict citations referencing specific lecture
                  slides or assignment briefs.
                </p>
                <span className="ws-tool-tag">Launch Grounded Q&A →</span>
              </div>
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
