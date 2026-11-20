import { invoke } from "@tauri-apps/api/core";

export interface DailyUsageSummary {
  date: string;
  prompts: number;
  promptTokens: number;
  completionTokens: number;
  totalTokens: number;
}

export interface TelemetryStats {
  todayTokens: number;
  todayPrompts: number;
  todayPromptTokens: number;
  todayCompletionTokens: number;
  allTimeTokens: number;
  allTimePrompts: number;
  allTimePromptTokens: number;
  allTimeCompletionTokens: number;
  dailyHistory: DailyUsageSummary[];
}

export function getLocalDateString(): string {
  const now = new Date();
  const year = now.getFullYear();
  const month = String(now.getMonth() + 1).padStart(2, "0");
  const day = String(now.getDate()).padStart(2, "0");
  return `${year}-${month}-${day}`;
}

export async function recordTelemetry(
  model: string,
  promptTokens: number,
  completionTokens: number,
  durationMs: number,
  tokensPerSec?: number
): Promise<void> {
  try {
    await invoke("db_record_telemetry", {
      model,
      promptTokens,
      completionTokens,
      durationMs,
      tokensPerSec: tokensPerSec ?? null,
      localDate: getLocalDateString(),
    });
  } catch (err) {
    console.error("Failed to record telemetry:", err);
  }
}

export async function getTelemetryStats(): Promise<TelemetryStats> {
  try {
    const raw: any = await invoke("db_get_telemetry_stats", {
      localDate: getLocalDateString(),
    });

    if (!raw) {
      return {
        todayTokens: 0,
        todayPrompts: 0,
        todayPromptTokens: 0,
        todayCompletionTokens: 0,
        allTimeTokens: 0,
        allTimePrompts: 0,
        allTimePromptTokens: 0,
        allTimeCompletionTokens: 0,
        dailyHistory: [],
      };
    }

    const rawHistory = raw.dailyHistory ?? raw.daily_history;
    const dailyHistory: DailyUsageSummary[] = Array.isArray(rawHistory)
      ? rawHistory.map((d: any) => ({
          date: String(d.date ?? ""),
          prompts: Number(d.prompts ?? 0),
          promptTokens: Number(d.promptTokens ?? d.prompt_tokens ?? 0),
          completionTokens: Number(d.completionTokens ?? d.completion_tokens ?? 0),
          totalTokens: Number(d.totalTokens ?? d.total_tokens ?? 0),
        }))
      : [];

    return {
      todayTokens: Number(raw.todayTokens ?? raw.today_tokens ?? 0),
      todayPrompts: Number(raw.todayPrompts ?? raw.today_prompts ?? 0),
      todayPromptTokens: Number(raw.todayPromptTokens ?? raw.today_prompt_tokens ?? 0),
      todayCompletionTokens: Number(raw.todayCompletionTokens ?? raw.today_completion_tokens ?? 0),
      allTimeTokens: Number(raw.allTimeTokens ?? raw.all_time_tokens ?? 0),
      allTimePrompts: Number(raw.allTimePrompts ?? raw.all_time_prompts ?? 0),
      allTimePromptTokens: Number(raw.allTimePromptTokens ?? raw.all_time_prompt_tokens ?? 0),
      allTimeCompletionTokens: Number(raw.allTimeCompletionTokens ?? raw.all_time_completion_tokens ?? 0),
      dailyHistory,
    };
  } catch (err) {
    console.error("Failed to get telemetry stats:", err);
    return {
      todayTokens: 0,
      todayPrompts: 0,
      todayPromptTokens: 0,
      todayCompletionTokens: 0,
      allTimeTokens: 0,
      allTimePrompts: 0,
      allTimePromptTokens: 0,
      allTimeCompletionTokens: 0,
      dailyHistory: [],
    };
  }
}
