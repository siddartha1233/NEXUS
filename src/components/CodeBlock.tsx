import { useState } from "react";
import {
  executeCodeSnippet,
  isExecutableLanguage,
  type ExecutionResult,
} from "../services/codeRunner";

interface CodeBlockProps {
  code: string;
  language?: string;
}

export function CodeBlock({ code, language = "" }: CodeBlockProps) {
  const [copied, setCopied] = useState(false);
  const [isRunning, setIsRunning] = useState(false);
  const [result, setResult] = useState<ExecutionResult | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [showOutput, setShowOutput] = useState(false);

  const cleanLang = language.trim().toLowerCase();
  const canRun = isExecutableLanguage(cleanLang);

  const handleCopy = async () => {
    try {
      await navigator.clipboard.writeText(code);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    } catch (e) {
      console.error("Failed to copy code snippet:", e);
    }
  };

  const isSqlSandbox =
    cleanLang === "sql" || cleanLang === "sqlite" || cleanLang === "sqlite3";

  const handleRun = async () => {
    if (isRunning || !canRun) return;

    if (!isSqlSandbox) {
      const confirmed = window.confirm(
        `Run this ${displayLang} snippet locally on your machine? Local scripts execute with your user account privileges (not inside an OS container). Make sure you have reviewed the code.`
      );
      if (!confirmed) return;
    }

    setIsRunning(true);
    setError(null);
    setShowOutput(true);

    try {
      const res = await executeCodeSnippet(cleanLang, code);
      setResult(res);
    } catch (err) {
      setError(err instanceof Error ? err.message : "Execution failed.");
      setResult(null);
    } finally {
      setIsRunning(false);
    }
  };

  const displayLang = cleanLang ? cleanLang.toUpperCase() : "CODE";

  return (
    <div className="nexus-code-card">
      <div className="nexus-code-header">
        <span className="nexus-code-lang">{displayLang}</span>

        <div className="nexus-code-actions">
          <button
            type="button"
            className="nexus-code-action-btn"
            onClick={() => void handleCopy()}
            title="Copy code to clipboard"
          >
            {copied ? "✓ Copied" : "Copy"}
          </button>

          {canRun && (
            <button
              type="button"
              className={`nexus-code-run-btn ${isRunning ? "running" : ""}`}
              onClick={() => void handleRun()}
              disabled={isRunning}
              title={
                isSqlSandbox
                  ? `Execute ${displayLang} in SQLite sandbox`
                  : `Execute ${displayLang} locally on your machine`
              }
            >
              {isRunning ? (
                <>
                  <span className="spinner-dot" /> Running...
                </>
              ) : (
                "▶ Run Code"
              )}
            </button>
          )}
        </div>
      </div>

      <pre className="nexus-code-content">
        <code>{code}</code>
      </pre>

      {/* TERMINAL OUTPUT DRAWER */}
      {showOutput && (result || error || isRunning) && (
        <div className="nexus-terminal-drawer">
          <div className="nexus-terminal-header">
            <div className="nexus-terminal-title">
              <span className="terminal-dot" />
              <span>{isSqlSandbox ? "SQL Sandbox Console" : "Local Host Console"}</span>
              {result && (
                <span
                  className={`terminal-status-badge ${
                    result.success ? "success" : "failure"
                  }`}
                >
                  {result.success
                    ? `Exit ${result.exitCode ?? 0}`
                    : `Error (${result.exitCode ?? 1})`}
                  {` · ${result.executionTimeMs}ms`}
                </span>
              )}
            </div>

            <button
              type="button"
              className="nexus-terminal-close-btn"
              onClick={() => setShowOutput(false)}
              title="Close terminal"
            >
              ×
            </button>
          </div>

          <div className="nexus-terminal-body">
            {isRunning && (
              <div className="nexus-terminal-loading">
                {isSqlSandbox
                  ? "Executing SQL in isolated SQLite sandbox..."
                  : "Executing snippet locally..."}
              </div>
            )}

            {error && (
              <div className="nexus-terminal-stderr">{error}</div>
            )}

            {result && (
              <>
                {result.stdout && (
                  <pre className="nexus-terminal-stdout">{result.stdout}</pre>
                )}
                {result.stderr && (
                  <pre className="nexus-terminal-stderr">{result.stderr}</pre>
                )}
                {!result.stdout && !result.stderr && (
                  <div className="nexus-terminal-empty">
                    (Process executed successfully with no console output)
                  </div>
                )}
              </>
            )}
          </div>
        </div>
      )}
    </div>
  );
}
