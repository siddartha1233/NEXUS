import { useCallback, useState } from "react";
import {
  getOllamaModels,
  getRunningOllamaModels,
  isEmbeddingModel,
  unloadOllamaModel,
  type OllamaModel,
  type RunningOllamaModel,
} from "../services/ollama";
import {
  fetchLiveGroqModels,
  isCloudModel,
  type CloudModelInfo,
} from "../services/cloudModels";

export function useOllamaModels(isGenerating: boolean) {
  const [selectedModel, setSelectedModel] = useState("qwen2.5-coder:7b");
  const [allModels, setAllModels] = useState<OllamaModel[]>([]);
  const [runningModels, setRunningModels] = useState<RunningOllamaModel[]>([]);
  const [cloudModels, setCloudModels] = useState<CloudModelInfo[]>([]);
  const [connectionStatus, setConnectionStatus] = useState<
    "checking" | "connected" | "disconnected"
  >("checking");
  const [isRefreshingModels, setIsRefreshingModels] = useState(false);
  const [busyModel, setBusyModel] = useState<string | null>(null);
  const [modelManagerError, setModelManagerError] = useState("");

  const chatModels = allModels.filter((m) => !isEmbeddingModel(m.name));
  const availableModels = chatModels.length > 0 ? chatModels : allModels;

  const refreshOllamaState = useCallback(async () => {
    setIsRefreshingModels(true);
    setModelManagerError("");

    try {
      const [models, running] = await Promise.all([
        getOllamaModels(),
        getRunningOllamaModels().catch(() => [] as RunningOllamaModel[]),
      ]);

      setAllModels(models);
      setRunningModels(running);
      setConnectionStatus("connected");

      // Also refresh live Groq models in the background
      void fetchLiveGroqModels()
        .then((live) => {
          setCloudModels(live);
          setSelectedModel((current) => {
            if (isCloudModel(current)) {
              if (!live.some((m) => m.id === current) && live.length > 0) {
                return live[0].id;
              }
            }
            return current;
          });
        })
        .catch(() => {});

      const nextChatModels = models.filter((m) => !isEmbeddingModel(m.name));
      const list = nextChatModels.length > 0 ? nextChatModels : models;

      setSelectedModel((current) => {
        // If current is a cloud model, keep it intact
        if (isCloudModel(current)) {
          return current;
        }
        if (list.length > 0 && !list.some((m) => m.name === current)) {
          return list[0].name;
        }
        return current;
      });
    } catch (error) {
      setConnectionStatus("disconnected");
      setRunningModels([]);
      // When Ollama is offline, still attempt to fetch cloud models so user can chat with cloud!
      void fetchLiveGroqModels()
        .then((live) => {
          setCloudModels(live);
          setSelectedModel((current) => {
            if (isCloudModel(current)) {
              if (!live.some((m) => m.id === current) && live.length > 0) {
                return live[0].id;
              }
            }
            return current;
          });
        })
        .catch(() => {});
      setModelManagerError(
        error instanceof Error ? error.message : "Unable to connect to Ollama."
      );
    } finally {
      setIsRefreshingModels(false);
    }
  }, []);

  const handleSelectModel = useCallback(
    async (newModel: string) => {
      if (isGenerating || newModel === selectedModel) return;

      setBusyModel(newModel);
      setModelManagerError("");

      try {
        // If switching to a cloud model or another model, unload running local chat models
        // to free up local GPU/VRAM
        const loadedChatModels = runningModels.filter(
          (m) => !isEmbeddingModel(m.name) && m.name !== newModel
        );

        for (const loaded of loadedChatModels) {
          await unloadOllamaModel(loaded.name);
        }

        setSelectedModel(newModel);

        if (loadedChatModels.length > 0) {
          const running = await getRunningOllamaModels().catch(
            () => [] as RunningOllamaModel[]
          );
          setRunningModels(running);
        }
      } catch (error) {
        setSelectedModel(newModel);
        setModelManagerError(
          error instanceof Error
            ? error.message
            : "Failed to unload previous model."
        );
      } finally {
        setBusyModel(null);
      }
    },
    [isGenerating, runningModels, selectedModel]
  );

  const handleUnloadModel = useCallback(
    async (modelName: string) => {
      if (isGenerating && !isEmbeddingModel(modelName)) return;

      setBusyModel(modelName);
      setModelManagerError("");

      try {
        await unloadOllamaModel(modelName);
        const running = await getRunningOllamaModels().catch(
          () => [] as RunningOllamaModel[]
        );
        setRunningModels(running);
      } catch (error) {
        setModelManagerError(
          error instanceof Error
            ? error.message
            : `Failed to unload ${modelName}.`
        );
      } finally {
        setBusyModel(null);
      }
    },
    [isGenerating]
  );

  // Fallback to the first available local model (used during cloud rate limits or offline events)
  const autoFallbackToLocal = useCallback((): string => {
    const fallback =
      availableModels[0]?.name ||
      allModels[0]?.name ||
      "qwen2.5-coder:7b";
    setSelectedModel(fallback);
    return fallback;
  }, [allModels, availableModels]);

  return {
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
  };
}
