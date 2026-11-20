import { useCallback, useState } from "react";
import {
  createWorkspace,
  deleteWorkspace,
  listWorkspaces,
  type Workspace,
} from "../services/workspaces";

export function useWorkspaces(onActiveWorkspaceDeleted?: () => void) {
  const [workspaces, setWorkspaces] = useState<Workspace[]>([]);
  const [activeWorkspace, setActiveWorkspace] = useState<Workspace | null>(null);

  const refreshWorkspaces = useCallback(async () => {
    try {
      const list = await listWorkspaces();
      setWorkspaces(list);
    } catch (error) {
      console.error("Failed to load workspaces:", error);
    }
  }, []);

  const handleCreateWorkspace = useCallback(
    async (name: string, description: string) => {
      try {
        const created = await createWorkspace(name, description);
        setWorkspaces((prev) => [created, ...prev]);
        setActiveWorkspace(created);
      } catch (err) {
        console.error("Failed to create workspace:", err);
      }
    },
    []
  );

  const handleDeleteWorkspace = useCallback(
    async (id: string) => {
      try {
        await deleteWorkspace(id);
        setWorkspaces((prev) => prev.filter((w) => w.id !== id));
        if (activeWorkspace?.id === id) {
          setActiveWorkspace(null);
          onActiveWorkspaceDeleted?.();
        }
      } catch (err) {
        console.error("Failed to delete workspace:", err);
      }
    },
    [activeWorkspace?.id, onActiveWorkspaceDeleted]
  );

  return {
    workspaces,
    activeWorkspace,
    setActiveWorkspace,
    refreshWorkspaces,
    handleCreateWorkspace,
    handleDeleteWorkspace,
  };
}
