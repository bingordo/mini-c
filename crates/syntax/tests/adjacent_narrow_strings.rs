//! Full Mini-C parser tests for C translation phase 6 string concatenation.
//! Literal tokens must contribute one decoded byte sequence and final NUL.
use mini_c_syntax::{
    ast::Expr,
    parser::Parser,
    stream::{ExternalItem, ItemSource},
};

fn initializer(source: &str) -> Expr {
    let mut parser = Parser::new(source);
    let Some(ExternalItem::Declaration(decl)) = parser.next_item().unwrap() else {
        panic!("expected a file-scope string initializer");
    };
    assert!(parser.next_item().unwrap().is_none());
    decl.init.expect("string initializer")
}

#[test]
fn adjacent_narrow_strings_are_one_array() {
    assert_eq!(
        initializer(r#"const char *message = "INFO:abi[" "ELF" "]";"#),
        Expr::StringLiteral(b"INFO:abi[ELF]\0".to_vec())
    );
    assert_eq!(
        initializer(r#"char text[] = "" "";"#),
        Expr::StringLiteral(vec![0])
    );
}

#[test]
fn comments_and_whitespace_separate_adjacent_tokens() {
    assert_eq!(
        initializer("const char *message = \"ab\" /* comment */\n \"cd\";"),
        Expr::StringLiteral(b"abcd\0".to_vec())
    );
}

#[test]
fn explicit_nul_and_escapes_are_preserved() {
    assert_eq!(
        initializer(r#"const char *message = "A\0" "B\n" "\x43";"#),
        Expr::StringLiteral(b"A\0B\nC\0".to_vec())
    );
}

#[test]
fn bad_second_fragment_is_not_silently_discarded() {
    let mut parser = Parser::new(r#"const char *message = "ok" "\q";"#);
    assert!(parser.next_item().is_err());
}
