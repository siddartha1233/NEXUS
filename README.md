# Nexus

**A 100% local, privacy-first AI desktop platform built with Tauri 2, Rust, React, and Ollama.**

Nexus brings local AI chat, isolated study/project workspaces, PDF document retrieval (RAG), interactive code execution, real-time token & VRAM analytics, and optional Obsidian vault memory into one fast, native desktop app — with zero cloud dependencies and zero API fees.

---

## What Is Nexus?

Most AI tools scatter your chats, PDFs, notes, and code across cloud apps that send your data to third-party servers. **Nexus** runs completely on your own machine:

- **Local Streaming Chat & Model Manager:** Chat with any locally installed Ollama model (e.g., `qwen2.5-coder:7b`, `qwen3:4b`) with real-time token streaming, instant generation stop (`■`), and automatic model unloading to protect your GPU/RAM.
- **Isolated Workspaces & Local PDF RAG:** Create separate workspaces for courses, projects, or research. Upload PDFs, Markdown, or text files directly into a workspace — Nexus extracts text locally in Rust, indexes it with **SQLite FTS5 (BM25)**, and grounds answers with filename citations. Data never leaks across workspaces.
- **Interactive Code Runner & SQL Sandbox:** Run AI-generated **SQL** queries directly inside a pre-seeded, locked-down SQLite sandbox (`students`, `courses`, `enrollments`, `employees`) with formatted table output, or run **Python** and **JavaScript** snippets locally on explicit click with confirmation and a 10-second watchdog timeout.
- **Persistent Usage & Hardware Analytics:** Track live generation speed (`tokens/sec`), resident VRAM/RAM utilization, session stats, daily usage, all-time totals, and a 14-day history stored in SQLite.
- **Optional Obsidian Vault Memory (MEMORA):** Connect your local Obsidian vault via MEMORA for hybrid search (BM25 + ChromaDB) and one-click **`⌘ Save to Memory`** directly from chat into your Markdown vault.
- **Atomic Backups & Portable Exports:** Export any chat to **Markdown (`.md`)** or **JSON (`.json`)**, and create/restore atomic SQLite database snapshots (`VACUUM INTO`) with pre-restore rollback protection.

---

## How to Use It

### 1. Prerequisites

Make sure you have the following installed on your machine:

- **[Ollama](https://ollama.com/)** running locally on `http://localhost:11434`
- **[Node.js](https://nodejs.org/)** (v20+ recommended)
- **[Rust & Cargo](https://rustup.rs/)** (required for Tauri 2)
- *(Optional)* **Python 3** and/or **Node.js** on your `PATH` if you want to run Python/JavaScript code blocks inside chat.

Pull at least one local chat model in Ollama:

```bash
ollama pull qwen2.5-coder:7b
```

### 2. Install & Run

Clone the repository and install frontend dependencies:

```bash
git clone https://github.com/siddartha1233/NEXUS.git
cd my-ai-platform
npm install
```

Start Nexus in development mode:

```bash
npm run tauri dev
```

To build a standalone native desktop installer:

```bash
npm run tauri build
```

### 3. Everyday Workflow

1. **General Chat:** Start chatting immediately or press `Ctrl + N` to start a fresh conversation. Switch models anytime from the composer bar or **Model Manager**.
2. **Workspaces (Courses & Projects):**
   - Open **Workspaces** in the sidebar and click **＋ New Workspace**.
   - Upload lecture PDFs or notes in the **Materials** tab.
   - Start a **Workspace Chat** — Nexus automatically searches your uploaded files via SQLite FTS5 and cites the exact filenames.
3. **Running Code Blocks:**
   - Ask Nexus for a SQL query, Python script, or JavaScript snippet.
   - Click **▶ Run Code** on the code card to execute it and inspect output, errors, and runtime in milliseconds.
4. **Analytics, Export & Backups:**
   - Open **Usage & Analytics** to view live `tokens/sec`, VRAM usage, today's usage, all-time totals, and 14-day history.
   - Use **`↓ Export .md`** or **`↓ .json`** in the chat top bar to save any conversation.
   - Open **Settings** to export or restore an atomic `.sqlite` database backup, or reset the SQL sandbox tables.

---

## Optional Configuration (Environment Variables)

Nexus works out of the box with zero configuration. If you use custom paths or want to integrate an external MEMORA Obsidian vault, you can set any of these optional environment variables:

| Variable | Purpose | Default Behavior |
|---|---|---|
| `MEMORA_DIR` | Path to your local MEMORA installation | Auto-detects `~/Downloads/MEMORA-master` if present |
| `MEMORA_PYTHON` | Path to the Python executable inside MEMORA's virtualenv | Auto-detects `.venv` inside `MEMORA_DIR`, then `python` on `PATH` |
| `NEXUS_PYTHON_PATH` | Custom Python executable for the Code Runner | Auto-detects `py -3`, `python3`, or `python` on `PATH` |
| `NEXUS_NODE_PATH` | Custom Node.js executable for the Code Runner | Auto-detects `node` on `PATH` |
| `NEXUS_ALLOW_SHELL_EXEC` | Set to `1` to opt in to raw PowerShell/CMD/Bash execution | Disabled by default for host safety |

---

## Tech Stack

| Layer | Technology |
|---|---|
| **Desktop Shell** | Tauri 2 (Native WebView + Rust IPC) |
| **Frontend** | React 19, TypeScript, Vite, custom dark-mode CSS |
| **Backend** | Rust (`tokio`, `reqwest`, `rusqlite`, `lopdf`) |
| **Local AI Inference** | Ollama (`http://localhost:11434`) |
| **Persistence & Search** | SQLite (WAL mode) + SQLite FTS5 (BM25) |
| **Optional Vault Memory** | MEMORA (BM25 + ChromaDB over Obsidian Markdown) |

---

## Testing & Verification

Run the frontend unit tests:

```bash
npm test
```

Run the TypeScript type-check and production frontend bundle:

```bash
npm run build
```

Run the Rust backend unit tests (SQL splitter, SQLite sandbox restrictions, path traversal protection, and FTS5 triggers):

```bash
cargo test --manifest-path src-tauri/Cargo.toml --lib
```

---

## Security & Privacy Design

- **100% Local Data:** Conversations, notes, materials, and telemetry stay in a local SQLite database (`nexus.db`). Nothing is sent to external servers.
- **Strict Content Security Policy:** Enforced in `src-tauri/tauri.conf.json`.
- **SQL Sandbox Isolation:** `SQLITE_LIMIT_ATTACHED = 0` prevents `ATTACH DATABASE`, `LOAD_EXTENSION`, and `VACUUM INTO` from accessing files outside `sandbox.db`.
- **No Silent Code Execution:** Code never runs automatically; local Python/JavaScript execution requires explicit click + confirmation, and raw OS shell commands are disabled by default.
- **Path Traversal & Restore Protection:** All backend file commands validate identifiers and extensions, block `..` traversal, and stage a pre-restore rollback copy before replacing `nexus.db`.
