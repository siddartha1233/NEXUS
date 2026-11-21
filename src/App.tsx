
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
  getAppSetting,
} from "./services/settings";
import {
  performWebSearch,
  buildWebSearchPromptContext,
  type WebSearchResult,
} from "./services/webSearch";
import { isCloudModel } from "./services/cloudModels";
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

// Applies a sliding context window budget so long chats never exceed free-tier TPM/payload or local VRAM limits
function applyContextWindowBudget(
  history: ChatMessage[],
  maxTurns = 16,
  maxTotalChars = 24000
): ChatMessage[] {
  const systemMsgs = history.filter((m) => m.role === "system");
  const nonSystem = history.filter((m) => m.role !== "system");

  const recent = nonSystem.slice(-maxTurns);
  const kept: ChatMessage[] = [];
  let charBudget = maxTotalChars - systemMsgs.reduce((acc, m) => acc + m.content.length, 0);

  // Always retain the latest user message, then walk backwards while budget remains
  for (let idx = recent.length - 1; idx >= 0; idx--) {
    const msg = recent[idx];
    const len = msg.content.length;
    if (kept.length === 0 || charBudget - len >= 0) {
      kept.unshift(msg);
      charBudget -= len;
    } else {
      break;
    }
  }

  return [...systemMsgs, ...kept];
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
  const [memorizedMessageKeys, setMemorizedMessageKeys] = useState<
    Set<string>
  >(new Set());
  const [savingMemoryKey, setSavingMemoryKey] = useState<string | null>(null);
  const [lastRetrievedSources, setLastRetrievedSources] = useState<string[]>(
    []
  );
  const [webSearchEnabled, setWebSearchEnabled] = useState(false);
  const [isSearchingWeb, setIsSearchingWeb] = useState(false);
  const [liveWebSources, setLiveWebSources] = useState<WebSearchResult[]>([]);
  const [webSearchNotice, setWebSearchNotice] = useState<string | null>(null);
  const [analytics, setAnalytics] = useState<AnalyticsData>({
    totalTokensGenerated: 0,
    totalPromptsSent: 0,
  });
  const [isGenerating, setIsGenerating] = useState(false);
  const [generationError, setGenerationError] = useState("");
  const [hasOlderMessages, setHasOlderMessages] = useState(false);
  const [isLoadingOlderMessages, setIsLoadingOlderMessages] = useState(false);
  const abortControllerRef = useRef<AbortController | null>(null);
  const conversationsReqIdRef = useRef(0);
  const chatContentRef = useRef<HTMLDivElement | null>(null);
  const chatBottomRef = useRef<HTMLDivElement | null>(null);
  const shouldAutoScrollRef = useRef(true);

  const handleChatScroll = useCallback(() => {
    const el = chatContentRef.current;
    if (!el) return;
    const distanceFromBottom = el.scrollHeight - el.scrollTop - el.clientHeight;
    shouldAutoScrollRef.current = distanceFromBottom < 96;
  }, []);

  useEffect(() => {
    if (activePage === "chat" && messages.length > 0 && shouldAutoScrollRef.current) {
      chatBottomRef.current?.scrollIntoView({
        behavior: isGenerating ? "auto" : "smooth",
        block: "end",
      });
    }
  }, [activePage, isGenerating, messages]);

  const {
    selectedModel,
    setSelectedModel,
    allModels,
    runningModels,
    setRunningModels,
    availableModels,
    cloudModels,
    connectionStatus,
    setConnectionStatus,
    isRefreshingModels,
    busyModel,
    modelManagerError,
    refreshOllamaState,
    handleSelectModel,
    handleUnloadModel,
    autoFallbackToLocal,
  } = useOllamaModels(isGenerating);

  const [workspaceStrictOffline, setWorkspaceStrictOffline] = useState(false);

  const resetActiveChatView = useCallback(() => {
    setActiveConversationId(null);
    setMessages([]);
    setHasOlderMessages(false);
    setMemorizedMessageKeys(new Set());
    setLastRetrievedSources([]);
    setLiveWebSources([]);
    setWebSearchNotice(null);
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

  const handleWorkspaceStrictOfflineChange = useCallback(
    (isStrict: boolean) => {
      setWorkspaceStrictOffline(isStrict);
      if (isStrict) {
        setWebSearchEnabled(false);
        if (isCloudModel(selectedModel)) {
          autoFallbackToLocal();
        }
      }
    },
    [autoFallbackToLocal, selectedModel]
  );

  // Sync workspace strict offline mode
  useEffect(() => {
    let cancelled = false;
    if (activeWorkspace) {
      void getAppSetting(`ws_offline_${activeWorkspace.id}`).then((val) => {
        if (!cancelled) {
          handleWorkspaceStrictOfflineChange(val === "true");
        }
      });
    } else {
      setWorkspaceStrictOffline(false);
    }
    return () => {
      cancelled = true;
    };
  }, [activeWorkspace, handleWorkspaceStrictOfflineChange]);

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
          setHasOlderMessages(false);
          setMemorizedMessageKeys(new Set());
          setLastRetrievedSources([]);
          setLiveWebSources([]);
          setWebSearchNotice(null);
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
      const pageSize = 500;
      const stored = await getConversationMessages(conv.id, { limit: pageSize });
      shouldAutoScrollRef.current = true;
      setActiveConversationId(conv.id);
      setMessages(
        stored.map((m) => ({
          id: m.id,
          role: m.role,
          content: m.content,
        }))
      );
      setHasOlderMessages(stored.length === pageSize);
      setMemorizedMessageKeys(new Set());
      setLastRetrievedSources([]);
      setLiveWebSources([]);
      setWebSearchNotice(null);
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

  async function handleLoadOlderMessages() {
    if (!activeConversationId || isLoadingOlderMessages || isGenerating) return;
    const oldestNumeric = messages.find((m) => typeof m.id === "number");
    if (!oldestNumeric || typeof oldestNumeric.id !== "number") {
      setHasOlderMessages(false);
      return;
    }

    setIsLoadingOlderMessages(true);
    try {
      const pageSize = 500;
      const older = await getConversationMessages(activeConversationId, {
        beforeId: oldestNumeric.id,
        limit: pageSize,
      });
      if (older.length > 0) {
        shouldAutoScrollRef.current = false;
        setMessages((prev) => [
          ...older.map((m) => ({
            id: m.id,
            role: m.role,
            content: m.content,
          })),
          ...prev,
        ]);
      }
      setHasOlderMessages(older.length === pageSize);
    } catch (error) {
      console.error("Failed to load older messages:", error);
    } finally {
      setIsLoadingOlderMessages(false);
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
        setHasOlderMessages(false);
        setMemorizedMessageKeys(new Set());
        setLastRetrievedSources([]);
        setLiveWebSources([]);
        setWebSearchNotice(null);
        setGenerationError("");
      }
    } catch (error) {
      console.error("Failed to delete conversation:", error);
    }
  }

  async function handleSaveToMemory(content: string, messageKey: string) {
    if (!content.trim() || savingMemoryKey !== null) return;
    setSavingMemoryKey(messageKey);
    try {
      const ok = await memorizeToMemora(content.trim(), "nexus_chat");
      if (ok) {
        setMemorizedMessageKeys((prev) => new Set(prev).add(messageKey));
        void refreshMemoraSources();
      }
    } catch (err) {
      console.error("Failed to memorize to MEMORA:", err);
    } finally {
      setSavingMemoryKey(null);
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

    shouldAutoScrollRef.current = true;
    setMessages([...conversationHistory, assistantMessage]);
    setMessage("");
    setIsGenerating(true);
    setGenerationError("");
    setWebSearchNotice(null);

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

    // 3. Live Web Search Grounding (Real-time current events, sports, news)
    if (webSearchEnabled && !workspaceStrictOffline) {
      setIsSearchingWeb(true);
      try {
        const searchResults = await performWebSearch(
          trimmed,
          activeWorkspace?.id ?? null
        );
        setLiveWebSources(searchResults);
        if (searchResults.length > 0) {
          const webGrounding = buildWebSearchPromptContext(searchResults);
          const lastIdx = effectiveHistory.length - 1;
          if (lastIdx >= 0 && effectiveHistory[lastIdx].role === "user") {
            effectiveHistory = [
              ...effectiveHistory.slice(0, lastIdx),
              {
                role: "user",
                content: `${webGrounding}\n\nUSER QUESTION:\n${effectiveHistory[lastIdx].content}`,
              },
            ];
          }
        } else {
          setWebSearchNotice(
            "🌐 Live web search returned no results for this query. Answering from model knowledge."
          );
        }
      } catch (err) {
        console.warn("Live web search failed, proceeding without web context:", err);
        const searchErrMsg = err instanceof Error ? err.message : String(err);
        setWebSearchNotice(
          searchErrMsg.includes("Strict Offline")
            ? `🔒 ${searchErrMsg}`
            : `🌐 Live web search unavailable (${searchErrMsg}). Answering from model knowledge.`
        );
      } finally {
        setIsSearchingWeb(false);
      }
    } else {
      setLiveWebSources([]);
    }

    const budgetedHistory = applyContextWindowBudget(effectiveHistory);

    try {
      await streamOllamaChat(
        selectedModel,
        budgetedHistory,
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
        },
        activeWorkspace?.id ?? null
      );

      if (!isCloudModel(selectedModel)) {
        setConnectionStatus("connected");
        void getRunningOllamaModels()
          .then(setRunningModels)
          .catch(() => {});
      }
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
        console.error("Chat streaming error:", error);
        const errMsg = error instanceof Error ? error.message : String(error);
        const isRateLimit = errMsg.includes("RATE_LIMIT") || errMsg.includes("429");
        const isAuthError = errMsg.includes("AUTH_ERROR");
        const isCloudNetworkDrop =
          errMsg.includes("Failed to connect to Groq") ||
          errMsg.includes("Stream interrupted from Groq") ||
          errMsg.includes("timed out") ||
          errMsg.includes("NETWORK_ERROR");

        if (isCloudModel(selectedModel) && (isRateLimit || isCloudNetworkDrop)) {
          const fallbackSetting = await getAppSetting("cloud_auto_fallback");
          const shouldAutoFallback = fallbackSetting === null || fallbackSetting === "true";
          const reasonLabel = isRateLimit
            ? "Groq free rate limit reached (30 RPM)"
            : "Groq cloud connection dropped or timed out";
          if (shouldAutoFallback) {
            const fallback = autoFallbackToLocal();
            setGenerationError(
              `⚡ ${reasonLabel}. Auto-switched to local model (${fallback}). Click Send to run locally.`
            );
          } else {
            setGenerationError(
              `⚡ ${reasonLabel}. Wait a moment or switch to a local model.`
            );
          }
        } else if (isAuthError) {
          setGenerationError(
            "🔑 Groq API key is missing or invalid. Open Settings -> Cloud Free-Tier Inference to add your free key."
          );
        } else {
          setGenerationError(errMsg);
        }

        if (
          error instanceof TypeError ||
          (error instanceof Error &&
            error.message.includes("Unable to connect to Ollama"))
        ) {
          setConnectionStatus("disconnected");
        }
      }

      if (!assistantText.trim()) {
        setLiveWebSources([]);
        setLastRetrievedSources([]);
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

  function startNewChat(prefillPrompt?: string) {
    if (isGenerating) return;

    setActiveConversationId(null);
    setMessages([]);
    setHasOlderMessages(false);
    setMemorizedMessageKeys(new Set());
    setLastRetrievedSources([]);
    setLiveWebSources([]);
    setWebSearchNotice(null);
    setGenerationError("");
    if (typeof prefillPrompt === "string") {
      setMessage(prefillPrompt);
    }
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
          onClick={() => startNewChat()}
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
                  if (isGenerating) {
                    setActivePage("chat");
                    return;
                  }
                  setActiveWorkspace(null);
                  resetActiveChatView();
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

          <button
            className="account-button"
            title="Open Settings & Account"
            onClick={() => setActivePage("settings")}
          >
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
            <span
              className="local-badge"
              title={
                isCloudModel(selectedModel)
                  ? "Connected to Groq Cloud (Free Tier)"
                  : "100% Local Ollama Inference"
              }
            >
              <span
                className="status-dot"
                style={
                  isCloudModel(selectedModel)
                    ? { background: "#fbbf24" }
                    : undefined
                }
              />
              {isCloudModel(selectedModel) ? "Cloud" : "Local"}
            </span>
          </div>
        </header>

        <ErrorBoundary key={activePage} fallbackTitle={pageTitles[activePage] || "Nexus View"}>
        {activePage === "chat" ? (
          <section className="chat-page">
            <div
              className="chat-content"
              ref={chatContentRef}
              onScroll={handleChatScroll}
            >
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
                  {hasOlderMessages && activeConversationId && (
                    <div style={{ display: "flex", justifyContent: "center", marginBottom: "12px" }}>
                      <button
                        type="button"
                        className="ws-secondary-btn"
                        onClick={() => void handleLoadOlderMessages()}
                        disabled={isLoadingOlderMessages || isGenerating}
                      >
                        {isLoadingOlderMessages ? "Loading older messages..." : "↑ Load older messages"}
                      </button>
                    </div>
                  )}
                  {messages.map((item, index) => {
                    const messageKey = String(item.id ?? `${item.role}-${index}`);
                    return (
                    <div className="user-message" key={messageKey}>
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

                        {item.role === "assistant" &&
                          index === messages.length - 1 &&
                          liveWebSources.length > 0 && (
                            <div className="retrieved-sources-pill" style={{ borderColor: "rgba(96, 165, 250, 0.4)" }}>
                              <span className="source-tag" style={{ color: "#60a5fa" }}>
                                🌐 Web Sources · {liveWebSources.map((s) => s.title).slice(0, 3).join(" · ")}
                              </span>
                            </div>
                          )}

                        {item.role === "assistant" && item.content && !isGenerating && (
                          <div className="message-actions-row">
                            <button
                              type="button"
                              className={`save-memory-btn ${
                                memorizedMessageKeys.has(messageKey) ? "saved" : ""
                              }`}
                              onClick={() => void handleSaveToMemory(item.content, messageKey)}
                              disabled={
                                memorizedMessageKeys.has(messageKey) ||
                                savingMemoryKey === messageKey
                              }
                            >
                              {memorizedMessageKeys.has(messageKey)
                                ? "✓ Saved to Vault"
                                : savingMemoryKey === messageKey
                                ? "Saving..."
                                : "⌘ Save to Memory"}
                            </button>
                          </div>
                        )}
                      </div>
                    </div>
                    );
                  })}

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
                  <div ref={chatBottomRef} />
                </div>
              )}
            </div>

            <div className="composer-area">
              {isSearchingWeb && (
                <div className="web-search-status-banner">
                  <span>🌐 Fetching live web results & current scores...</span>
                </div>
              )}
              {!isSearchingWeb && webSearchNotice && (
                <div
                  className="web-search-status-banner"
                  role="status"
                  style={{ justifyContent: "space-between", gap: "12px" }}
                >
                  <span>{webSearchNotice}</span>
                  <button
                    type="button"
                    className="feedback-dismiss"
                    aria-label="Dismiss web search notice"
                    onClick={() => setWebSearchNotice(null)}
                  >
                    ×
                  </button>
                </div>
              )}
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
                  placeholder={
                    webSearchEnabled
                      ? "Ask anything with live web access (e.g. today's F1 results, news, scores)..."
                      : "Message your AI..."
                  }
                  rows={2}
                  disabled={isGenerating}
                />

                <div className="composer-toolbar">
                  <div className="composer-tools">
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

                    <button
                      type="button"
                      className={`web-search-toggle ${webSearchEnabled && !workspaceStrictOffline ? "on" : ""}`}
                      onClick={() => {
                        if (!workspaceStrictOffline) {
                          setWebSearchEnabled(!webSearchEnabled);
                        }
                      }}
                      disabled={workspaceStrictOffline}
                      aria-pressed={webSearchEnabled && !workspaceStrictOffline}
                      title={
                        workspaceStrictOffline
                          ? "Strict Offline Mode is active for this workspace - web search is disabled"
                          : webSearchEnabled
                            ? "Live Web Search is ON - searches web for real-time news & scores"
                            : "Live Web Search is OFF - click to enable real-time web access"
                      }
                    >
                      <span className="web-search-dot" />
                      <span>🌐 Web</span>
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
                      <optgroup label="Local (Ollama 100% Offline)">
                        {availableModels.length > 0 ? (
                          availableModels.map((model) => (
                            <option key={model.name} value={model.name}>
                              💻 {model.name}
                            </option>
                          ))
                        ) : (
                          <>
                            <option value="qwen2.5-coder:7b">
                              💻 qwen2.5-coder:7b
                            </option>
                            <option value="qwen3:4b">💻 qwen3:4b</option>
                          </>
                        )}
                      </optgroup>
                      {!workspaceStrictOffline && (
                        <optgroup label="Cloud (Groq Free - Live)">
                          {cloudModels.length > 0 ? (
                            cloudModels.map((cm) => (
                              <option key={cm.id} value={cm.id}>
                                ⚡ {cm.name}
                              </option>
                            ))
                          ) : (
                            <option disabled value="">
                              Paste Groq key in Settings to load models
                            </option>
                          )}
                        </optgroup>
                      )}
                    </select>

                    {isCloudModel(selectedModel) ? (
                      <span
                        className="privacy-badge cloud"
                        title="Connected to Groq Cloud (Free Tier) · Excerpts processed by Groq"
                      >
                        ⚡ Cloud (Groq Free)
                      </span>
                    ) : (
                      <span
                        className="privacy-badge local"
                        title="100% Offline · Zero data leaves this machine"
                      >
                        🔒 100% Local
                      </span>
                    )}
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
              onStartNewChat={(prefillPrompt) => {
                startNewChat(prefillPrompt);
              }}
              onBack={handleExitWorkspace}
              onStrictOfflineChange={handleWorkspaceStrictOfflineChange}
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
