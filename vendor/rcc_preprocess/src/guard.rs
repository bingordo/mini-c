//! Include-guard detection.
//!
//! Recognises the canonical `#ifndef X / #define X / ... / #endif`
//! idiom that wraps an entire header. When a file matches, subsequent
//! `#include`s of it can be elided without re-tokenising — Clang calls
//! this the *multiple-include optimization*. Only the *shape* of the
//! guard is validated here; body-level semantics (is `X` already
//! `#define`d? has it since been `#undef`ed?) are decided at the
//! inclusion site by the preprocessor.
//!
//! ### What counts as a guard
//!
//! The guard must cover the **entire** translation unit of the
//! header:
//!
//! - The first non-blank logical line is `#ifndef NAME`.
//! - The second non-blank logical line is `#define NAME ...`
//!   (replacement list is allowed but irrelevant).
//! - The last non-blank logical line is `#endif`.
//!
//! Any stray token before `#ifndef`, between `#endif` and EOF, or a
//! mismatched guard name defeats the optimisation — the header is
//! then processed normally on every inclusion.
//!
//! Blank lines (newlines with no intervening tokens) are ignored:
//! `rcc_lexer::tokenize` collapses horizontal whitespace and comments
//! in its default mode, so "blank" here means a token run between
//! two consecutive `Newline`s is empty.

use rcc_lexer::{PpToken, PpTokenKind, Punct};
use rcc_span::{Interner, Symbol};

/// Return the guard macro name if `tokens` (a header's full pp-token
/// stream, as produced by `rcc_lexer::tokenize`) matches the canonical
/// `#ifndef / #define / #endif` wrapper. `src` is the source text the
/// tokens were lexed from; `interner` receives the guard identifier.
///
/// See the [module docs](self) for the exact acceptance rules.
pub fn detect_guard(tokens: &[PpToken], src: &str, interner: &mut Interner) -> Option<Symbol> {
    detect_guard_iter(tokens.iter().cloned(), src, interner)
}

/// Detect the same canonical guard shape directly from a token iterator.
///
/// Unlike the legacy slice API this keeps only the active logical line and
/// the guard identifier, not a token vector for the entire header. All
/// intermediate body lines are discarded after their shape is observed.
/// The full stream is still consumed even when the prefix is not a guard:
/// the caller may be enforcing a cumulative lexical-work budget.
pub fn detect_guard_iter<I>(
    tokens: I,
    src: &str,
    interner: &mut Interner,
) -> Option<Symbol>
where
    I: Iterator<Item = PpToken>,
{
    let mut nonblank = 0usize;
    let mut guard_text: Option<String> = None;
    let mut define_matches = false;
    let mut final_endif = false;

    for line in crate::line_stream::LineStream::new(tokens) {
        if line.is_empty() {
            continue;
        }
        nonblank = nonblank.saturating_add(1);
        if nonblank == 1 {
            // A guard must open the file's first non-blank line.
            if line.len() == 3
                && is_hash(&line[0])
                && ident_is(&line[1], src, "ifndef")
                && line[2].kind == PpTokenKind::Ident
            {
                guard_text = Some(token_text(&line[2], src).to_owned());
            }
        } else if nonblank == 2 {
            if let Some(guard) = guard_text.as_deref() {
                define_matches = line.len() >= 3
                    && is_hash(&line[0])
                    && ident_is(&line[1], src, "define")
                    && line[2].kind == PpTokenKind::Ident
                    && token_text(&line[2], src) == guard;
            }
        }
        // Every non-blank line becomes the candidate last line.
        final_endif = line.len() == 2
            && is_hash(&line[0])
            && ident_is(&line[1], src, "endif");
    }
    (nonblank >= 3 && define_matches && final_endif)
        .then(|| interner.intern(guard_text.as_deref().expect("verified guard name")))
}

fn is_hash(tok: &PpToken) -> bool {
    // A `#` only introduces a directive when it starts the logical
    // line (C99 §6.10 p1). Mid-line `#`s are rejected so that source
    // like `x #ifndef G` cannot be mistaken for a guard.
    tok.kind == PpTokenKind::Punct(Punct::Hash) && tok.at_line_start
}

fn ident_is(tok: &PpToken, src: &str, want: &str) -> bool {
    tok.kind == PpTokenKind::Ident && token_text(tok, src) == want
}

fn token_text<'a>(tok: &PpToken, src: &'a str) -> &'a str {
    &src[tok.span.lo.0 as usize..tok.span.hi.0 as usize]
}

#[cfg(test)]
mod tests {
    use super::*;
    use rcc_lexer::tokenize;
    use rcc_span::FileId;

    fn tokens_of(src: &str) -> Vec<PpToken> {
        tokenize(FileId(0), src).collect()
    }

