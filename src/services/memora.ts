import { invoke } from "@tauri-apps/api/core";

export interface MemoraContextResult {
  available: boolean;
  context: string;
  sources: string[];
  error?: string | null;
}

export interface MemoraSourceItem {
  path: string;
  status: string;
  size: number;
  chunk_count: number;
  indexed_at: number | null;
}

export async function checkMemoraStatus(): Promise<boolean> {
  try {
    return await invoke<boolean>("memora_check_status");
  } catch {
    return false;
  }
}

export async function listMemoraSources(): Promise<MemoraSourceItem[]> {
  try {
    return await invoke<MemoraSourceItem[]>("memora_list_sources");
  } catch {
    return [];
  }
}

export async function getMemoraContext(
  topic: string
): Promise<MemoraContextResult> {
  try {
    return await invoke<MemoraContextResult>("memora_get_context", { topic });
  } catch (error) {
    return {
      available: false,
      context: "",
      sources: [],
      error: error instanceof Error ? error.message : "Failed to query MEMORA",
    };
  }
}

export async function memorizeToMemora(
  content: string,
  source?: string
): Promise<boolean> {
  try {
    return await invoke<boolean>("memora_memorize", {
      content,
      source: source ?? null,
    });
  } catch {
    return false;
  }
}

export async function triggerMemoraIngest(): Promise<string> {
  return invoke<string>("memora_ingest");
}
