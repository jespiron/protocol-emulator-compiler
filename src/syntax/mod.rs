//! Source text to syntax tree.
pub(crate) mod ast;
mod lexer;
mod parser;

use crate::diagnostic::Result;

pub(crate) fn parse(source: &str) -> Result<Vec<ast::Item>> {
    parser::Parser::new(lexer::tokenize(source)?).file()
}