    /// `ok.h` — the canonical header shape every C project uses. Must
    /// be detected and produce the correct guard symbol.
    #[test]
    fn incremental_guard_detector_matches_legacy_slice_on_header_shapes() {
        for source in [
            "", "#ifndef G\n#define G\nint x;\n#endif\n",
            "#ifndef G\n#define G 42\n#endif\n",
            "#ifndef G\n#define G\n#endif\nint tail;\n",
            "#pragma once\n#ifndef G\n#define G\n#endif\n",
            "\n\n#ifndef G\n\n#define G\nint x;\n\n#endif\n",
            "#ifndef G\n#define Q\n#endif\n",
            "#ifndef G\n#define G\n#ifdef X\n#endif\n#endif\n",
            "#ifndef G\n#define G\n#endif BAD\n",
            "int x;\n#ifndef G\n#define G\n#endif\n",
        ] {
            let tokens = tokens_of(source);
            let mut a = Interner::new();
            let mut b = Interner::new();
            let from_slice = detect_guard(&tokens, source, &mut a)
                .map(|symbol| a.get(symbol).to_owned());
            let from_stream = detect_guard_iter(
                rcc_lexer::tokenize(FileId(0), source), source, &mut b,
            ).map(|symbol| b.get(symbol).to_owned());
            assert_eq!(from_slice, from_stream, "{source:?}");
        }
    }

    #[test]
    fn ok_h_canonical_guard() {
        let src = "#ifndef OK_H\n#define OK_H\nint ok;\n#endif\n";
        let toks = tokens_of(src);
        let mut interner = Interner::new();
        let sym = detect_guard(&toks, src, &mut interner);
        assert_eq!(sym, Some(interner.intern("OK_H")));
    }

    /// `bad.h` — stray token before `#ifndef` disqualifies the guard.
    /// The file must still be considered includable (the caller falls
    /// back to full processing), so `detect_guard` returns `None`.
    #[test]
    fn bad_h_stray_token_before_ifndef() {
        let src = "int stray;\n#ifndef BAD_H\n#define BAD_H\n#endif\n";
        let toks = tokens_of(src);
        let mut interner = Interner::new();
        assert!(detect_guard(&toks, src, &mut interner).is_none());
    }

    #[test]
    fn guard_with_define_replacement_list_still_detected() {
        // `#define G 1` is a perfectly valid guard — the replacement
        // list is irrelevant to the skip optimisation.
        let src = "#ifndef G\n#define G 1\n#endif\n";
        let toks = tokens_of(src);
        let mut interner = Interner::new();
        assert_eq!(detect_guard(&toks, src, &mut interner), Some(interner.intern("G")));
    }

    #[test]
    fn blank_lines_around_directives_are_ignored() {
        let src = "\n\n#ifndef G\n\n#define G\n\nint x;\n\n#endif\n\n";
        let toks = tokens_of(src);
        let mut interner = Interner::new();
        assert_eq!(detect_guard(&toks, src, &mut interner), Some(interner.intern("G")));
    }

    #[test]
    fn mismatched_guard_names_rejected() {
        // `#ifndef A` followed by `#define B` is not a guard — neither
        // clang nor gcc treat it as one.
        let src = "#ifndef A\n#define B\n#endif\n";
        let toks = tokens_of(src);
        let mut interner = Interner::new();
        assert!(detect_guard(&toks, src, &mut interner).is_none());
    }

    #[test]
    fn stray_token_after_endif_rejected() {
        // Anything on the far side of `#endif` breaks the
        // "covers the entire file" requirement.
        let src = "#ifndef G\n#define G\n#endif\nint trail;\n";
        let toks = tokens_of(src);
        let mut interner = Interner::new();
        assert!(detect_guard(&toks, src, &mut interner).is_none());
    }

    #[test]
    fn missing_define_rejected() {
        let src = "#ifndef G\nint x;\n#endif\n";
        let toks = tokens_of(src);
        let mut interner = Interner::new();
        assert!(detect_guard(&toks, src, &mut interner).is_none());
    }

    #[test]
    fn missing_endif_rejected() {
        let src = "#ifndef G\n#define G\nint x;\n";
        let toks = tokens_of(src);
        let mut interner = Interner::new();
        assert!(detect_guard(&toks, src, &mut interner).is_none());
    }

    #[test]
    fn empty_file_rejected() {
        let toks = tokens_of("");
        let mut interner = Interner::new();
        assert!(detect_guard(&toks, "", &mut interner).is_none());
    }

    #[test]
    fn hash_not_at_line_start_rejected() {
        // `x #ifndef G` keeps the `#` as a punctuator but with
        // `at_line_start = false`; a stray leading token also
        // disqualifies the guard independently.
        let src = "x #ifndef G\n#define G\n#endif\n";
        let toks = tokens_of(src);
        let mut interner = Interner::new();
        assert!(detect_guard(&toks, src, &mut interner).is_none());
    }

    #[test]
    fn endif_with_trailing_token_rejected() {
        // `#endif G` (extra ident on the endif line) is malformed by
        // C99 §6.10.1 and also forbidden under our guard shape.
        let src = "#ifndef G\n#define G\n#endif G\n";
        let toks = tokens_of(src);
        let mut interner = Interner::new();
        assert!(detect_guard(&toks, src, &mut interner).is_none());
    }
}
