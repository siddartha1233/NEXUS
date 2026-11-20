// @ts-expect-error node built-in without @types/node
import assert from "node:assert/strict";
// @ts-expect-error node built-in without @types/node
import { describe, it } from "node:test";
import { parseMessageContent } from "./parseMessage.ts";

describe("parseMessageContent", () => {
  it("returns empty array for empty input", () => {
    assert.deepEqual(parseMessageContent(""), []);
  });

  it("parses plain text without code blocks", () => {
    const parts = parseMessageContent("Hello from Nexus!");
    assert.equal(parts.length, 1);
    assert.equal(parts[0].type, "text");
    assert.equal(parts[0].text, "Hello from Nexus!");
  });

  it("extracts fenced code blocks with language tags", () => {
    const input = "Intro\n```python\nprint('hi')\n```\nOutro";
    const parts = parseMessageContent(input);
    assert.equal(parts.length, 3);
    assert.equal(parts[0].type, "text");
    assert.equal(parts[1].type, "code");
    assert.equal(parts[1].language, "python");
    assert.equal(parts[1].text, "print('hi')");
    assert.equal(parts[2].type, "text");
    assert.equal(parts[2].text, "\nOutro");
  });

  it("handles unclosed streaming code blocks safely", () => {
    const input = "Streaming:\n```sql\nSELECT * FROM students;";
    const parts = parseMessageContent(input);
    assert.equal(parts.length, 2);
    assert.equal(parts[1].type, "code");
    assert.equal(parts[1].language, "sql");
    assert.equal(parts[1].text, "SELECT * FROM students;");
  });
});
