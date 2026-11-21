import { invoke } from "@tauri-apps/api/core";

export interface CloudModelInfo {
  id: string;
  name: string;
  rawId: string;
  provider: "groq";
  providerLabel: string;
  description: string;
  speed: string;
  contextWindow: string;
}

export interface GroqApiRawModel {
  id: string;
  object?: string;
  created?: number;
  owned_by?: string;
  active?: boolean;
}

export function isCloudModel(modelId: string): boolean {
  return modelId.startsWith("groq:");
}

function formatModelDisplayName(rawId: string): string {
  const lower = rawId.toLowerCase();
  if (lower.includes("gpt-oss-120b")) {
    return "GPT-OSS 120B (Flagship Reasoning)";
  }
  if (lower.includes("gpt-oss-20b")) {
    return "GPT-OSS 20B (Fast)";
  }
  if (lower.includes("llama-3.3-70b")) {
    return "Llama 3.3 70B (Flagship)";
  }
  if (lower.includes("qwen3.8-27b")) {
    return "Qwen 3.8 27B";
  }
  if (lower.includes("deepseek-r1")) {
    return "DeepSeek R1 70B (Math & Logic)";
  }

  return rawId
    .replace(/^openai\//i, "")
    .replace(/^meta-llama\//i, "")
    .replace(/^qwen\//i, "Qwen ")
    .split(/[-_]/)
    .map((word) => {
      const w = word.toLowerCase();
      if (w === "gpt") return "GPT";
      if (w === "oss") return "OSS";
      if (w === "r1") return "R1";
      if (w === "llama") return "Llama";
      return word.charAt(0).toUpperCase() + word.slice(1);
    })
    .join(" ");
}

/**
 * Dynamically queries the Groq API using the user's stored key.
 * Filtering and flagship priority sorting are handled authoritatively by Rust (`fetch_active_groq_models`).
 */
export async function fetchLiveGroqModels(): Promise<CloudModelInfo[]> {
  try {
    const rawList = await invoke<GroqApiRawModel[]>("groq_get_active_models");
    if (!rawList || rawList.length === 0) {
      return [];
    }

    return rawList.map((m) => {
      const displayName = formatModelDisplayName(m.id);
      return {
        id: `groq:${m.id}`,
        name: displayName,
        rawId: m.id,
        provider: "groq",
        providerLabel: "Groq (Free)",
        description: `Flagship model on Groq Cloud (${m.id})`,
        speed: m.id.includes("20b") ? "~450+ tok/s" : "~280+ tok/s",
        contextWindow: "128k",
      };
    });
  } catch (error) {
    // If no key configured or network down, return empty array (no hardcoding)
    return [];
  }
}
