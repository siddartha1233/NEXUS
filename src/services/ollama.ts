import { Channel, invoke } from "@tauri-apps/api/core";

export interface OllamaModel {
  name: string;
  size: number;
  modified_at: string;
}

export interface RunningOllamaModel {
  name: string;
  size: number;
  size_vram: number;
}

export interface ChatMessage {
  role: "system" | "user" | "assistant";
  content: string;
}

export function isEmbeddingModel(name: string): boolean {
  return name.toLowerCase().includes("embed");
}

export interface GenerationMetrics {
  totalDurationMs?: number;
  loadDurationMs?: number;
  promptEvalCount?: number;
  evalCount?: number;
  tokensPerSec?: number;
}

type StreamEvent =
  | {
      event: "token";
      data: {
        content: string;
      };
    }
  | {
      event: "done";
      data: {
        totalDuration?: number;
        loadDuration?: number;
        promptEvalCount?: number;
        evalCount?: number;
        evalDuration?: number;
        total_duration?: number;
        load_duration?: number;
        prompt_eval_count?: number;
        eval_count?: number;
        eval_duration?: number;
      };
    };

export async function getOllamaModels(): Promise<OllamaModel[]> {
  try {
    return await invoke<OllamaModel[]>("ollama_get_models");
  } catch (error) {
    throw new Error(
      typeof error === "string" ? error : "Unable to connect to Ollama."
    );
  }
}

export async function getRunningOllamaModels(): Promise<RunningOllamaModel[]> {
  try {
    return await invoke<RunningOllamaModel[]>("ollama_get_running_models");
  } catch (error) {
    throw new Error(
      typeof error === "string"
        ? error
        : "Unable to query running Ollama models."
    );
  }
}

export async function unloadOllamaModel(model: string): Promise<void> {
  try {
    await invoke<void>("ollama_unload_model", { model });
  } catch (error) {
    throw new Error(
      typeof error === "string" ? error : `Unable to unload ${model}.`
    );
  }
}

export async function checkOllamaConnection(): Promise<boolean> {
  try {
    await getOllamaModels();
    return true;
  } catch {
    return false;
  }
}

export async function streamOllamaChat(
  model: string,
  messages: ChatMessage[],
  onToken: (token: string) => void,
  signal?: AbortSignal,
  onDone?: (metrics: GenerationMetrics) => void
): Promise<void> {
  if (signal?.aborted) {
    throw new DOMException("Generation aborted", "AbortError");
  }

  const onEvent = new Channel<StreamEvent>();
  onEvent.onmessage = (message) => {
    if (message.event === "token" && message.data?.content) {
      onToken(message.data.content);
    } else if (message.event === "done" && onDone) {
      const d = message.data;
      const evalCount = d.evalCount ?? d.eval_count ?? 0;
      const evalDuration = d.evalDuration ?? d.eval_duration ?? 0;
      const evalDurationSec = evalDuration / 1_000_000_000;
      const totalDuration = d.totalDuration ?? d.total_duration;
      const loadDuration = d.loadDuration ?? d.load_duration;
      const promptEvalCount = d.promptEvalCount ?? d.prompt_eval_count;

      const tokensPerSec =
        evalDurationSec > 0 && evalCount > 0
          ? Number((evalCount / evalDurationSec).toFixed(1))
          : undefined;

      onDone({
        totalDurationMs: totalDuration ? Math.round(totalDuration / 1_000_000) : undefined,
        loadDurationMs: loadDuration ? Math.round(loadDuration / 1_000_000) : undefined,
        promptEvalCount,
        evalCount,
        tokensPerSec,
      });
    }
  };

  const handleAbort = () => {
    void invoke("ollama_stop_generation").catch(() => {});
  };

  signal?.addEventListener("abort", handleAbort, { once: true });

  try {
    await invoke<void>("ollama_chat_stream", {
      model,
      messages,
      onEvent,
    });
  } catch (error) {
    if (error === "ABORTED" || signal?.aborted) {
      throw new DOMException("Generation aborted", "AbortError");
    }

    throw new Error(
      typeof error === "string"
        ? error
        : error instanceof Error
          ? error.message
          : "Ollama request failed."
    );
  } finally {
    signal?.removeEventListener("abort", handleAbort);
  }
}
