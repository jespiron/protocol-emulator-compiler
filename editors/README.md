# Editor support for Spatial (`.spt`)

| Editor | What you get | Lives in |
| --- | --- | --- |
| VS Code, Cursor | TextMate grammar, comments, brackets | `vscode/` |
| Vim | Regex syntax, filetype detection, `commentstring` | `vim/` |
| Neovim | Tree-sitter highlighting (falls back to the Vim syntax) | `vim/` + `tree-sitter-spatial/` |
| Emacs | `spatial-mode` (font-lock, comments, brace indentation) | `emacs/` |
| Helix | Tree-sitter highlighting | `helix/` + `tree-sitter-spatial/` |
| Zed | Tree-sitter highlighting | `zed/` + `tree-sitter-spatial/` |

Every snippet below assumes `REPO` is the absolute path of your checkout:

```sh
REPO=/path/to/protocol-emulator-compiler
```

## VS Code / Cursor

Symlink the extension and reload the window (for Cursor use `~/.cursor/extensions`). See `vscode/README.md` to build a `.vsix` instead.

```sh
ln -s "$REPO/editors/vscode" ~/.vscode/extensions/spatial-syntax
```

## Vim

Install `editors/vim` as a package. It sets the filetype for `*.spt` (Vim's default maps `.spt` to SNOBOL4), adds regex highlighting and sets `commentstring` to `// %s`. If you use vim-plug, add `Plug '<repo>', { 'rtp': 'editors/vim' }` instead.

```sh
mkdir -p ~/.vim/pack/spatial/start
ln -s "$REPO/editors/vim" ~/.vim/pack/spatial/start/spatial
```

## Neovim

Install the same Vim package so Neovim recognizes `.spt`. Then build the tree-sitter parser into your `site` directory and link the queries. You need a C compiler, but not nvim-treesitter or the tree-sitter CLI. `ftplugin/spatial.lua` calls `vim.treesitter.start()` when it finds the parser. Without the parser, Neovim uses the Vim syntax file.

```sh
mkdir -p ~/.local/share/nvim/site/pack/spatial/start ~/.local/share/nvim/site/parser ~/.local/share/nvim/site/queries
ln -s "$REPO/editors/vim" ~/.local/share/nvim/site/pack/spatial/start/spatial
cc -O2 -shared -fPIC -I "$REPO/editors/tree-sitter-spatial/src" \
  "$REPO/editors/tree-sitter-spatial/src/parser.c" -o ~/.local/share/nvim/site/parser/spatial.so
ln -s "$REPO/editors/tree-sitter-spatial/queries" ~/.local/share/nvim/site/queries/spatial
```

nvim-treesitter users can register the parser with `install_info = { url = "/path/to/repo/editors/tree-sitter-spatial", files = { "src/parser.c" } }` (on the `master` branch) or with `install_info = { path = "/path/to/repo/editors/tree-sitter-spatial", queries = "queries" }` (on `main`), and then run `:TSInstall spatial`.

## Emacs

Load `spatial-mode.el`. It adds `\.spt\'` to `auto-mode-alist`. It works on Emacs 25.1 and later. There is no `spatial-ts-mode` yet, so Emacs 29 users get the same regex font-lock as everyone else.

```elisp
(add-to-list 'load-path "/path/to/protocol-emulator-compiler/editors/emacs")
(require 'spatial-mode)
```

## Helix

Add the language and grammar entries to your `languages.toml`, with `/path/to/repo` replaced by your checkout path. Then copy the queries into place and build the grammar. Helix uses its own capture names and lets the first matching pattern win, so it gets its own copy of `highlights.scm`.

```sh
mkdir -p ~/.config/helix/runtime/queries
sed "s|/path/to/repo|$REPO|" "$REPO/editors/helix/languages.toml" >> ~/.config/helix/languages.toml
cp -R "$REPO/editors/helix/runtime/queries/spatial" ~/.config/helix/runtime/queries/
hx --grammar build && hx --health spatial
```

## Zed

Open the command palette, run **zed: install dev extension**, and pick `$REPO/editors/zed`. Zed builds the grammar from the GitHub repository at the commit pinned in `zed/extension.toml`. That commit must be pushed first. Before then, try pointing `[grammars.spatial] repository` at `file:///path/to/repo`. Zed should accept a local repository there, but this hasn't been tested.

## GitHub rendering

Linguist has no entry for `.spt`, so GitHub shows Spatial files as plain text. For approximate highlighting, add this line to the repository's top-level `.gitattributes`:

```gitattributes
*.spt linguist-language=Rust
```

Rust is the closest common language in appearance. Spatial shares its `//` comments, braces, `fn name(x: T) -> T` signatures, `const NAME =`, `if cond { }` without parentheses, `for x in`, `..=` ranges, and `0x`/`0b` literals with `_` separators. Spatial never uses `'`, so Rust's lexing of chars and lifetimes can't misfire. The override also counts these files as Rust in the language statistics.

Real highlighting needs an upstream Linguist PR that adds Spatial to `languages.yml` with a TextMate grammar (`vscode/` would work). Linguist only accepts a language that is already in wide use: roughly a couple of hundred distinct repositories found by GitHub code search, not counting forks.

## Developing the grammar

```sh
cd editors/tree-sitter-spatial
npx --yes tree-sitter-cli@0.27.1 generate --abi 14   # regenerates src/ (commit it)
npx --yes tree-sitter-cli@0.27.1 parse --quiet --stat ../../examples/*.spt
npx --yes tree-sitter-cli@0.27.1 test                # runs test/highlight/*.spt
```

The parser is generated with ABI 14 so that older libtree-sitter builds can still load it. Whenever you change `queries/highlights.scm`, make the same change in `helix/runtime/queries/spatial/highlights.scm` and `zed/languages/spatial/highlights.scm`.
