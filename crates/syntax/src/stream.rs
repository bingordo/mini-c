//! Incremental top-level item delivery.
//!
//! This interface deliberately does not retain a translation-unit AST. A parser
//! can hand off a completed function and immediately reclaim its syntax tree.
use crate::ast::{Decl, Function};

/// One completed external declaration or function definition.
#[derive(Debug, Clone, PartialEq)]
pub enum ExternalItem {
    Declaration(Decl),
    Function(Function),
}

/// Consumer of parsed top-level items. Implementations may lower and emit each
/// function before requesting the next item. Errors stop processing immediately.
pub trait ItemSink {
    type Error;

    fn declaration(&mut self, declaration: Decl) -> Result<(), Self::Error>;
    fn function(&mut self, function: Function) -> Result<(), Self::Error>;
}

/// A source of one parsed item at a time. The parser owns any lookahead and
/// must not retain earlier function bodies after returning them.
pub trait ItemSource {
    type Error;

    fn next_item(&mut self) -> Result<Option<ExternalItem>, Self::Error>;
}

/// Drive parsing and consumption without accumulating function ASTs.
pub fn consume<S, C>(source: &mut S, sink: &mut C) -> Result<(), StreamError<S::Error, C::Error>>
where
    S: ItemSource,
    C: ItemSink,
{
    while let Some(item) = source.next_item().map_err(StreamError::Source)? {
        match item {
            ExternalItem::Declaration(decl) => sink.declaration(decl).map_err(StreamError::Sink)?,
            ExternalItem::Function(function) => sink.function(function).map_err(StreamError::Sink)?,
        }
    }
    Ok(())
}

#[derive(Debug, PartialEq, Eq)]
pub enum StreamError<SE, CE> {
    Source(SE),
    Sink(CE),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::{Span, Stmt, Type};
    use std::collections::VecDeque;

    struct Source(VecDeque<ExternalItem>);
    impl ItemSource for Source {
        type Error = ();
        fn next_item(&mut self) -> Result<Option<ExternalItem>, Self::Error> {
            Ok(self.0.pop_front())
        }
    }

    #[derive(Default)]
    struct Sink { functions: usize, declarations: usize }
    impl ItemSink for Sink {
        type Error = ();
        fn declaration(&mut self, _: Decl) -> Result<(), Self::Error> {
            self.declarations += 1;
            Ok(())
        }
        fn function(&mut self, _: Function) -> Result<(), Self::Error> {
            self.functions += 1;
            Ok(())
        }
    }

    #[test]
    fn delivers_many_functions_without_collecting() {
        let functions = (0..1000).map(|i| ExternalItem::Function(Function {
            name: format!("f{i}"),
            result: Type::Int,
            params: vec![],
            body: Stmt::Block(vec![]),
            span: Span::default(),
        }));
        let mut source = Source(functions.collect());
        let mut sink = Sink::default();
        consume(&mut source, &mut sink).unwrap();
        assert_eq!(sink.functions, 1000);
        assert_eq!(sink.declarations, 0);
        assert!(source.0.is_empty());
    }

    #[test]
    fn sink_failure_stops_before_reading_more_items() {
        struct Failing;
        impl ItemSink for Failing {
            type Error = &'static str;
            fn declaration(&mut self, _: Decl) -> Result<(), Self::Error> { Ok(()) }
            fn function(&mut self, _: Function) -> Result<(), Self::Error> { Err("stop") }
        }
        let function = ExternalItem::Function(Function {
            name: "f".into(), result: Type::Int, params: vec![],
            body: Stmt::Block(vec![]), span: Span::default(),
        });
        let mut source = Source(VecDeque::from([function.clone(), function]));
        assert_eq!(consume(&mut source, &mut Failing), Err(StreamError::Sink("stop")));
        assert_eq!(source.0.len(), 1);
    }
}
