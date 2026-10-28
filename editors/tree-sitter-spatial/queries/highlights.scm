; Spatial highlights, with nvim-treesitter capture names (used by Neovim and
; `tree-sitter highlight`). Copies with Helix and Zed capture names live in
; editors/helix and editors/zed; keep all three in sync.
; Later patterns take precedence over earlier ones.

; Identifiers ----------------------------------------------------------------

((identifier) @constant
  (#match? @constant "^[A-Z][A-Z0-9_]*$"))

((identifier) @variable.builtin
  (#eq? @variable.builtin "host"))

; Declarations ---------------------------------------------------------------

(const_declaration name: (identifier) @constant)

(pin_declaration name: (identifier) @type)
(block_declaration name: (identifier) @type)
(reg_declaration name: (identifier) @type)
(event_declaration name: (identifier) @type)
(flag_declaration name: (identifier) @type)
(stream_declaration name: (identifier) @type)

(function_declaration name: (identifier) @function)
(parameter name: (identifier) @variable.parameter)
(type_identifier) @type.builtin
(type via: (identifier) @variable)
(for_statement variable: (identifier) @variable)
(attach_statement name: (identifier) @variable)
(fire_statement event: (identifier) @type)

; Fields and members ---------------------------------------------------------

(field name: (identifier) @property)
(flag name: (identifier) @property)
(named_argument name: (identifier) @variable.parameter)
(property_identifier) @variable.member

; Calls ----------------------------------------------------------------------

(call_expression function: (identifier) @function.call)
(call_expression
  function: (member_expression property: (property_identifier) @function.method.call))
(set_statement "set" @function.method.call)

; Literals -------------------------------------------------------------------

(number) @number
(unit) @constant.builtin
(string) @string
(comment) @comment @spell

; Keywords -------------------------------------------------------------------

[
  "clock"
  "const"
  "pin"
  "timer"
  "shifter"
  "counter"
  "crc"
  "reg"
  "event"
  "flag"
  "stream"
  "test"
  "always"
] @keyword

"fn" @keyword.function
"export" @keyword.modifier
(pin_mode) @keyword.modifier

[
  "init"
  "on"
  "via"
  "as"
  "next"
  "fails"
  "fire"
  "wait"
  "expect"
  "attach"
  "wire"
] @keyword

[
  "if"
  "else"
] @keyword.conditional

[
  "for"
  "repeat"
] @keyword.repeat

(for_statement "in" @keyword.repeat)
(type "in" @keyword)

[
  "reply"
  "fail"
] @keyword.return

[
  "until"
  "at"
  "implies"
] @keyword.operator

; Operators and punctuation ----------------------------------------------------

[
  "="
  "=="
  "!="
  "<"
  ">"
  "<="
  ">="
  "||"
  "&&"
  "|"
  "^"
  "&"
  "<<"
  ">>"
  "+"
  "-"
  "*"
  "/"
  "!"
  "->"
  "..="
  "@"
] @operator

[
  "("
  ")"
  "["
  "]"
  "{"
  "}"
] @punctuation.bracket

[
  ","
  "."
  ":"
] @punctuation.delimiter
