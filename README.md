◈ Lunvix Explorer
Lunvix is a high-performance, developer-centric file explorer that combines the safety and speed of Rust with a fluid, modern Svelte interface. Unlike traditional explorers that crawl every time you open a folder, Lunvix uses a lightning-fast key-value database to index and cache your filesystem.

🚀 Key Features
⚡ Blazing Fast Indexing
Parallel Filesystem Walking: Utilizes jwalk to perform multi-threaded directory traversal.

Persistent Redb Cache: Stores file metadata in a high-performance redb (embedded KV store) to provide near-instant search results and directory loads.

Real-time File Watching: Uses a debounced filesystem watcher (notify) to keep the UI in sync with disk changes without spamming updates.

🛠 Developer Workflow Integration
Contextual Terminal: Open a terminal directly at your current path. It intelligently handles files by opening their parent directory.

Email Integration: native OS integration to instantly attach files to new emails (supports Outlook/PowerShell on Windows, Mail.app on macOS, and xdg-email on Linux).

👁 Smart UI & Preview
Rich Previews: Instant text previews for code files and visual previews for images via Tauri’s asset protocol.

Category Filtering: Quickly isolate files using "PIL" filters: Code, Media, Docs, and Hidden files.

Visual Iconography: Dynamic emoji-based icons mapped to hundreds of specific file extensions.

📂 System Management
Disk & Device Detection: Automatically identifies and mounts system disks and quick-access folders (Home, Desktop, Downloads).

Safe CRUD: Full support for creating, renaming, and moving items to the trash via the trash crate.

🏗 Architecture
Backend (Rust)
The core logic resides in a robust Rust backend:

State Management: Uses DashMap for thread-safe, concurrent access to the directory tree and file nodes.

Database: redb manages three primary tables: entries (metadata), tree (directory structure), and metadata (indexing status).

IPC Bridge: Custom Tauri commands handle everything from directory reconciliation to native OS shell executions.

Frontend (Svelte)
A reactive dashboard built for speed:

Tailwind CSS: A dark-themed, "Cyberpunk-minimal" aesthetic with a focus on scannability.

Sveltekit: Leverages Svelte's reactivity for breadcrumb navigation and real-time search filtering.

🛠 Installation
Prerequisites
Rust: rustc and cargo installed.

Node.js: npm or pnpm for frontend dependencies.

OS Dependencies: Webview2 (Windows), AppKit (macOS), or WebKitGTK (Linux).

Build Instructions
Clone & Install:

Bash
git clone https://github.com/yourusername/lunvix.git
cd lunvix
npm install
Run Development:

Bash
npm run tauri dev
Build Production:

Bash
npm run tauri build
⚙️ Configuration
Lunvix automatically detects your environment.

Windows: Uses powershell for email and cmd /C start for terminal windows.

macOS: Uses osascript for Mail.app and open for the default Terminal.

Linux: Defaults to xdg-open, xdg-email, and a prioritized list of terminal emulators (Alacritty, Kitty, etc.).

macOS: Terminal.app via open.

Linux: Searches for gnome-terminal, konsole, alacritty, kitty, and xterm.
