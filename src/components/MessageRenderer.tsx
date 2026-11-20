import React from "react";
import { CodeBlock } from "./CodeBlock";
import { parseMessageContent } from "../utils/parseMessage";

export { parseMessageContent };

interface MessageRendererProps {
  content: string;
}

export const MessageRenderer: React.FC<MessageRendererProps> = ({ content }) => {
  const parts = parseMessageContent(content);

  return (
    <div className="nexus-message-rendered">
      {parts.map((part, idx) => {
        const partKey = `${part.type}-${part.language ?? ""}-${idx}-${part.text.slice(0, 24)}`;
        if (part.type === "code") {
          return (
            <CodeBlock
              key={partKey}
              language={part.language}
              code={part.text}
            />
          );
        }

        return (
          <p
            key={partKey}
            className="nexus-prose-paragraph"
            style={{ whiteSpace: "pre-wrap" }}
          >
            {part.text}
          </p>
        );
      })}
    </div>
  );
};
