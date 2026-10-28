" Vim filetype plugin for Spatial (.spt)

if exists('b:did_ftplugin')
  finish
endif
let b:did_ftplugin = 1

setlocal commentstring=//\ %s
setlocal comments=://
setlocal formatoptions-=t formatoptions+=croql

let b:undo_ftplugin = 'setlocal commentstring< comments< formatoptions<'
