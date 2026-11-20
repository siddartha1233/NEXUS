import { invoke } from "@tauri-apps/api/core";

export interface ExecutionResult {
  stdout: string;
  stderr: string;
  exitCode: number | null;
  executionTimeMs: number;
  success: boolean;
}

export const EXECUTABLE_LANGUAGES = new Set([
  "python",
  "py",
  "python3",
  "javascript",
  "js",
  "node",
  "sql",
  "sqlite",
  "sqlite3",
  "powershell",
  "ps1",
  "pwsh",
  "bash",
  "sh",
  "shell",
  "cmd",
  "batch",
]);

export function isExecutableLanguage(lang: string): boolean {
  if (!lang) return false;
  return EXECUTABLE_LANGUAGES.has(lang.toLowerCase().trim());
}

export async function executeCodeSnippet(
  language: string,
  code: string
): Promise<ExecutionResult> {
  try {
    return await invoke<ExecutionResult>("execute_code_snippet", {
      language,
      code,
    });
  } catch (error) {
    throw new Error(
      typeof error === "string" ? error : "Code execution failed."
    );
  }
}
