" Vim maps *.spt to snobol4 with `setf`; `set filetype=` runs after it and wins.
autocmd BufNewFile,BufRead *.spt set filetype=spatial
