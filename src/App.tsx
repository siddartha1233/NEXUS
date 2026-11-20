
import { useCallback, useEffect, useRef, useState } from "react";
import "./App.css";
import { MessageRenderer } from "./components/MessageRenderer";
import { AnalyticsView, type AnalyticsData } from "./components/AnalyticsView";
import { ErrorBoundary } from "./components/ErrorBoundary";
import { KnowledgeView } from "./components/KnowledgeView";
import { ModelManager } from "./components/ModelManager";
import { SettingsView } from "./components/SettingsView";
import { SidebarIcon } from "./components/SidebarIcon";
import { WorkspaceDetail } from "./components/WorkspaceDetail";
import { WorkspacesView } from "./components/WorkspacesView";
import {
  exportConversationJson,
  exportConversationMarkdown,
} from "./services/settings";
import { recordTelemetry } from "./services/telemetry";
import {
  addConversationMessage,
  createConversation,
  deleteConversation,
  getConversationMessages,
  listConversations,
  renameConversation,
  type Conversation,
} from "./services/conversations";
import {
  getWorkspaceContext,
  type Workspace,
} from "./services/workspaces";
import { useWorkspaces } from "./hooks/useWorkspaces";
import {
  checkMemoraStatus,
  getMemoraContext,
  listMemoraSources,
  memorizeToMemora,
  type MemoraSourceItem,
} from "./services/memora";
import { useOllamaModels } from "./hooks/useOllamaModels";
import {
  getRunningOllamaModels,
  isEmbeddingModel,
  streamOllamaChat,
  type ChatMessage,
} from "./services/ollama";

type DisplayMessage = ChatMessage & { id?: string | number };

const navigation = [
  { id: "chat", label: "General Chat", icon: "chat" },
  { id: "workspaces", label: "Workspaces", icon: "workspaces" },
  { id: "knowledge", label: "Knowledge", icon: "knowledge" },
];

const management = [
  { id: "analytics", label: "Usage & Analytics", icon: "analytics" },
  { id: "models", label: "Model Manager", icon: "models" },
  { id: "settings", label: "Settings", icon: "settings" },
];

const pageTitles: Record<string, string> = {
  chat: "General Chat",
  workspaces: "Workspaces",
  knowledge: "Knowledge",
  analytics: "Usage & Analytics",
  models: "Model Manager",
  settings: "Settings",
};

function buildMemoraSystemPrompt(context: string): ChatMessage {
  return {
    role: "system",
    content: `You are Nexus. Below are excerpts retrieved from the user's personal Obsidian vault via MEMORA:\n\n${context}\n\nINSTRUCTIONS:\n- If the excerpts above are directly relevant to the user's question, use them to answer and cite the source filename (e.g., Welcome.md).\n- If the excerpts above are NOT relevant to the user's question, ignore them completely and answer the user's question directly using your general knowledge.\n- NEVER apologize, mention missing information, or list/describe unrelated vault files when the excerpts don't match the question.`,
  };
}

