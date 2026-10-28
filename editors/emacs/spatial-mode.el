;;; spatial-mode.el --- Major mode for Spatial (.spt) files  -*- lexical-binding: t; -*-

;; Author: Jessica Ruan <jess@standardkernel.com>
;; Version: 0.1.0
;; Package-Requires: ((emacs "25.1"))
;; Keywords: languages
;; URL: https://github.com/jespiron/protocol-emulator-compiler

;; This file is not part of GNU Emacs.

;;; Commentary:

;; Syntax highlighting, comments and brace indentation for Spatial, a small
;; hardware-description language.  The highlighting rules mirror the VS Code
;; TextMate grammar in editors/vscode/syntaxes/spatial.tmLanguage.json.
;;
;; Install:
;;
;;   (add-to-list 'load-path "/path/to/repo/editors/emacs")
;;   (require 'spatial-mode)
;;
;; `.spt' files then open in `spatial-mode'.

;;; Code:

(defgroup spatial nil
  "Major mode for Spatial (.spt) files."
  :group 'languages
  :prefix "spatial-")

(defcustom spatial-indent-offset 4
  "Number of spaces per brace level in `spatial-mode'."
  :type 'integer
  :safe #'integerp
  :group 'spatial)

(defface spatial-property-face
  `((t :inherit ,(if (facep 'font-lock-property-name-face)
                     'font-lock-property-name-face
                   'font-lock-variable-name-face)))
  "Face for field and argument names (`every:', `run:')."
  :group 'spatial)

(defface spatial-member-face
  `((t :inherit ,(if (facep 'font-lock-property-use-face)
                     'font-lock-property-use-face
                   'font-lock-variable-name-face)))
  "Face for member accesses (`sender.out')."
  :group 'spatial)

(defface spatial-call-face
  `((t :inherit ,(if (facep 'font-lock-function-call-face)
                     'font-lock-function-call-face
                   'font-lock-function-name-face)))
  "Face for function and method calls."
  :group 'spatial)

(defface spatial-number-face
  `((t :inherit ,(if (facep 'font-lock-number-face)
                     'font-lock-number-face
                   'font-lock-constant-face)))
  "Face for numeric literals."
  :group 'spatial)

(defconst spatial--ident "[[:alpha:]_][[:alnum:]_]*"
  "Regexp matching a Spatial identifier.")

(defconst spatial--number "\\(?:0x[[:xdigit:]_]+\\|0b[01_]+\\|[0-9][0-9_]*\\)"
  "Regexp matching a Spatial number literal.")

(defconst spatial--decl-keywords
  '("clock" "export" "test" "always" "event" "flag" "pin" "timer" "shifter"
    "counter" "crc" "reg" "const" "stream" "fn")
  "Keywords that introduce items.")

(defconst spatial--control-keywords
  '("fire" "wait" "next" "repeat" "for" "in" "if" "else" "reply" "fails" "fail"
    "expect" "attach" "as" "wire")
  "Statement keywords.")

(defconst spatial--word-operators
  '("until" "at" "implies" "via" "init" "on")
  "Keywords used as operators or modifiers.")

(defconst spatial--units
  '("cycle" "cycles" "ns" "us" "ms" "s" "Hz" "kHz" "MHz")
  "Units that may follow a number to form a quantity.")

(defconst spatial--types '("bits" "bit" "u7" "u8" "bytes")
  "Built-in parameter types.")

(defconst spatial--builtins '("host" "msb_first" "lsb_first" "even" "odd")
  "Built-in names used as values.")

(defconst spatial--flags '("pulse" "lead" "end_on_sample")
  "Bare flags allowed inside timer, shifter, counter and crc blocks.")

(defconst spatial-font-lock-keywords
  (let ((id spatial--ident))
    ;; Without OVERRIDE, the first rule to fontify a span wins, so the
    ;; specific rules come first.
    `(;; pin NAME [@ pad]: in|out|open_drain
      (,(concat "\\_<pin\\_>[ \t]+" id "[^:\n]*:[ \t]*"
                "\\(" (regexp-opt '("in" "out" "open_drain") 'symbols) "\\)")
       (1 font-lock-builtin-face))
      ;; Declarations: timer NAME, reg NAME, ...
      (,(concat "\\_<"
                (regexp-opt '("pin" "timer" "shifter" "counter" "crc" "reg" "stream") t)
                "\\_>[ \t]+\\(" id "\\)")
       (1 font-lock-keyword-face) (2 font-lock-type-face))
      (,(concat "\\_<\\(const\\)\\_>[ \t]+\\(" id "\\)")
       (1 font-lock-keyword-face) (2 font-lock-constant-face))
      (,(concat "\\_<\\(fn\\)\\_>[ \t]+\\(" id "\\)")
       (1 font-lock-keyword-face) (2 font-lock-function-name-face))
      ;; event a, b / flag a, b
      ("\\_<\\(event\\|flag\\)\\_>"
       (1 font-lock-keyword-face)
       (,(concat "\\=[ \t]*,?[ \t]*\\(" id "\\)") nil nil (1 font-lock-type-face)))
      ;; Quantities: 64 cycles, 50 MHz
      (,(concat "\\_<\\(" spatial--number "\\)[ \t]+"
                (regexp-opt spatial--units 'symbols))
       (1 'spatial-number-face) (2 font-lock-constant-face))
      (,(regexp-opt spatial--decl-keywords 'symbols) . font-lock-keyword-face)
      (,(regexp-opt spatial--control-keywords 'symbols) . font-lock-keyword-face)
      (,(regexp-opt spatial--word-operators 'symbols) . font-lock-keyword-face)
      (,(concat "\\_<" spatial--number "\\_>") . 'spatial-number-face)
      ;; `via NAME' names a shifter, never a type.
      (,(concat "\\_<\\(via\\)\\_>[ \t]+\\(" id "\\)")
       (1 font-lock-keyword-face) (2 'default))
      ;; Types, unless used as a value (`bit.tick', `bit: BIT').
      (,(concat (regexp-opt spatial--types 'symbols)
                "\\(?:[ \t]*[^.(: \t\n]\\|[ \t]*$\\)")
       (1 font-lock-type-face))
      (,(concat (regexp-opt '("data" "len") 'symbols) "[ \t]*(")
       (1 font-lock-builtin-face))
      (,(regexp-opt spatial--builtins 'symbols) . font-lock-builtin-face)
      (,(regexp-opt spatial--flags 'symbols) . 'spatial-property-face)
      ;; Calls and members.
      (,(concat "\\_<\\(" id "\\)[ \t]*(") (1 'spatial-call-face))
      ("\\.\\(set\\)[ \t]*{" (1 'spatial-call-face))
      (,(concat "\\.\\(" id "\\)") (1 'spatial-member-face))
      ;; Field and named-argument names.
      (,(concat "\\_<\\(" id "\\)[ \t]*:") (1 'spatial-property-face))
      ("\\_<[[:upper:]][[:upper:][:digit:]_]*\\_>" . font-lock-constant-face)))
  "Font-lock rules for `spatial-mode'.")

(defvar spatial-mode-syntax-table
  (let ((table (make-syntax-table)))
    (modify-syntax-entry ?/ ". 12" table)
    (modify-syntax-entry ?\n ">" table)
    (modify-syntax-entry ?_ "_" table)
    (modify-syntax-entry ?\" "\"" table)
    (modify-syntax-entry ?\\ "." table)
    (modify-syntax-entry ?' "." table)
    (dolist (c '(?+ ?- ?* ?& ?| ?^ ?! ?< ?> ?= ?@ ?: ?, ?.))
      (modify-syntax-entry c "." table))
    table)
  "Syntax table for `spatial-mode'.")

(defun spatial-indent-line ()
  "Indent the current line by brace depth."
  (interactive)
  (let ((indent (save-excursion
                  (beginning-of-line)
                  (let ((depth (car (syntax-ppss))))
                    (skip-chars-forward " \t")
                    (when (memq (char-after) '(?\} ?\] ?\)))
                      (setq depth (1- depth)))
                    (* spatial-indent-offset (max depth 0))))))
    (if (> (current-column) (current-indentation))
        (save-excursion (indent-line-to indent))
      (indent-line-to indent))))

;;;###autoload
(define-derived-mode spatial-mode prog-mode "Spatial"
  "Major mode for editing Spatial (.spt) hardware descriptions."
  :syntax-table spatial-mode-syntax-table
  :group 'spatial
  (setq-local comment-start "// ")
  (setq-local comment-end "")
  (setq-local comment-start-skip "//+[ \t]*")
  (setq-local indent-line-function #'spatial-indent-line)
  (setq-local electric-indent-chars (append "{}[]()" electric-indent-chars))
  (setq-local font-lock-defaults '(spatial-font-lock-keywords)))

;;;###autoload
(add-to-list 'auto-mode-alist '("\\.spt\\'" . spatial-mode))

(provide 'spatial-mode)

;;; spatial-mode.el ends here
