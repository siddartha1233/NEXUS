import { useEffect, useState } from "react";
import { type RunningOllamaModel } from "../services/ollama";
import {
  getLocalDateString,
  getTelemetryStats,
  type TelemetryStats,
} from "../services/telemetry";

export interface AnalyticsData {
  totalTokensGenerated: number;
  totalPromptsSent: number;
  lastTokensPerSec?: number;
  lastTotalDurationMs?: number;
  lastLoadDurationMs?: number;
  lastOutputTokens?: number;
  lastPromptTokens?: number;
}

interface AnalyticsViewProps {
  analytics: AnalyticsData;
  runningModels: RunningOllamaModel[];
  selectedModel: string;
  totalConversations: number;
}

function formatBytes(bytes: number): string {
  if (!bytes || bytes <= 0) return "0 MB";
  const mb = bytes / (1024 * 1024);
  if (mb >= 1024) {
    return `${(mb / 1024).toFixed(2)} GB`;
  }
  return `${mb.toFixed(0)} MB`;
}

export function AnalyticsView({
  analytics,
  runningModels,
  selectedModel,
  totalConversations,
}: AnalyticsViewProps) {
  const [telemetry, setTelemetry] = useState<TelemetryStats | null>(null);
  const [loading, setLoading] = useState(true);

  const activeLoaded = runningModels.find((m) => m.name === selectedModel);
  const todayStr = getLocalDateString();

  useEffect(() => {
    let isMounted = true;
    void getTelemetryStats().then((data) => {
      if (isMounted) {
        setTelemetry(data);
        setLoading(false);
      }
    });

    return () => {
      isMounted = false;
    };
  }, [analytics.totalPromptsSent]);

  const todayTokens = Number(
    telemetry
      ? (telemetry.todayTokens ?? 0)
      : (analytics?.totalTokensGenerated ?? 0)
  );
  const todayPrompts = Number(
    telemetry
      ? (telemetry.todayPrompts ?? 0)
      : (analytics?.totalPromptsSent ?? 0)
  );
  const allTimeTokens = Number(
    telemetry
      ? (telemetry.allTimeTokens ?? 0)
      : (analytics?.totalTokensGenerated ?? 0)
  );
  const allTimePrompts = Number(
    telemetry
      ? (telemetry.allTimePrompts ?? 0)
      : (analytics?.totalPromptsSent ?? 0)
  );

  const dailyHistoryList = Array.isArray(telemetry?.dailyHistory)
    ? telemetry.dailyHistory
    : [];

  return (
    <div className="analytics-page">
      <div className="analytics-header">
        <div>
          <h1>Usage & Performance Analytics</h1>
          <p className="analytics-subtitle">
            Local telemetry tracking inference performance, token generation speed, and hardware utilization on your machine.
          </p>
        </div>
      </div>

      {/* KEY PERFORMANCE TILES */}
      <div className="analytics-grid">
        <div className="analytics-card">
          <span className="analytics-card-label">TODAY'S USAGE</span>
          <div className="analytics-metric">
            <span className="metric-value">
              {loading ? "…" : todayTokens.toLocaleString()}
            </span>
            <span className="metric-unit">tokens</span>
          </div>
          <span className="analytics-card-sub">
            Across {loading ? "…" : todayPrompts} prompt{todayPrompts === 1 ? "" : "s"} today
          </span>
        </div>

        <div className="analytics-card">
          <span className="analytics-card-label">ALL-TIME USAGE</span>
          <div className="analytics-metric">
            <span className="metric-value">
              {loading ? "…" : allTimeTokens.toLocaleString()}
            </span>
            <span className="metric-unit">tokens</span>
          </div>
          <span className="analytics-card-sub">
            Across {loading ? "…" : allTimePrompts} prompt{allTimePrompts === 1 ? "" : "s"} total
          </span>
        </div>

        <div className="analytics-card">
          <span className="analytics-card-label">GENERATION SPEED</span>
          <div className="analytics-metric">
            <span className="metric-value">
              {analytics?.lastTokensPerSec ? `${analytics.lastTokensPerSec}` : "—"}
            </span>
            <span className="metric-unit">tokens/sec</span>
          </div>
          <span className="analytics-card-sub">
            {analytics?.lastTokensPerSec
              ? `Measured on ${selectedModel}`
              : "Generate a message to measure"}
          </span>
        </div>

        <div className="analytics-card">
          <span className="analytics-card-label">VRAM / RAM UTILIZATION</span>
          <div className="analytics-metric">
            <span className="metric-value">
              {activeLoaded ? formatBytes(activeLoaded.size_vram || activeLoaded.size) : "—"}
            </span>
          </div>
          <span className="analytics-card-sub">
            {activeLoaded
              ? `${activeLoaded.name} resident in memory`
              : "No chat model resident in memory"}
          </span>
        </div>
      </div>

      {/* 3-TIER COMPARISON SECTION */}
      <div className="analytics-details-section">
        <h2>Usage Scope Breakdown</h2>
        <div className="analytics-scope-grid">
          {/* SESSION SCOPE */}
          <div className="analytics-scope-card">
            <div className="scope-badge">ACTIVE SESSION</div>
            <div className="scope-tokens">
              {(analytics?.totalTokensGenerated ?? 0).toLocaleString()}
            </div>
            <div className="scope-tokens-label">tokens generated</div>
            <div className="scope-divider" />
            <div className="scope-meta-row">
              <span>Prompts Processed</span>
              <span className="scope-meta-val">{analytics?.totalPromptsSent ?? 0}</span>
            </div>
            <div className="scope-meta-row">
              <span>Last Latency</span>
              <span className="scope-meta-val">
                {analytics?.lastTotalDurationMs
                  ? `${(analytics.lastTotalDurationMs / 1000).toFixed(2)}s`
                  : "—"}
              </span>
            </div>
          </div>

          {/* TODAY SCOPE */}
          <div className="analytics-scope-card featured">
            <div className="scope-badge">TODAY ({todayStr})</div>
            <div className="scope-tokens">{todayTokens.toLocaleString()}</div>
            <div className="scope-tokens-label">total tokens today</div>
            <div className="scope-divider" />
            <div className="scope-meta-row">
              <span>Prompts Today</span>
              <span className="scope-meta-val">{todayPrompts}</span>
            </div>
            <div className="scope-meta-row">
              <span>Prompt / Input Tokens</span>
              <span className="scope-meta-val">
                {telemetry ? (telemetry.todayPromptTokens ?? 0).toLocaleString() : "—"}
              </span>
            </div>
            <div className="scope-meta-row">
              <span>Completion / Output Tokens</span>
              <span className="scope-meta-val">
                {telemetry ? (telemetry.todayCompletionTokens ?? 0).toLocaleString() : "—"}
              </span>
            </div>
          </div>

          {/* ALL-TIME SCOPE */}
          <div className="analytics-scope-card">
            <div className="scope-badge">ALL-TIME (LIFETIME)</div>
            <div className="scope-tokens">{allTimeTokens.toLocaleString()}</div>
            <div className="scope-tokens-label">lifetime tokens</div>
            <div className="scope-divider" />
            <div className="scope-meta-row">
              <span>Total Prompts</span>
              <span className="scope-meta-val">{allTimePrompts}</span>
            </div>
            <div className="scope-meta-row">
              <span>All-Time Input Tokens</span>
              <span className="scope-meta-val">
                {telemetry ? (telemetry.allTimePromptTokens ?? 0).toLocaleString() : "—"}
              </span>
            </div>
            <div className="scope-meta-row">
              <span>All-Time Output Tokens</span>
              <span className="scope-meta-val">
                {telemetry ? (telemetry.allTimeCompletionTokens ?? 0).toLocaleString() : "—"}
              </span>
            </div>
          </div>
        </div>
      </div>

      {/* DAILY USAGE HISTORY */}
      {dailyHistoryList.length > 0 && (
        <div className="analytics-details-section">
          <h2>Daily Usage History (Last 14 Days)</h2>
          <div className="analytics-table-card">
            <div className="daily-table-header">
              <span className="daily-col-date">Date</span>
              <span className="daily-col-num">Prompts</span>
              <span className="daily-col-num">Input Tokens</span>
              <span className="daily-col-num">Output Tokens</span>
              <span className="daily-col-num">Total Tokens</span>
            </div>
            {dailyHistoryList.map((day) => (
              <div
                key={day.date}
                className={`daily-table-row ${day.date === todayStr ? "is-today" : ""}`}
              >
                <span className="daily-col-date">
                  {day.date === todayStr ? `Today (${day.date})` : day.date}
                </span>
                <span className="daily-col-num">{day.prompts ?? 0}</span>
                <span className="daily-col-num">{(day.promptTokens ?? 0).toLocaleString()}</span>
                <span className="daily-col-num">{(day.completionTokens ?? 0).toLocaleString()}</span>
                <span className="daily-col-num font-bold">{(day.totalTokens ?? 0).toLocaleString()}</span>
              </div>
            ))}
          </div>
        </div>
      )}

      {/* INFERENCE & PLATFORM METRICS TABLE */}
      <div className="analytics-details-section">
        <h2>Inference & Platform Metrics</h2>
        <div className="analytics-table-card">
          <div className="analytics-row">
            <span className="row-label">Active Chat Model</span>
            <span className="row-value">{selectedModel}</span>
          </div>
          <div className="analytics-row">
            <span className="row-label">Prompt Input Tokens (Last Request)</span>
            <span className="row-value">{analytics?.lastPromptTokens ?? "—"}</span>
          </div>
          <div className="analytics-row">
            <span className="row-label">Output Completion Tokens (Last Request)</span>
            <span className="row-value">{analytics?.lastOutputTokens ?? "—"}</span>
          </div>
          <div className="analytics-row">
            <span className="row-label">Saved Conversations in SQLite</span>
            <span className="row-value">{totalConversations}</span>
          </div>
          <div className="analytics-row">
            <span className="row-label">Persistence Database</span>
            <span className="row-value">nexus.db (usage_records table)</span>
          </div>
          <div className="analytics-row">
            <span className="row-label">Telemetry Storage</span>
            <span className="row-value">Local Only · 100% Private</span>
          </div>
        </div>
      </div>
    </div>
  );
}
