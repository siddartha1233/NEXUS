export interface ContentPart {
  type: "text" | "code";
  text: string;
  language?: string;
}

export function parseMessageContent(content: string): ContentPart[] {
  if (!content) return [];

  const parts: ContentPart[] = [];
  const codeBlockRegex = /```(\w*)\r?\n([\s\S]*?)(?:```|$)/g;

  let lastIndex = 0;
  let match: RegExpExecArray | null;

  while ((match = codeBlockRegex.exec(content)) !== null) {
    const textBefore = content.substring(lastIndex, match.index);
    if (textBefore) {
      parts.push({
        type: "text",
        text: textBefore,
      });
    }

    const language = match[1]?.trim() || "";
    const code = match[2] || "";

    parts.push({
      type: "code",
      language,
      text: code.trimEnd(),
    });

    lastIndex = match.index + match[0].length;
  }

  const remaining = content.substring(lastIndex);
  if (remaining) {
    parts.push({
      type: "text",
      text: remaining,
    });
  }

  return parts;
}
