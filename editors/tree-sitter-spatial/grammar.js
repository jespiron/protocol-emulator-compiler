/**
 * @file Tree-sitter grammar for Spatial (.spt), a small hardware-description language.
 * @license UNLICENSED
 */

/* eslint-disable arrow-parens, camelcase */
// @ts-check

const PREC = {
  until: 1,
  implies: 2,
  or: 3,
  and: 4,
  compare: 5,
  bit_or: 6,
  bit_xor: 7,
  bit_and: 8,
  additive: 9,
  multiplicative: 10,
  unary: 11,
  quantity: 12,
  postfix: 13,
};

const UNITS = ['cycle', 'cycles', 'ns', 'us', 'ms', 's', 'Hz', 'kHz', 'MHz'];

// Newlines are insignificant; list items, fields and statements may be
// separated by optional commas.
const sepBy = (rule) => repeat(seq(rule, optional(',')));
const commaList = (rule) => optional(seq(rule, repeat(seq(',', rule)), optional(',')));

module.exports = grammar({
  name: 'spatial',

  extras: $ => [/\s/, $.comment],

  word: $ => $.identifier,

  supertypes: $ => [$._item, $._statement, $._expression],


  rules: {
    source_file: $ => sepBy($._item),

    _item: $ => choice(
      $.clock_declaration,
      $.const_declaration,
      $.pin_declaration,
      $.block_declaration,
      $.reg_declaration,
      $.event_declaration,
      $.flag_declaration,
      $.stream_declaration,
      $.function_declaration,
      $.test_declaration,
      $.always_declaration,
    ),

    // ── items ────────────────────────────────────────────────────────────

    clock_declaration: $ => seq('clock', field('value', $._expression)),

    const_declaration: $ => seq(
      'const',
      field('name', $.identifier),
      '=',
      field('value', $._expression),
    ),

    pin_declaration: $ => seq(
      'pin',
      field('name', $.identifier),
      optional(seq('@', field('pad', $._expression))),
      ':',
      field('mode', $.pin_mode),
      optional(seq('=', field('value', $._expression))),
    ),

    pin_mode: _ => choice('in', 'out', 'open_drain'),

    block_declaration: $ => seq(
      field('kind', choice('timer', 'shifter', 'counter', 'crc')),
      field('name', $.identifier),
      field('body', $.field_block),
    ),

    field_block: $ => seq('{', sepBy(choice($.field, $.flag)), '}'),

    field: $ => seq(field('name', $.identifier), ':', field('value', $._expression)),

    flag: $ => field('name', $.identifier),

    reg_declaration: $ => seq(
      'reg',
      field('name', $.identifier),
      optional(seq('init', field('init', $._expression))),
      optional(seq('on', field('on', $._expression))),
      '=',
      field('value', $._expression),
    ),

    event_declaration: $ => seq('event', $._name_list),

    flag_declaration: $ => seq('flag', $._name_list),

    _name_list: $ => prec.right(seq(
      field('name', $.identifier),
      repeat(seq(',', field('name', $.identifier))),
    )),

    stream_declaration: $ => seq(
      optional('export'),
      'stream',
      field('name', $.identifier),
      ':',
      field('source', $._expression),
      '->',
      field('sink', $._expression),
    ),

    function_declaration: $ => seq(
      optional('export'),
      'fn',
      field('name', $.identifier),
      field('parameters', $.parameters),
      optional(seq('->', field('return_type', $.type))),
      field('body', $.block),
    ),

    parameters: $ => seq('(', commaList($.parameter), ')'),

    parameter: $ => seq(field('name', $.identifier), ':', field('type', $.type)),

    type: $ => seq(
      field('name', alias($.identifier, $.type_identifier)),
      optional(seq('via', field('via', $.identifier))),
      optional(seq('in', field('range', $.range))),
    ),

    range: $ => seq(field('start', $._expression), '..=', field('end', $._expression)),

    test_declaration: $ => seq('test', field('name', $.string), field('body', $.block)),

    always_declaration: $ => seq(
      'always',
      field('name', $.string),
      '{',
      field('condition', $._expression),
      '}',
    ),

    // ── statements ───────────────────────────────────────────────────────

    block: $ => seq('{', sepBy($._statement), '}'),

    _statement: $ => choice(
      $.fire_statement,
      $.wait_statement,
      $.repeat_statement,
      $.for_statement,
      $.if_statement,
      $.reply_statement,
      $.fail_statement,
      $.expect_statement,
      $.attach_statement,
      $.wire_statement,
      $.set_statement,
      $.assignment_statement,
      $.expression_statement,
    ),

    fire_statement: $ => seq('fire', field('event', $.identifier)),

    wait_statement: $ => seq('wait', optional('next'), field('condition', $._expression)),

    repeat_statement: $ => seq('repeat', field('count', $._expression), field('body', $.block)),

    for_statement: $ => seq(
      'for',
      field('variable', $.identifier),
      'in',
      field('iterable', $._expression),
      field('body', $.block),
    ),

    if_statement: $ => seq(
      'if',
      field('condition', $._expression),
      field('consequence', $.block),
      optional(seq('else', field('alternative', choice($.block, $.if_statement)))),
    ),

    reply_statement: $ => seq('reply', field('value', $._expression)),

    fail_statement: $ => seq('fail', field('message', $.string)),

    expect_statement: $ => prec.right(seq(
      'expect',
      field('condition', $._expression),
      optional(seq('fails', field('message', $.string))),
    )),

    attach_statement: $ => prec(-1, seq(
      'attach',
      field('peer', $.call_expression),
      optional(seq('as', field('name', $.identifier))),
    )),

    wire_statement: $ => seq('wire', field('from', $._expression), '->', field('to', $._expression)),

    set_statement: $ => seq(
      field('object', $._expression),
      '.',
      'set',
      field('fields', $.field_block),
    ),

    assignment_statement: $ => seq(field('left', $._expression), '=', field('right', $._expression)),

    expression_statement: $ => prec(-1, $.call_expression),

    // ── expressions ──────────────────────────────────────────────────────

    _expression: $ => choice(
      $.identifier,
      $.number,
      $.quantity,
      $.string,
      $.list,
      $.parenthesized_expression,
      $.if_expression,
      $.unary_expression,
      $.binary_expression,
      $.member_expression,
      $.call_expression,
    ),

    binary_expression: $ => {
      const table = [
        [PREC.until, choice('until', 'at')],
        [PREC.implies, 'implies'],
        [PREC.or, '||'],
        [PREC.and, '&&'],
        [PREC.compare, choice('==', '!=', '<', '>', '<=', '>=')],
        [PREC.bit_or, '|'],
        [PREC.bit_xor, '^'],
        [PREC.bit_and, '&'],
        [PREC.additive, choice('<<', '>>', '+', '-')],
        [PREC.multiplicative, choice('*', '/')],
      ];
      return choice(...table.map(([p, op]) => prec.left(/** @type {number} */ (p), seq(
        field('left', $._expression),
        // @ts-ignore
        field('operator', op),
        field('right', $._expression),
      ))));
    },

    unary_expression: $ => prec(PREC.unary, seq(
      field('operator', choice('!', '-')),
      field('operand', $._expression),
    )),

    member_expression: $ => prec(PREC.postfix, seq(
      field('object', $._expression),
      '.',
      field('property', alias($.identifier, $.property_identifier)),
    )),

    call_expression: $ => prec(PREC.postfix, seq(
      field('function', $._expression),
      field('arguments', $.arguments),
    )),

    arguments: $ => seq('(', commaList(choice($._expression, $.named_argument)), ')'),

    named_argument: $ => seq(field('name', $.identifier), ':', field('value', $._expression)),

    list: $ => seq('[', commaList($._expression), ']'),

    parenthesized_expression: $ => seq('(', $._expression, ')'),

    if_expression: $ => seq(
      'if',
      field('condition', $._expression),
      '{',
      field('consequence', $._expression),
      '}',
      'else',
      '{',
      field('alternative', $._expression),
      '}',
    ),

    // ── tokens ───────────────────────────────────────────────────────────

    quantity: $ => prec(PREC.quantity, seq(field('value', $.number), field('unit', $.unit))),

    unit: _ => choice(...UNITS),

    number: _ => token(choice(
      /0x[0-9A-Fa-f_]+/,
      /0b[01_]+/,
      /[0-9][0-9_]*/,
    )),

    string: _ => token(seq('"', /[^"\n]*/, '"')),

    comment: _ => token(seq('//', /.*/)),

    identifier: _ => /[A-Za-z_][A-Za-z0-9_]*/,
  },
});
