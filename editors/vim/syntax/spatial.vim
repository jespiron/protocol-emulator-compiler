" Vim syntax file
" Language:   Spatial (.spt), a small hardware-description language
" Mirrors the token rules of editors/vscode/syntaxes/spatial.tmLanguage.json.

if exists('b:current_syntax')
  finish
endif

let s:cpo_save = &cpo
set cpo&vim

syn case match

" Generic identifiers first: later :syn match items win at the same column,
" and :syn keyword items beat every :syn match.
syn match   spatialConstant     "\<\u[A-Z0-9_]*\>"
syn match   spatialFunctionCall "\<\h\w*\ze\s*("
syn match   spatialBuiltinFunc  "\<\%(data\|len\)\ze\s*("
syn match   spatialBuiltin      "\<\%(host\|msb_first\|lsb_first\|even\|odd\)\>"
syn match   spatialFlag         "\<\%(pulse\|lead\|end_on_sample\)\>"
syn match   spatialMember       "\.\@1<=\h\w*"
syn match   spatialMethodCall   "\.\@1<=\%(\h\w*\ze\s*(\|set\ze\s*{\)"
syn match   spatialProperty     "\<\h\w*\ze\s*:"
syn match   spatialType         "\%(\<via\s\+\)\@<!\<\%(bits\|bit\|u7\|u8\|bytes\)\>\%(\s*[.(:]\)\@!"

" Numbers and quantities (`64 cycles`, `50 MHz`).
syn match   spatialNumber       "\<\%(0x[0-9A-Fa-f_]\+\|0b[01_]\+\|\d[0-9_]*\)\>"
syn match   spatialUnit         "\%(\<\%(0x[0-9A-Fa-f_]\+\|0b[01_]\+\|\d[0-9_]*\)\s\+\)\@<=\%(cycles\|cycle\|ns\|us\|ms\|s\|Hz\|kHz\|MHz\)\>"

" Keywords.
syn keyword spatialDeclKeyword  clock export test always event flag pin timer shifter counter crc reg const stream
syn keyword spatialFnKeyword    fn nextgroup=spatialFunctionName skipwhite
syn keyword spatialStatement    fire wait repeat for if else reply fails fail expect attach as wire next
syn match   spatialStatement    "\<in\>"
syn keyword spatialWordOperator until at implies via init on

" Declaration names (`pin NAME`, `timer NAME`, `event a, b`, `fn NAME`).
syn match   spatialDeclName     "\%(\<\%(pin\|timer\|shifter\|counter\|crc\|reg\|stream\)\s\+\)\@<=\h\w*"
syn match   spatialConstName    "\%(\<const\s\+\)\@<=\h\w*"
syn match   spatialDeclName     "\%(\<\%(event\|flag\)\s\+\)\@<=\h\w*\%(\s*,\s*\h\w*\)*" contains=spatialComma
syn match   spatialComma        "," contained
syn match   spatialFunctionName "\h\w*" contained
syn match   spatialPinMode      "\%(\<pin\s\+\h\w*[^:]*:\s*\)\@<=\<\%(in\|out\|open_drain\)\>"

syn match   spatialOperator     "->\|==\|!=\|<=\|>=\|&&\|||\|<<\|>>\|\.\.=\|[!+\-*/&|^<>=@]"

syn region  spatialString       start=+"+ end=+"+ end=+$+ oneline
syn match   spatialComment      "//.*$" contains=spatialTodo,@Spell
syn keyword spatialTodo         TODO FIXME XXX NOTE contained

hi def link spatialDeclKeyword  Keyword
hi def link spatialFnKeyword    Keyword
hi def link spatialStatement    Statement
hi def link spatialWordOperator Keyword
hi def link spatialDeclName     Type
hi def link spatialConstName    Constant
hi def link spatialComma        Delimiter
hi def link spatialFunctionName Function
hi def link spatialFunctionCall Function
hi def link spatialMethodCall   Function
hi def link spatialBuiltinFunc  Special
hi def link spatialBuiltin      Special
hi def link spatialFlag         Identifier
hi def link spatialMember       Identifier
hi def link spatialProperty     Identifier
hi def link spatialPinMode      StorageClass
hi def link spatialType         Type
hi def link spatialConstant     Constant
hi def link spatialNumber       Number
hi def link spatialUnit         Constant
hi def link spatialOperator     Operator
hi def link spatialString       String
hi def link spatialComment      Comment
hi def link spatialTodo         Todo

let b:current_syntax = 'spatial'

let &cpo = s:cpo_save
unlet s:cpo_save