function App() {
  const [activePage, setActivePage] = useState("chat");
  const [sidebarOpen, setSidebarOpen] = useState(true);
  const [message, setMessage] = useState("");
  const [messages, setMessages] = useState<DisplayMessage[]>([]);
  const [conversations, setConversations] = useState<Conversation[]>([]);
  const [activeConversationId, setActiveConversationId] = useState<string | null>(
    null
  );
  const [editingConversationId, setEditingConversationId] = useState<
    string | null
  >(null);
  const [editingTitle, setEditingTitle] = useState("");
  const [memoraEnabled, setMemoraEnabled] = useState(true);
  const [memoraAvailable, setMemoraAvailable] = useState(false);
  const [memoraSources, setMemoraSources] = useState<MemoraSourceItem[]>([]);
  const [memorizedMessageIndices, setMemorizedMessageIndices] = useState<
    Set<number>
  >(new Set());
  const [savingMemoryIndex, setSavingMemoryIndex] = useState<number | null>(
    null
  );
  const [lastRetrievedSources, setLastRetrievedSources] = useState<string[]>(
    []
  );
  const [analytics, setAnalytics] = useState<AnalyticsData>({
    totalTokensGenerated: 0,
    totalPromptsSent: 0,
  });
  const [isGenerating, setIsGenerating] = useState(false);
  const [generationError, setGenerationError] = useState("");
  const abortControllerRef = useRef<AbortController | null>(null);
  const conversationsReqIdRef = useRef(0);

  const {
    selectedModel,
    setSelectedModel,
    allModels,
    runningModels,
    setRunningModels,
    availableModels,
    connectionStatus,
    setConnectionStatus,
    isRefreshingModels,
    busyModel,
    modelManagerError,
    refreshOllamaState,
    handleSelectModel,
    handleUnloadModel,
  } = useOllamaModels(isGenerating);

  const resetActiveChatView = useCallback(() => {
    setActiveConversationId(null);
    setMessages([]);
    setMemorizedMessageIndices(new Set());
    setLastRetrievedSources([]);
    setGenerationError("");
  }, []);

  const {
    workspaces,
    activeWorkspace,
    setActiveWorkspace,
    refreshWorkspaces,
    handleCreateWorkspace,
    handleDeleteWorkspace,
  } = useWorkspaces(resetActiveChatView);

  const refreshConversations = useCallback(async () => {
    const reqId = ++conversationsReqIdRef.current;
    try {
      const list = await listConversations(activeWorkspace?.id ?? null);
      if (reqId === conversationsReqIdRef.current) {
        setConversations(list);
      }
    } catch (error) {
      if (reqId === conversationsReqIdRef.current) {
        console.error("Failed to load conversations:", error);
      }
    }
  }, [activeWorkspace?.id]);

  const refreshMemoraSources = useCallback(async () => {
    try {
      const items = await listMemoraSources();
      setMemoraSources(items);
    } catch (error) {
      console.error("Failed to load MEMORA sources:", error);
    }
  }, []);

  useEffect(() => {
    void refreshOllamaState();
    void refreshWorkspaces();
    void checkMemoraStatus()
      .then((ready) => {
        setMemoraAvailable(ready);
        if (ready) void refreshMemoraSources();
      })
      .catch(() => {
        setMemoraAvailable(false);
      });
  }, [refreshOllamaState, refreshWorkspaces, refreshMemoraSources]);

  useEffect(() => {
    void refreshConversations();
  }, [refreshConversations]);

  useEffect(() => {
    function handleKeyDown(e: KeyboardEvent) {
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "n") {
        e.preventDefault();
        if (!isGenerating) {
          setActiveConversationId(null);
          setMessages([]);
          setMemorizedMessageIndices(new Set());
          setLastRetrievedSources([]);
          setGenerationError("");
          setActivePage("chat");
        }
      }
    }

    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [isGenerating]);

  useEffect(() => {
    if (activePage === "models") {
      void refreshOllamaState();
    }
  }, [activePage, refreshOllamaState]);

  async function openConversation(conv: Conversation) {
    if (isGenerating) return;

    try {
      const stored = await getConversationMessages(conv.id);
      setActiveConversationId(conv.id);
      setMessages(
        stored.map((m) => ({
          id: m.id,
          role: m.role,
          content: m.content,
        }))
      );
      setMemorizedMessageIndices(new Set());
      setLastRetrievedSources([]);
      setGenerationError("");
      setActivePage("chat");
    } catch (error) {
      console.error("Failed to open conversation:", error);
      setGenerationError(
        error instanceof Error
          ? error.message
          : "Unable to load conversation messages."
      );
    }
  }

  function beginRenameConversation(conv: Conversation) {
    setEditingConversationId(conv.id);
    setEditingTitle(conv.title);
  }

  async function commitRenameConversation(id: string) {
    const clean = editingTitle.trim();
    setEditingConversationId(null);

    if (!clean) return;

    try {
      await renameConversation(id, clean);
      setConversations((prev) =>
        prev.map((c) => (c.id === id ? { ...c, title: clean } : c))
      );
    } catch (error) {
      console.error("Failed to rename conversation:", error);
    }
  }

  async function handleDeleteConversation(id: string) {
    if (isGenerating && activeConversationId === id) return;

    try {
      await deleteConversation(id);
      setConversations((prev) => prev.filter((c) => c.id !== id));

      if (activeConversationId === id) {
        setActiveConversationId(null);
        setMessages([]);
        setMemorizedMessageIndices(new Set());
        setLastRetrievedSources([]);
        setGenerationError("");
      }
    } catch (error) {
      console.error("Failed to delete conversation:", error);
    }
  }

  async function handleSaveToMemory(content: string, index: number) {
    if (!content.trim() || savingMemoryIndex !== null) return;
    setSavingMemoryIndex(index);
    try {
      const ok = await memorizeToMemora(content.trim(), "nexus_chat");
      if (ok) {
        setMemorizedMessageIndices((prev) => new Set(prev).add(index));
        void refreshMemoraSources();
      }
    } catch (err) {
      console.error("Failed to memorize to MEMORA:", err);
    } finally {
      setSavingMemoryIndex(null);
    }
  }

  async function handleExportChat(format: "md" | "json") {
    if (!activeConversationId) return;
    const currentConv = conversations.find((c) => c.id === activeConversationId);
    const title = currentConv?.title || "conversation";

    try {
      if (format === "md") {
        await exportConversationMarkdown(activeConversationId, title);
      } else {
        await exportConversationJson(activeConversationId, title);
      }
    } catch (err) {
      console.error("Failed to export chat:", err);
      alert(err instanceof Error ? err.message : "Export failed.");
    }
  }

  async function sendMessage(e: React.FormEvent<HTMLFormElement>) {
    e.preventDefault();

    const trimmed = message.trim();

    if (!trimmed || isGenerating) return;

    const userMessage: DisplayMessage = {
      id: `user-${Date.now()}-${Math.random().toString(36).slice(2, 7)}`,
      role: "user",
      content: trimmed,
    };

    const assistantMessage: DisplayMessage = {
      id: `assistant-${Date.now()}-${Math.random().toString(36).slice(2, 7)}`,
      role: "assistant",
      content: "",
    };

    const conversationHistory = [...messages, userMessage];

    setMessages([...conversationHistory, assistantMessage]);
    setMessage("");
    setIsGenerating(true);
    setGenerationError("");

    let convId = activeConversationId;
    try {
      if (!convId) {
        const normalized = trimmed.replace(/\s+/g, " ");
        const autoTitle =
          normalized.length > 44 ? `${normalized.slice(0, 44)}…` : normalized;
        const created = await createConversation(
          autoTitle,
          selectedModel,
          activeWorkspace?.id ?? null
        );
        convId = created.id;
        setActiveConversationId(created.id);
        setConversations((prev) => [created, ...prev]);
      }

      await addConversationMessage(convId, "user", trimmed, selectedModel);
    } catch (error) {
      console.error("Failed to persist user message:", error);
    }

    const controller = new AbortController();
    abortControllerRef.current = controller;
    let assistantText = "";
    setLastRetrievedSources([]);

    // Retrieve grounding context from active workspace materials or MEMORA
    let effectiveHistory: ChatMessage[] = conversationHistory;

    if (activeWorkspace) {
      // Prioritize active isolated workspace materials
      try {
        const wsRes = await getWorkspaceContext(activeWorkspace.id, trimmed);
        if (wsRes.available && wsRes.context.trim()) {
          setLastRetrievedSources(
            wsRes.sources.map((s) => `Workspace: ${s}`)
          );
          const systemGroundingPrompt: ChatMessage = {
            role: "system",
            content: `You are Nexus assisting inside the isolated workspace "${activeWorkspace.name}". You have access to the workspace's study materials:\n\n${wsRes.context}\n\nINSTRUCTIONS:\n- Answer the user's question accurately using the study materials above.\n- Clearly cite the relevant material title and filename.\n- If the materials do not contain the answer, say so clearly. Do not fabricate facts.`,
          };
          effectiveHistory = [systemGroundingPrompt, ...conversationHistory];
        } else if (memoraEnabled) {
          // Fall back to MEMORA vault if workspace materials don't have matches
          const memoraRes = await getMemoraContext(trimmed);
          if (memoraRes.available && memoraRes.context.trim()) {
            setLastRetrievedSources(
              memoraRes.sources.length > 0 ? memoraRes.sources : ["MEMORA Vault"]
            );
            effectiveHistory = [
              buildMemoraSystemPrompt(memoraRes.context),
              ...conversationHistory,
            ];
          }
        }
      } catch (err) {
        console.warn("Workspace context retrieval error:", err);
      }
    } else if (memoraEnabled) {
      // In General Chat: use MEMORA personal vault
      try {
        const memoraRes = await getMemoraContext(trimmed);
        if (memoraRes.error) {
          console.warn("MEMORA error:", memoraRes.error);
          setGenerationError(`MEMORA warning: ${memoraRes.error}`);
        } else if (memoraRes.available && memoraRes.context.trim()) {
          setLastRetrievedSources(
            memoraRes.sources.length > 0 ? memoraRes.sources : ["MEMORA Vault"]
          );
          effectiveHistory = [
            buildMemoraSystemPrompt(memoraRes.context),
            ...conversationHistory,
          ];
        }
      } catch (err) {
        console.warn("MEMORA retrieval failed, proceeding with model:", err);
      }
    }

    try {
      await streamOllamaChat(
        selectedModel,
        effectiveHistory,
        (token) => {
          assistantText += token;
          setMessages((previous) => {
            const updated = [...previous];
            const lastIndex = updated.length - 1;

            if (updated[lastIndex]?.role === "assistant") {
              updated[lastIndex] = {
                ...updated[lastIndex],
                content: updated[lastIndex].content + token,
              };
            }

            return updated;
          });
        },
        controller.signal,
        (metrics) => {
          setAnalytics((prev) => ({
            totalTokensGenerated:
              prev.totalTokensGenerated + (metrics.evalCount ?? 0),
            totalPromptsSent: prev.totalPromptsSent + 1,
            lastTokensPerSec: metrics.tokensPerSec,
            lastTotalDurationMs: metrics.totalDurationMs,
            lastLoadDurationMs: metrics.loadDurationMs,
            lastOutputTokens: metrics.evalCount,
            lastPromptTokens: metrics.promptEvalCount,
          }));

          void recordTelemetry(
            selectedModel,
            metrics.promptEvalCount ?? 0,
            metrics.evalCount ?? 0,
            metrics.totalDurationMs ?? 0,
            metrics.tokensPerSec
          );
        }
      );

      setConnectionStatus("connected");
      void getRunningOllamaModels()
        .then(setRunningModels)
        .catch(() => {});
    } catch (error) {
      const isAbort =
        error instanceof DOMException && error.name === "AbortError";

      if (isAbort) {
        if (assistantText.trim().length > 0) {
          const stoppedSuffix = "\n\n_[Generation stopped]_";
          assistantText += stoppedSuffix;
          setMessages((previous) => {
            const updated = [...previous];
            const lastIndex = updated.length - 1;
            if (updated[lastIndex]?.role === "assistant") {
              updated[lastIndex] = {
                ...updated[lastIndex],
                content: updated[lastIndex].content + stoppedSuffix,
              };
            }
            return updated;
          });
        }
      } else {
        console.error("Ollama chat error:", error);

        setGenerationError(
          error instanceof Error
            ? error.message
            : "Something went wrong while generating a response."
        );

        if (
          error instanceof TypeError ||
          (error instanceof Error &&
            error.message.includes("Unable to connect to Ollama"))
        ) {
          setConnectionStatus("disconnected");
        }
      }

      setMessages((previous) => {
        const last = previous[previous.length - 1];
        if (last?.role === "assistant" && !last.content) {
          return previous.slice(0, -1);
        }
        return previous;
      });
    } finally {
      if (convId && assistantText.trim().length > 0) {
        try {
          await addConversationMessage(
            convId,
            "assistant",
            assistantText,
            selectedModel
          );
          void refreshConversations();
        } catch (error) {
          console.error("Failed to persist assistant message:", error);
        }
      }

      abortControllerRef.current = null;
      setIsGenerating(false);
    }
  }

  function stopGeneration() {
    abortControllerRef.current?.abort();
  }

  function handleSelectWorkspace(ws: Workspace) {
    if (isGenerating) return;
    setActiveWorkspace(ws);
    resetActiveChatView();
  }

  function handleExitWorkspace() {
    if (isGenerating) return;
    setActiveWorkspace(null);
    resetActiveChatView();
    setActivePage("workspaces");
  }

  function startNewChat() {
    if (isGenerating) return;

    setActiveConversationId(null);
    setMessages([]);
    setMemorizedMessageIndices(new Set());
    setLastRetrievedSources([]);
    setGenerationError("");
    setActivePage("chat");
  }

  const connectionLabel =
    connectionStatus === "checking"
      ? "Checking Ollama..."
      : connectionStatus === "connected"
        ? "Ollama connected"
        : "Ollama disconnected";

  return (
    <div className="app-shell">
      <aside className={`sidebar ${sidebarOpen ? "sidebar-open" : "sidebar-closed"}`}>
        <div className="sidebar-brand">
          <div className="brand-mark">N</div>
          {sidebarOpen && (
            <div className="brand-copy">
              <span className="brand-name">NEXUS</span>
              <span className="brand-caption">PERSONAL AI PLATFORM</span>
            </div>
          )}
        </div>

        <button
          className="new-chat-button"
          onClick={startNewChat}
          disabled={isGenerating}
        >
          <span className="nav-icon"><SidebarIcon name="plus" /></span>
          {sidebarOpen && <span>New chat</span>}
          {sidebarOpen && <span className="shortcut">Ctrl N</span>}
        </button>

        <div className="sidebar-section">
          {sidebarOpen && <span className="section-label">WORKSPACE</span>}

          {navigation.map((item) => (
            <button
              key={item.id}
              className={`nav-item ${activePage === item.id ? "active" : ""}`}
              onClick={() => {
                if (item.id === "chat" && activeWorkspace) {
                  setActiveWorkspace(null);
                  setActiveConversationId(null);
                  setMessages([]);
                }
                setActivePage(item.id);
              }}
              title={!sidebarOpen ? item.label : undefined}
            >
              <span className="nav-icon"><SidebarIcon name={item.icon} /></span>
              {sidebarOpen && <span>{item.label}</span>}
            </button>
          ))}
        </div>

        {sidebarOpen && conversations.length > 0 && (
          <div className="sidebar-section recent-chats-section">
            <span className="section-label">
              {activeWorkspace ? `${activeWorkspace.name.toUpperCase()} CHATS` : "RECENT CHATS"}
              <span className="section-count">{conversations.length}</span>
            </span>

            <div className="recent-chats-list">
              {conversations.map((conv) => {
                const isActive =
                  activePage === "chat" && activeConversationId === conv.id;
                const isEditing = editingConversationId === conv.id;

                return (
                  <div
                    key={conv.id}
                    className={`chat-history-item ${isActive ? "active" : ""}`}
                  >
                    {isEditing ? (
                      <input
                        type="text"
                        className="chat-rename-input"
                        value={editingTitle}
                        autoFocus
                        onChange={(e) => setEditingTitle(e.currentTarget.value)}
                        onBlur={() => void commitRenameConversation(conv.id)}
                        onKeyDown={(e) => {
                          if (e.key === "Enter") {
                            e.preventDefault();
                            void commitRenameConversation(conv.id);
                          } else if (e.key === "Escape") {
                            setEditingConversationId(null);
                          }
                        }}
                      />
                    ) : (
                      <>
                        <button
                          type="button"
                          className="chat-history-select"
                          onClick={() => void openConversation(conv)}
                          disabled={isGenerating}
                          title={conv.title}
                        >
                          <span className="chat-history-title">
                            {conv.title}
                          </span>
                        </button>

                        <div className="chat-history-actions">
                          <button
                            type="button"
                            className="chat-action-btn"
                            title="Rename chat"
                            onClick={() => beginRenameConversation(conv)}
                            disabled={isGenerating}
                          >
                            ✎
                          </button>
                          <button
                            type="button"
                            className="chat-action-btn"
                            title="Delete chat"
                            onClick={() =>
                              void handleDeleteConversation(conv.id)
                            }
                            disabled={isGenerating && isActive}
                          >
                            ×
                          </button>
                        </div>
                      </>
                    )}
                  </div>
                );
              })}
            </div>
          </div>
        )}

        <div className="sidebar-section management-section">
          {sidebarOpen && <span className="section-label">MANAGEMENT</span>}

          {management.map((item) => (
            <button
              key={item.id}
              className={`nav-item ${activePage === item.id ? "active" : ""}`}
              onClick={() => setActivePage(item.id)}
              title={!sidebarOpen ? item.label : undefined}
            >
              <span className="nav-icon"><SidebarIcon name={item.icon} /></span>
              {sidebarOpen && <span>{item.label}</span>}
            </button>
          ))}
        </div>

        <div className="sidebar-bottom">
          <div
            className="connection-status"
            title={`${connectionLabel} · ${memoraAvailable ? "MEMORA ready" : "MEMORA idle"}`}
          >
            <span
              className={`status-dot ${
                connectionStatus === "connected" ? "status-dot-ok" : ""
              }`}
            />
            {sidebarOpen && (
              <span className="status-line">
                {connectionLabel} · {memoraAvailable ? "MEMORA ready" : "MEMORA idle"}
              </span>
            )}
          </div>

          <button className="account-button" title="Account">
            <div className="avatar">S</div>
            {sidebarOpen && (
              <div className="account-copy">
                <span className="account-name">My Account</span>
                <span className="account-caption">Personal workspace</span>
              </div>
            )}
            {sidebarOpen && <span className="account-menu">•••</span>}
          </button>
        </div>
      </aside>

      <main className="main-area">
        <header className="topbar">
          <div className="topbar-left">
            <button
              className="icon-button sidebar-toggle"
              onClick={() => setSidebarOpen(!sidebarOpen)}
              title="Toggle sidebar"
            >
              ☰
            </button>

            <span className="breadcrumb">Nexus</span>
            {activeWorkspace ? (
              <>
                <span className="breadcrumb-separator">/</span>
                <span
                  className="breadcrumb-link"
                  onClick={() => {
                    if (!isGenerating) {
                      setActivePage("workspaces");
                    }
                  }}
                  style={{ cursor: "pointer" }}
                >
                  Workspaces
                </span>
                <span className="breadcrumb-separator">/</span>
                <span className="breadcrumb-current">{activeWorkspace.name}</span>
              </>
            ) : (
              <>
                <span className="breadcrumb-separator">/</span>
                <span className="breadcrumb-current">{pageTitles[activePage]}</span>
              </>
            )}
          </div>

          <div className="topbar-right">
            {activePage === "chat" && activeConversationId && messages.length > 0 && (
              <div className="topbar-export-menu">
                <button
                  type="button"
                  className="export-chat-btn"
                  onClick={() => void handleExportChat("md")}
                  title="Export conversation as Markdown"
                >
                  ↓ Export .md
                </button>
                <button
                  type="button"
                  className="export-chat-btn"
                  onClick={() => void handleExportChat("json")}
                  title="Export conversation as JSON"
                >
                  ↓ .json
                </button>
              </div>
            )}
            <span className="local-badge">
              <span className="status-dot" />
              Local
            </span>
          </div>
        </header>

        <ErrorBoundary key={activePage} fallbackTitle={pageTitles[activePage] || "Nexus View"}>
        {activePage === "chat" ? (
          <section className="chat-page">
            <div className="chat-content">
              {messages.length === 0 ? (
                <div className="welcome-content">
                  <h1>
                    What would you like<br />
                    to <span>explore</span> today?
                  </h1>

                  <p className="welcome-description">
                    Think, learn, create and solve problems with your personal AI platform.
                    Your workspace starts here.
                  </p>
                </div>
              ) : (
                <div className="message-list">
                  {messages.map((item, index) => (
                    <div className="user-message" key={item.id ?? `${item.role}-${index}`}>
                      <div className="message-avatar">
                        {item.role === "user" ? "S" : "N"}
                      </div>

                      <div>
                        <div className="message-author">
                          {item.role === "user" ? "You" : "Nexus"}
                        </div>

                        {item.content ? (
                          <MessageRenderer content={item.content} />
                        ) : isGenerating && index === messages.length - 1 ? (
                          <p style={{ whiteSpace: "pre-wrap", color: "#888888" }}>
                            Thinking...
                          </p>
                        ) : null}

                        {item.role === "assistant" &&
                          index === messages.length - 1 &&
                          lastRetrievedSources.length > 0 && (
                            <div className="retrieved-sources-pill">
                              <span className="source-tag">
                                ⌘ MEMORA · {lastRetrievedSources.join(", ")}
                              </span>
                            </div>
                          )}

                        {item.role === "assistant" && item.content && !isGenerating && (
                          <div className="message-actions-row">
                            <button
                              type="button"
                              className={`save-memory-btn ${
                                memorizedMessageIndices.has(index) ? "saved" : ""
                              }`}
                              onClick={() => void handleSaveToMemory(item.content, index)}
                              disabled={
                                memorizedMessageIndices.has(index) ||
                                savingMemoryIndex === index
                              }
                            >
                              {memorizedMessageIndices.has(index)
                                ? "✓ Saved to Vault"
                                : savingMemoryIndex === index
                                ? "Saving..."
                                : "⌘ Save to Memory"}
                            </button>
                          </div>
                        )}
                      </div>
                    </div>
                  ))}

                  {isGenerating && (
                    <div className="local-placeholder">
                      <span className="status-dot" />
                      Generating response with {selectedModel}...
                    </div>
                  )}

                  {generationError && (
                    <div className="local-placeholder">
                      <span className="status-dot" />
                      {generationError}
                    </div>
                  )}
                </div>
              )}
            </div>

            <div className="composer-area">
              <form className="composer" onSubmit={sendMessage}>
                <textarea
                  value={message}
                  onChange={(e) => setMessage(e.currentTarget.value)}
                  onKeyDown={(e) => {
                    if (e.key === "Enter" && !e.shiftKey) {
                      e.preventDefault();
                      e.currentTarget.form?.requestSubmit();
                    }
                  }}
                  placeholder="Message your AI..."
                  rows={2}
                  disabled={isGenerating}
                />

                <div className="composer-toolbar">
                  <div className="composer-tools">
                    <button
                      type="button"
                      className="tool-button"
                      title="Attach file"
                    >
                      ＋
                    </button>

                    <button
                      type="button"
                      className={`memory-toggle ${memoraEnabled ? "on" : ""}`}
                      onClick={() => setMemoraEnabled(!memoraEnabled)}
                      aria-pressed={memoraEnabled}
                      title={
                        memoraEnabled
                          ? "MEMORA memory is ON - click to turn off"
                          : "MEMORA memory is OFF - click to turn on"
                      }
                    >
                      <span className="memory-toggle-dot" />
                      <span>Memory</span>
                    </button>

                    <span className="tool-divider" />
                    <span className="model-label">MODEL</span>

                    <select
                      value={selectedModel}
                      onChange={(e) =>
                        void handleSelectModel(e.currentTarget.value)
                      }
                      aria-label="Select model"
                      disabled={isGenerating || busyModel !== null}
                    >
                      {availableModels.length > 0 ? (
                        availableModels.map((model) => (
                          <option key={model.name} value={model.name}>
                            {model.name}
                          </option>
                        ))
                      ) : (
                        <>
                          <option value="qwen2.5-coder:7b">
                            qwen2.5-coder:7b
                          </option>
                          <option value="qwen3:4b">qwen3:4b</option>
                        </>
                      )}
                    </select>
                  </div>

                  {isGenerating ? (
                    <button
                      type="button"
                      className="send-button"
                      onClick={stopGeneration}
                      title="Stop generation"
                    >
                      ■
                    </button>
                  ) : (
                    <button
                      type="submit"
                      className="send-button"
                      disabled={!message.trim()}
                      title="Send message"
                    >
                      ↑
                    </button>
                  )}
                </div>
              </form>

              <p className="composer-disclaimer">
                Local-first AI · Your data stays on your device
              </p>
            </div>
          </section>
        ) : activePage === "workspaces" ? (
          activeWorkspace ? (
            <WorkspaceDetail
              workspace={activeWorkspace}
              conversations={conversations}
              selectedModel={selectedModel}
              onOpenConversation={(conv) => {
                void openConversation(conv);
              }}
              onStartNewChat={() => {
                startNewChat();
              }}
              onBack={handleExitWorkspace}
            />
          ) : (
            <WorkspacesView
              workspaces={workspaces}
              activeWorkspaceId={activeWorkspace ? (activeWorkspace as Workspace).id : null}
              onSelectWorkspace={handleSelectWorkspace}
              onCreateWorkspace={handleCreateWorkspace}
              onDeleteWorkspace={handleDeleteWorkspace}
            />
          )
        ) : activePage === "knowledge" ? (
          <KnowledgeView
            sources={memoraSources}
            memoraReady={memoraAvailable}
            onRefreshSources={refreshMemoraSources}
          />
        ) : activePage === "analytics" ? (
          <ErrorBoundary fallbackTitle="Usage & Analytics">
            <AnalyticsView
              analytics={analytics}
              runningModels={runningModels}
              selectedModel={selectedModel}
              totalConversations={conversations.length}
            />
          </ErrorBoundary>
        ) : activePage === "models" ? (
          <ModelManager
            models={allModels}
            runningModels={runningModels}
            selectedModel={selectedModel}
            connectionStatus={connectionStatus}
            isRefreshing={isRefreshingModels}
            isGenerating={isGenerating}
            busyModel={busyModel}
            error={modelManagerError}
            onSelectModel={(name) => void handleSelectModel(name)}
            onUnloadModel={(name) => void handleUnloadModel(name)}
            onRefresh={() => void refreshOllamaState()}
          />
        ) : activePage === "settings" ? (
          <SettingsView
            selectedModel={selectedModel}
            onSelectModel={(model) => setSelectedModel(model)}
            availableModels={
              allModels.length > 0
                ? allModels.filter((m) => !isEmbeddingModel(m.name)).map((m) => m.name)
                : ["qwen2.5-coder:7b", "qwen3:4b"]
            }
            ollamaStatus={connectionStatus}
            memoraReady={memoraAvailable}
            onRefreshAll={async () => {
              await refreshOllamaState();
              await refreshWorkspaces();
              await refreshConversations();
            }}
          />
        ) : (
          <section className="placeholder-page">
            <div className="placeholder-icon">✳</div>
            <span className="eyebrow">NEXUS PLATFORM</span>
            <h1>{pageTitles[activePage]}</h1>
            <p>This section will be built in a later development milestone.</p>

            <button
              className="back-to-chat"
              onClick={() => setActivePage("chat")}
            >
              Return to General Chat <span>→</span>
            </button>
          </section>
        )}
        </ErrorBoundary>
      </main>
    </div>
  );
}

export default App;
