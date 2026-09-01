-- formality (fml) — Neovim integration
-- ====================================
--
-- One formatter for every language `fml` covers, with no per-language
-- formatter plugins. `fml lsp` speaks LSP over stdio like any other server,
-- so no third-party plugin (not even nvim-lspconfig) is required.
--
-- What this gives you:
--   * `textDocument/formatting` routed through `fml fmt` (format on save below)
--   * `fml lint` diagnostics published on open / save
--
-- What this is NOT: a replacement for rust-analyzer, pyright, gopls, clangd,
-- etc. Keep your existing language servers attached for completion, hover and
-- go-to-definition. `fml lsp` only owns formatting and lint diagnostics.
--
-- On startup `fml lsp` may log lines like "child LSP 'pyright-langserver' not
-- found" to `:LspLog`. These are harmless leftovers from an earlier design and
-- are not surfaced as notifications; nothing is spawned.
--
-- Install: copy this file to  ~/.config/nvim/plugin/formality.lua
-- (Neovim 0.10+ for `vim.fs.root`; tested on 0.11.)
--
-- If `fml` is not on your PATH, replace the two occurrences of "fml" in
-- `cmd` below with an absolute path.

local FILETYPES = {
  "rust",
  "python",
  "markdown",
  "yaml",
  "json",
  "jsonc",
  "toml",
  "typst",
  "go",
  "c",
  "cpp",
  "java",
  "kotlin",
  "javascript",
  "javascriptreact",
  "typescript",
  "typescriptreact",
}

local group = vim.api.nvim_create_augroup("formality", { clear = true })

-- Start `fml lsp` for any buffer whose filetype fml can format, rooted at the
-- nearest formality.toml / .formality.toml (falling back to the repo root).
vim.api.nvim_create_autocmd("FileType", {
  group = group,
  pattern = FILETYPES,
  callback = function(args)
    local root = vim.fs.root(args.buf, {
      "formality.toml",
      ".formality.toml",
      ".git",
    })
    if not root then
      return
    end
    vim.lsp.start({
      name = "formality",
      cmd = { "fml", "lsp" },
      root_dir = root,
    }, { bufnr = args.buf })
  end,
})

-- Format on save.
--
-- `fml lsp` declares `textDocumentSync = none` and formats by rewriting the
-- file on disk, then returning the edits. That means it must format the
-- *saved* bytes, not the in-memory buffer — so we format on BufWritePost
-- (after Neovim has written the buffer) and then reload the freshly
-- formatted file. Formatting in BufWritePre instead would make Neovim warn
-- that the file "changed since reading it" on every save.
vim.api.nvim_create_autocmd("LspAttach", {
  group = group,
  callback = function(args)
    local client = vim.lsp.get_client_by_id(args.data.client_id)
    if not client or client.name ~= "formality" then
      return
    end

    vim.bo[args.buf].autoread = true

    vim.api.nvim_create_autocmd("BufWritePost", {
      group = group,
      buffer = args.buf,
      callback = function(ev)
        vim.lsp.buf.format({
          name = "formality",
          bufnr = ev.buf,
          timeout_ms = 3000,
        })
        -- The buffer now matches what `fml lsp` wrote to disk; clear the
        -- transient "modified" flag and reload so mtime is back in sync and
        -- the next `:w` is quiet.
        vim.bo[ev.buf].modified = false
        local view = vim.fn.winsaveview()
        vim.cmd("silent! edit")
        vim.fn.winrestview(view)
      end,
    })
  end,
})
