; Spatial highlights for Helix. Same rules as
; editors/tree-sitter-spatial/queries/highlights.scm, with Helix capture names.
; In Helix the first matching pattern wins, so specific patterns come first.

; Literals -------------------------------------------------------------------

(comment) @comment
(string) @string
(number) @constant.numeric
(unit) @constant.builtin

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

"fn" @keyword.function
"export" @keyword.storage.modifier
(pin_mode) @keyword.storage.modifier

[
  "if"
  "else"
] @keyword.control.conditional

[
  "for"
  "repeat"
] @keyword.control.repeat

(for_statement "in" @keyword.control.repeat)
(type "in" @keyword)

[
  "reply"
  "fail"
] @keyword.control.return

[
  "until"
  "at"
  "implies"
] @keyword.operator

; Declarations ---------------------------------------------------------------

(const_declaration name: (identifier) @constant)

(pin_declaration name: (identifier) @type)
(block_declaration name: (identifier) @type)
(reg_declaration name: (identifier) @type)
(event_declaration name: (identifier) @type)
(flag_declaration name: (identifier) @type)
(stream_declaration name: (identifier) @type)
(fire_statement event: (identifier) @type)

(function_declaration name: (identifier) @function)
(parameter name: (identifier) @variable.parameter)
(type_identifier) @type.builtin
(type via: (identifier) @variable)
(for_statement variable: (identifier) @variable)
(attach_statement name: (identifier) @variable)

; Calls ----------------------------------------------------------------------

(call_expression
  function: (member_expression property: (property_identifier) @function.method))
(set_statement "set" @function.method)
(call_expression function: (identifier) @function)

; Fields and members ---------------------------------------------------------

(field name: (identifier) @variable.other.member)
(flag name: (identifier) @variable.other.member)
(named_argument name: (identifier) @variable.parameter)
(property_identifier) @variable.other.member

; Identifiers ----------------------------------------------------------------

((identifier) @variable.builtin
  (#eq? @variable.builtin "host"))

((identifier) @constant
  (#match? @constant "^[A-Z][A-Z0-9_]*$"))

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
