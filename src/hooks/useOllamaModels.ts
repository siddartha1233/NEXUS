import { useCallback, useState } from "react";
import {
  getOllamaModels,
  getRunningOllamaModels,
  isEmbeddingModel,
  unloadOllamaModel,
  type OllamaModel,
  type RunningOllamaModel,
} from "../services/ollama";

export function useOllamaModels(isGenerating: boolean) {
  const [selectedModel, setSelectedModel] = useState("qwen2.5-coder:7b");
  const [allModels, setAllModels] = useState<OllamaModel[]>([]);
  const [runningModels, setRunningModels] = useState<RunningOllamaModel[]>([]);
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

      const nextChatModels = models.filter((m) => !isEmbeddingModel(m.name));
      const list = nextChatModels.length > 0 ? nextChatModels : models;

      setSelectedModel((current) => {
        if (list.length > 0 && !list.some((m) => m.name === current)) {
          return list[0].name;
        }
        return current;
      });
    } catch (error) {
      setConnectionStatus("disconnected");
      setRunningModels([]);
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

  return {
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
  };
}
