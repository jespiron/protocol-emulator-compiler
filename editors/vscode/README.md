# Spatial (.spt) syntax highlighting

A small VS Code / Cursor extension that adds syntax highlighting for Spatial `.spt` files. It has no build step and no dependencies.

## Install

Copy or symlink this folder into your extensions directory, then reload the editor:

```sh
ln -s "$(pwd)/editors/vscode" ~/.vscode/extensions/spatial-syntax   # Cursor: ~/.cursor/extensions/
```

Or package it and install the `.vsix`:

```sh
cd editors/vscode
npx @vscode/vsce package
code --install-extension spatial-syntax-0.1.0.vsix
```

## GitHub

Linguist does not know about Spatial yet. Adding a new language takes an upstream PR, so for now you can add this line to `.gitattributes` to get approximate highlighting on GitHub:

```
*.spt linguist-language=Rust
```
