import { invoke } from "@tauri-apps/api/core";

export interface Workspace {
  id: string;
  name: string;
  description: string;
  created_at: number;
  updated_at: number;
}

export interface WorkspaceNote {
  id: string;
  workspace_id: string;
  title: string;
  content: string;
  created_at: number;
  updated_at: number;
}

export interface WorkspaceMaterial {
  id: string;
  workspace_id: string;
  category: "lectures" | "assignments" | "other";
  title: string;
  filename: string;
  content_preview: string;
  created_at: number;
}

export async function listWorkspaces(): Promise<Workspace[]> {
  return invoke<Workspace[]>("db_list_workspaces");
}

export async function createWorkspace(
  name: string,
  description = ""
): Promise<Workspace> {
  return invoke<Workspace>("db_create_workspace", { name, description });
}

export async function renameWorkspace(
  id: string,
  name: string,
  description?: string
): Promise<void> {
  return invoke<void>("db_rename_workspace", { id, name, description });
}

export async function deleteWorkspace(id: string): Promise<void> {
  return invoke<void>("db_delete_workspace", { id });
}

export async function listWorkspaceNotes(
  workspaceId: string
): Promise<WorkspaceNote[]> {
  return invoke<WorkspaceNote[]>("db_list_workspace_notes", { workspaceId });
}

export async function saveWorkspaceNote(
  workspaceId: string,
  title: string,
  content: string,
  id?: string
): Promise<WorkspaceNote> {
  return invoke<WorkspaceNote>("db_save_workspace_note", {
    workspaceId,
    title,
    content,
    id: id ?? null,
  });
}

export async function deleteWorkspaceNote(
  workspaceId: string,
  noteId: string
): Promise<void> {
  return invoke<void>("db_delete_workspace_note", { workspaceId, noteId });
}

export async function listWorkspaceMaterials(
  workspaceId: string
): Promise<WorkspaceMaterial[]> {
  return invoke<WorkspaceMaterial[]>("db_list_workspace_materials", {
    workspaceId,
  });
}

export async function addWorkspaceMaterial(
  workspaceId: string,
  category: string,
  title: string,
  filename: string,
  contentPreview = ""
): Promise<WorkspaceMaterial> {
  return invoke<WorkspaceMaterial>("db_add_workspace_material", {
    workspaceId,
    category,
    title,
    filename,
    contentPreview,
  });
}

export async function importWorkspaceFile(
  workspaceId: string,
  category: string,
  sourcePath: string
): Promise<WorkspaceMaterial> {
  return invoke<WorkspaceMaterial>("db_import_workspace_file", {
    workspaceId,
    category,
    sourcePath,
  });
}

export interface WorkspaceContextResult {
  available: boolean;
  context: string;
  sources: string[];
}

export async function getWorkspaceContext(
  workspaceId: string,
  query: string
): Promise<WorkspaceContextResult> {
  try {
    return await invoke<WorkspaceContextResult>("db_get_workspace_context", {
      workspaceId,
      query,
    });
  } catch (err) {
    console.error("Failed to fetch workspace context:", err);
    return {
      available: false,
      context: "",
      sources: [],
    };
  }
}

export async function deleteWorkspaceMaterial(
  workspaceId: string,
  materialId: string
): Promise<void> {
  return invoke<void>("db_delete_workspace_material", {
    workspaceId,
    materialId,
  });
}
