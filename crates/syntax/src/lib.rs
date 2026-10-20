//! Parser entry points; downstream crates depend on the AST, not LALRPOP.
pub mod ast;
pub mod diag;
pub mod source;
pub mod scopes;
lalrpop_util::lalrpop_mod!(pub grammar);

pub fn parse_expression(source: &str) -> Result<ast::Expr, String> {
    grammar::ExpressionParser::new().parse(source).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ast::{BinOp, Expr};
    #[test]
    fn precedence() {
        let result = parse_expression("1 + 2 * 3").unwrap();
        assert_eq!(result, Expr::Binary(BinOp::Add, Box::new(Expr::Int(1)), Box::new(Expr::Binary(BinOp::Mul, Box::new(Expr::Int(2)), Box::new(Expr::Int(3))))));
    }
    #[test]
    fn overflowing_integer_is_error_not_panic() {
        assert!(parse_expression("18446744073709551616").is_err());
    }
    #[test]
    fn names_and_parentheses() {
        assert!(parse_expression("(alpha + 4) / 2").is_ok());
        assert!(parse_expression("1 +").is_err());
    }
}
