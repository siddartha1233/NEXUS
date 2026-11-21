import React from "react";
import { CodeBlock } from "./CodeBlock";
import { parseMessageContent } from "../utils/parseMessage";

export { parseMessageContent };

interface MessageRendererProps {
  content: string;
}

function renderInlineMarkdown(text: string): React.ReactNode[] {
  // Tokenize inline code (`...`) and bold (**...**) safely without dangerouslySetInnerHTML
  const tokens = text.split(/(`[^`]+`|\*\*[^*]+\*\*)/g);
  return tokens.map((tok, i) => {
    if (tok.startsWith("`") && tok.endsWith("`") && tok.length > 2) {
      return (
        <code key={i} className="nexus-md-inline-code">
          {tok.slice(1, -1)}
        </code>
      );
    }
    if (tok.startsWith("**") && tok.endsWith("**") && tok.length > 4) {
      return <strong key={i}>{tok.slice(2, -2)}</strong>;
    }
    return tok;
  });
}

function parseTableCells(rowLine: string): string[] {
  const trimmed = rowLine.trim();
  const inner = trimmed.replace(/^\|/, "").replace(/\|$/, "");
  return inner.split("|").map((c) => c.trim());
}

function isTableDividerRow(rowLine: string): boolean {
  const cells = parseTableCells(rowLine);
  return cells.length > 0 && cells.every((c) => /^:?-{2,}:?$/.test(c));
}

function isTableStartAt(lines: string[], index: number): boolean {
  if (index + 1 >= lines.length) return false;
  const header = lines[index].trim();
  const divider = lines[index + 1].trim();
  return (
    header.startsWith("|") &&
    header.endsWith("|") &&
    divider.startsWith("|") &&
    isTableDividerRow(divider)
  );
}

function renderProseSegment(text: string, baseKey: string): React.ReactNode {
  const lines = text.split("\n");
  const elements: React.ReactNode[] = [];
  let i = 0;

  while (i < lines.length) {
    const line = lines[i];
    const trimmed = line.trim();

    // 1. Check for Markdown pipe table
    if (isTableStartAt(lines, i)) {
      const headers = parseTableCells(trimmed);
      i += 2; // skip header and divider
      const rows: string[][] = [];
      while (i < lines.length && lines[i].trim().startsWith("|")) {
        if (!isTableDividerRow(lines[i])) {
          rows.push(parseTableCells(lines[i]));
        }
        i++;
      }
      elements.push(
        <div key={`${baseKey}-tbl-${i}`} className="nexus-table-wrap">
          <table className="nexus-md-table">
            <thead>
              <tr>
                {headers.map((h, hIdx) => (
                  <th key={hIdx}>{renderInlineMarkdown(h)}</th>
                ))}
              </tr>
            </thead>
            <tbody>
              {rows.map((r, rIdx) => (
                <tr key={rIdx}>
                  {r.map((cell, cIdx) => (
                    <td key={cIdx}>{renderInlineMarkdown(cell)}</td>
                  ))}
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      );
      continue;
    }

    // 2. Check for Markdown heading (#, ##, ###, ####)
    const headingMatch = /^(#{1,4})\s+(.+)$/.exec(trimmed);
    if (headingMatch) {
      const level = headingMatch[1].length;
      const headingText = headingMatch[2];
      const fontSize = level === 1 ? "18px" : level === 2 ? "16px" : "14.5px";
      elements.push(
        <div
          key={`${baseKey}-h-${i}`}
          className="nexus-md-heading"
          style={{ fontSize }}
        >
          {renderInlineMarkdown(headingText)}
        </div>
      );
      i++;
      continue;
    }

    // 3. Check for unordered bullet list (- or *)
    if (/^[-*]\s+/.test(trimmed)) {
      const items: string[] = [];
      while (i < lines.length && /^[-*]\s+/.test(lines[i].trim())) {
        items.push(lines[i].trim().replace(/^[-*]\s+/, ""));
        i++;
      }
      elements.push(
        <ul key={`${baseKey}-ul-${i}`} className="nexus-md-list">
          {items.map((item, idx) => (
            <li key={idx}>{renderInlineMarkdown(item)}</li>
          ))}
        </ul>
      );
      continue;
    }

    // 4. Accumulate consecutive regular paragraph lines
    if (trimmed === "") {
      i++;
      continue;
    }

    const paraLines: string[] = [];
    while (
      i < lines.length &&
      lines[i].trim() !== "" &&
      !/^(#{1,4})\s+/.test(lines[i].trim()) &&
      !/^[-*]\s+/.test(lines[i].trim()) &&
      !isTableStartAt(lines, i)
    ) {
      paraLines.push(lines[i]);
      i++;
    }

    // Defensive progress guarantee so partial streaming syntax can never stall the parser loop
    if (paraLines.length === 0 && i < lines.length) {
      paraLines.push(lines[i]);
      i++;
    }

    elements.push(
      <p
        key={`${baseKey}-p-${i}`}
        className="nexus-prose-paragraph"
        style={{ whiteSpace: "pre-wrap" }}
      >
        {renderInlineMarkdown(paraLines.join("\n"))}
      </p>
    );
  }

  return elements;
}

export const MessageRenderer = React.memo<MessageRendererProps>(({ content }) => {
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
          <React.Fragment key={partKey}>
            {renderProseSegment(part.text, partKey)}
          </React.Fragment>
        );
      })}
    </div>
  );
});
MessageRenderer.displayName = "MessageRenderer";
