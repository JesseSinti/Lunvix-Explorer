<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { convertFileSrc } from "@tauri-apps/api/core";

  let currentDir: string = "C:\\";
  let files: any[] = [];
  let sidebar: any = { quick_access: [], system_disks: [] };
  let selectedFiles: Set<string> = new Set();

  let searchQuery = "";
  let sortColumn = "Name";
  let sortOrder = "Ascending";
  let pilType = "All";
  let isLoading = false;

  let showPreviewPanel = false;
  let previewFile: any = null;
  let previewContent = "";

  let isRenaming = false;
  let isCreating = false;
  let createIsFolder = false;
  let promptText = "";

  $: {
    if (currentDir || sortColumn || sortOrder || pilType) fetchFiles();
  }

  async function fetchFiles() {
    isLoading = true;
    try {
      files = await invoke("read_directory", {
        path: currentDir,
        sortColumn,
        sortOrder,
        pilType,
        searchQuery,
      });
      selectedFiles = new Set();
    } catch (e) {
      console.error(e);
    } finally {
      isLoading = false;
    }
  }

  onMount(async () => {
    sidebar = await invoke("get_sidebar");
    listen("fs_changed", () => fetchFiles());
  });

  $: breadcrumbs = currentDir.split(/[\\/]/).filter(Boolean);

  function toggleSelection(path: string) {
    if (selectedFiles.has(path)) selectedFiles.delete(path);
    else selectedFiles.add(path);
    selectedFiles = selectedFiles;
  }

  async function handleDelete() {
    if (selectedFiles.size === 0) return;
    await invoke("delete_files", { paths: Array.from(selectedFiles) });
    fetchFiles();
  }

  async function handleCreate() {
    let newPath = `${currentDir}\\${promptText}`;
    await invoke("create_item", { path: newPath, isDir: createIsFolder });
    isCreating = false;
    promptText = "";
    fetchFiles();
  }

  async function handleRename() {
    if (selectedFiles.size !== 1) return;
    let oldPath = Array.from(selectedFiles)[0];
    let newPath = `${currentDir}\\${promptText}`;
    await invoke("rename_item", { oldPath, newPath });
    isRenaming = false;
    promptText = "";
    fetchFiles();
  }

  async function selectForPreview(file: any) {
    if (file.display_kind === "Directory") return;
    previewFile = file;

    const ext = file.lowercase_ext;
    if (["txt", "rs", "py", "js", "json", "md", "html", "css"].includes(ext)) {
      previewContent = await invoke("read_file_text", { path: file.full_path });
    } else {
      previewContent = "";
    }
  }
</script>

<div
  class="flex flex-col h-screen w-screen bg-[#08080d] text-[#e4e4f0] font-sans overflow-hidden"
