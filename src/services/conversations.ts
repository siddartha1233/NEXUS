import { invoke } from "@tauri-apps/api/core";

export interface Conversation {
  id: string;
  title: string;
  model: string;
  workspace_id?: string | null;
  created_at: number;
  updated_at: number;
}

export interface StoredMessage {
  id: number;
  conversation_id: string;
  role: "system" | "user" | "assistant";
  content: string;
  model: string | null;
  created_at: number;
}

export async function listConversations(
  workspaceId?: string | null
): Promise<Conversation[]> {
  return invoke<Conversation[]>("db_list_conversations", {
    workspaceId: workspaceId ?? null,
  });
}

export async function createConversation(
  title: string,
  model: string,
  workspaceId?: string | null
): Promise<Conversation> {
  return invoke<Conversation>("db_create_conversation", {
    title,
    model,
    workspaceId: workspaceId ?? null,
  });
}

export async function renameConversation(
  id: string,
  title: string
): Promise<void> {
  return invoke<void>("db_rename_conversation", { id, title });
}

export async function deleteConversation(id: string): Promise<void> {
  return invoke<void>("db_delete_conversation", { id });
}

export async function getConversationMessages(
  conversationId: string,
  options?: { beforeId?: number; limit?: number }
): Promise<StoredMessage[]> {
  return invoke<StoredMessage[]>("db_get_messages", {
    conversationId,
    beforeId: options?.beforeId ?? null,
    limit: options?.limit ?? null,
  });
}

export async function addConversationMessage(
  conversationId: string,
  role: "system" | "user" | "assistant",
  content: string,
  model?: string
): Promise<StoredMessage> {
  return invoke<StoredMessage>("db_add_message", {
    conversationId,
    role,
    content,
    model: model ?? null,
  });
}
