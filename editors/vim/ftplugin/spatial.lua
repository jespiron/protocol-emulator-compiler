-- Neovim only: use the tree-sitter parser when one is installed
-- (editors/tree-sitter-spatial). Otherwise syntax/spatial.vim highlights.
if #vim.api.nvim_get_runtime_file('parser/spatial.*', false) > 0 then
  pcall(vim.treesitter.start)
end