>
  <header
    class="flex flex-col border-b border-[#1c1c2a] shrink-0 bg-linear-to-r from-[#0b0e18] to-[#121622]"
  >
    <div class="flex items-center px-4 py-3 gap-4">
      <div class="text-[#00d2ff] font-bold text-sm tracking-wide">
        ◈ Lunvix Explorer
      </div>
      <div class="h-6 w-px bg-[#36364a]"></div>

      <button
        on:click={() => (showPreviewPanel = !showPreviewPanel)}
        class="px-2 py-1 text-[#00d2ff] bg-[#003246] rounded border border-[#00d2ff]"
        >👁</button
      >

      <div
        class="flex-1 flex items-center bg-[#1a1a26] border border-[#36364a] rounded px-3 py-1.5 focus-within:border-[#00d2ff] transition-colors"
      >
        <span class="text-[#73738c] mr-2">⌕</span>
        <input
          type="text"
          bind:value={searchQuery}
          placeholder="Search files..."
          class="bg-transparent outline-none w-full text-sm text-[#e4e4f0]"
          on:keyup={fetchFiles}
        />
      </div>

      <select
        bind:value={sortColumn}
        class="bg-[#1a1a26] border border-[#36364a] rounded px-2 py-1 text-sm"
      >
        <option value="Name">Name</option>
        <option value="Size">Size</option>
        <option value="ModifiedDate">Modified</option>
      </select>
      <select
        bind:value={sortOrder}
        class="bg-[#1a1a26] border border-[#36364a] rounded px-2 py-1 text-sm"
      >
        <option value="Ascending">Ascending</option>
        <option value="Descending">Descending</option>
      </select>

      <div class="h-6 w-px bg-[#36364a]"></div>

      <button
        on:click={() => {
          isCreating = true;
          createIsFolder = true;
          promptText = "New Folder";
        }}
        class="px-3 py-1 text-sm bg-[#1a1a26] border border-[#36364a] rounded hover:bg-[#10202a]"
        >⊕ Folder</button
      >
      <button
        on:click={() => {
          isCreating = true;
          createIsFolder = false;
          promptText = "new_file.txt";
        }}
        class="px-3 py-1 text-sm bg-[#1a1a26] border border-[#36364a] rounded hover:bg-[#10202a]"
        >⊕ File</button
      >

      <button
        disabled={selectedFiles.size !== 1}
        on:click={() => {
          isRenaming = true;
          promptText = "New Name";
        }}
        class="px-3 py-1 text-sm bg-[#1a1a26] border border-[#36364a] rounded disabled:opacity-50"
        >✏ Rename</button
      >
      <button
        on:click={handleDelete}
        disabled={selectedFiles.size === 0}
        class="px-3 py-1 bg-[#2c0e12] border border-[#581a22] text-[#ff4250] rounded text-sm disabled:opacity-50"
        >🗑 Delete</button
      >
    </div>

    <div
      class="flex items-center px-4 py-2 bg-[#0d0d14] border-t border-[#1c1c2a] gap-4"
    >
      <div class="flex gap-1 text-sm font-mono text-[#73738c]">
        {#each breadcrumbs as crumb, i}
          <button
            class="hover:text-[#e4e4f0] cursor-pointer"
            on:click={() =>
              (currentDir = breadcrumbs.slice(0, i + 1).join("\\") + "\\")}
          >
            {crumb}
          </button>
          {#if i < breadcrumbs.length - 1}
            <span class="mx-1">›</span>
          {/if}
        {/each}
      </div>

      <div class="flex-1"></div>

      <div class="flex gap-2">
        {#each ["All", "Hidden", "Code", "Media", "Docs"] as cat}
          <button
            on:click={() => (pilType = cat)}
            class="px-3 py-1 rounded-full text-xs {pilType === cat
              ? 'bg-[#00d2ff] text-[#08080d]'
              : 'bg-[#1a1a26] text-[#73738c]'}"
          >
            {cat}
          </button>
        {/each}
      </div>
    </div>
  </header>

  <div class="flex flex-1 overflow-hidden relative">
    <aside
      class="w-56 bg-[#0d0d14] border-r border-[#1c1c2a] flex flex-col py-4 overflow-y-auto shrink-0"
    >
      <div class="px-4 text-[10px] font-bold text-[#73738c] mb-2">
        QUICK ACCESS
      </div>
      {#each sidebar.quick_access as [name, path]}
        <button
          on:click={() => (currentDir = path)}
          class="text-left px-4 py-1.5 text-xs {currentDir === path
            ? 'text-[#00d2ff] bg-[#003246] border-l-2 border-[#00d2ff]'
            : 'text-[#73738c] hover:bg-[#14141e]'}"
        >
          {name}
        </button>
      {/each}

      <div class="px-4 text-[10px] font-bold text-[#73738c] mt-6 mb-2">
        DEVICES
      </div>
      {#each sidebar.system_disks as [name, path]}
        <button
          on:click={() => (currentDir = path)}
          class="text-left px-4 py-1.5 text-xs {currentDir === path
            ? 'text-[#00d2ff] bg-[#003246] border-l-2 border-[#00d2ff]'
            : 'text-[#73738c] hover:bg-[#14141e]'}"
        >
          💾 {name}
        </button>
      {/each}
    </aside>

    <main class="flex-1 overflow-y-auto bg-[#08080d] relative">
      <table class="w-full text-left border-collapse text-xs">
        <thead
          class="sticky top-0 bg-[#0d0d14] text-[10px] font-bold text-[#00d2ff] border-b border-[#1c1c2a]"
        >
          <tr>
            <th class="py-2 px-4 w-8"></th>
            <th class="py-2 px-4">NAME</th>
            <th class="py-2 px-4">SIZE</th>
            <th class="py-2 px-4">KIND</th>
            <th class="py-2 px-4">CREATED</th>
            <th class="py-2 px-4">MODIFIED</th>
          </tr>
        </thead>
        <tbody>
          {#each files as file}
            <tr
              on:click={() => selectForPreview(file)}
              on:dblclick={() =>
                file.display_kind === "Directory"
                  ? (currentDir = file.full_path)
                  : null}
              class="border-b border-[#1c1c2a] hover:bg-[#14141e] cursor-pointer {selectedFiles.has(
                file.full_path,
              )
                ? 'bg-[#142d3c]'
                : 'even:bg-[#0b0b11]'}"
            >
              <td class="py-2 px-4">
                <input
                  type="checkbox"
                  checked={selectedFiles.has(file.full_path)}
                  on:change={() => toggleSelection(file.full_path)}
                  class="accent-[#00d2ff]"
                />
              </td>
              <td class="py-2 px-4 text-[#e4e4f0] flex items-center gap-2">
                <span>{file.icon}</span>
                {file.display_name}
              </td>
              <td class="py-2 px-4 text-[#73738c]">{file.display_size}</td>
              <td class="py-2 px-4 text-[#008cb4]"
                >{file.display_kind === "File"
                  ? file.display_ext || "File"
                  : "Folder"}</td
              >
              <td class="py-2 px-4 text-[#73738c]">{file.display_created}</td>
              <td class="py-2 px-4 text-[#73738c]">{file.display_modified}</td>

              <td class="py-2 px-4 flex gap-2">
                <button
                  class="text-[#73738c] hover:text-[#00d2ff]"
                  on:click|stopPropagation={() => {
                    console.log("Attempting to open");
                    invoke("open_in_terminal", {
                      path: file.full_path,
                    })
                      .then(() => console.log("attemtpting"))
                      .catch((err) => console.log(err));
                  }}>💻</button
                >
                <button
                  class="text-[#73738c] hover:text-[#00d2ff]"
                  on:click|stopPropagation={() =>
                    invoke("email_item", { path: file.full_path })}>✉</button
                >
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
    </main>

    {#if showPreviewPanel && previewFile}
      <aside
        class="w-64 bg-[#0d0d14] border-l border-[#1c1c2a] p-4 overflow-y-auto shrink-0 flex flex-col items-center"
      >
        <div class="text-4xl mb-4">{previewFile.icon}</div>
        <div class="text-[#e4e4f0] font-bold text-center break-all">
          {previewFile.display_name}
        </div>
        <div class="text-xs text-[#73738c] mt-2 mb-6">
          {previewFile.display_size} • {previewFile.display_kind}
        </div>

        {#if ["png", "jpg", "jpeg", "gif", "webp"].includes(previewFile.lowercase_ext)}
          <img
            src={convertFileSrc(previewFile.full_path)}
            alt="preview"
            class="max-w-full rounded border border-[#36364a]"
          />
        {:else if previewContent}
          <pre
            class="w-full text-[10px] bg-[#1a1a26] p-2 rounded overflow-x-auto text-[#e4e4f0] border border-[#36364a]">{previewContent}</pre>
        {:else}
          <div class="text-[#73738c] text-sm text-center">
            No preview available for this format.
          </div>
        {/if}
      </aside>
    {/if}
  </div>

  {#if isRenaming || isCreating}
    <div
      class="absolute inset-0 bg-black/60 flex items-center justify-center z-50"
    >
      <div
        class="bg-[#14141e] p-6 rounded-xl border border-[#36364a] w-80 shadow-2xl"
      >
        <h3 class="text-[#00d2ff] font-bold mb-4">
          {isRenaming ? "Rename Item" : "Create Item"}
        </h3>
        <input
          type="text"
          bind:value={promptText}
          class="w-full bg-[#1a1a26] border border-[#36364a] rounded px-3 py-2 text-[#e4e4f0] outline-none focus:border-[#00d2ff] mb-4"
        />
        <div class="flex justify-end gap-2">
          <button
            on:click={() => {
              isRenaming = false;
              isCreating = false;
            }}
            class="px-4 py-2 text-sm text-[#73738c] hover:text-[#e4e4f0]"
            >Cancel</button
          >
          <button
            on:click={isRenaming ? handleRename : handleCreate}
            class="px-4 py-2 text-sm bg-[#00d2ff] text-black rounded font-bold"
            >Confirm</button
          >
        </div>
      </div>
    </div>
  {/if}
</div>
