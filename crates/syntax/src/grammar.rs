// auto-generated: "lalrpop 0.23.1"
// sha3: ee435cb01508cb4457a54a56b70d3346723451ec54404f6c80565f45107a4646
use crate::expr_lexer::ExprToken;
use crate::ast::{Expr, BinOp, UnOp};
#[allow(unused_extern_crates)]
extern crate lalrpop_util as __lalrpop_util;
#[allow(unused_imports)]
use self::__lalrpop_util::state_machine as __state_machine;
#[allow(unused_extern_crates)]
extern crate alloc;

#[rustfmt::skip]
#[allow(explicit_outlives_requirements, non_snake_case, non_camel_case_types, unused_mut, unused_variables, unused_imports, unused_parens, clippy::needless_lifetimes, clippy::type_complexity, clippy::needless_return, clippy::too_many_arguments, clippy::match_single_binding, clippy::clone_on_copy, clippy::unit_arg)]
mod __parse__Expression {

    use crate::expr_lexer::ExprToken;
    use crate::ast::{Expr, BinOp, UnOp};
    #[allow(unused_extern_crates)]
    extern crate lalrpop_util as __lalrpop_util;
    #[allow(unused_imports)]
    use self::__lalrpop_util::state_machine as __state_machine;
    #[allow(unused_extern_crates)]
    extern crate alloc;
    use super::__ToTriple;
    #[allow(dead_code)]
    pub(crate) enum __Symbol<'input>
     {
        Variant0(&'input str),
        Variant1(ExprToken<'input>),
        Variant2(Expr),
        Variant3(alloc::vec::Vec<Expr>),
        Variant4(Vec<Expr>),
    }
    const __ACTION: &[i8] = &[
        // State 0
        49, 47, 48, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 6, 7, 5, 0, 0, 2, 3, 0, 0, 8, 0, 0, 4, 0, 0,
        // State 1
        49, 47, 48, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 6, 7, 5, 0, 0, 2, 3, 0, 0, 8, 0, 0, 4, 0, 0,
        // State 2
        49, 47, 48, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 6, 7, 5, 0, 0, 2, 3, 0, 0, 8, 0, 0, 4, 0, 0,
        // State 3
        49, 47, 48, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 6, 7, 5, 0, 0, 2, 3, 0, 0, 8, 0, 0, 4, 0, 0,
        // State 4
        49, 47, 48, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 6, 7, 5, 0, 0, 2, 3, 0, 0, 8, 0, 0, 4, 0, 0,
        // State 5
        49, 47, 48, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 6, 7, 5, 0, 0, 2, 3, 0, 0, 8, 0, 0, 4, 0, 0,
        // State 6
        49, 47, 48, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 6, 7, 5, 0, 0, 2, 3, 0, 0, 8, 0, 0, 4, 0, 0,
        // State 7
        49, 47, 48, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 6, 7, 5, 0, 0, 2, 3, 0, 0, 8, 0, 0, 4, 0, 0,
        // State 8
        49, 47, 48, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 6, 7, 5, 0, 0, 2, 3, 0, 0, 8, 0, 0, 4, 0, 0,
        // State 9
        49, 47, 48, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 6, 7, 5, 0, 0, 2, 3, 0, 0, 8, 0, 0, 4, 0, 0,
        // State 10
        49, 47, 48, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 6, 7, 5, 0, 0, 2, 3, 0, 0, 8, 0, 0, 4, 0, 0,
        // State 11
        49, 47, 48, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 6, 7, 5, 0, 0, 2, 3, 0, 0, 8, 0, 0, 4, 0, 0,
        // State 12
        49, 47, 48, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 6, 7, 5, 0, 0, 2, 3, 0, 0, 8, 0, 0, 4, 0, 0,
        // State 13
        49, 47, 48, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 6, 7, 5, 0, 0, 2, 3, 0, 0, 8, 0, 0, 4, 0, 0,
        // State 14
        49, 47, 48, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 6, 7, 5, 0, 0, 2, 3, 0, 0, 8, 0, 0, 4, 0, 0,
        // State 15
        49, 47, 48, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 6, 7, 5, 0, 0, 2, 3, 0, 0, 8, 0, 0, 4, 0, 0,
        // State 16
        49, 47, 48, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 6, 7, 5, 0, 0, 2, 3, 0, 0, 8, 0, 0, 4, 0, 0,
        // State 17
        49, 47, 48, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 6, 7, 5, 0, 0, 2, 3, 0, 0, 8, 0, 0, 4, 0, 0,
        // State 18
        49, 47, 48, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 6, 7, 5, 0, 0, 2, 3, 0, 0, 8, 0, 0, 4, 0, 0,
        // State 19
        49, 47, 48, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 6, 7, 5, 0, 0, 2, 3, 0, 0, 8, 0, 0, 4, 0, 0,
        // State 20
        49, 47, 48, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 6, 7, 5, 0, 0, 2, 3, 0, 0, 8, 0, 0, 4, 0, 0,
        // State 21
        49, 47, 48, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 6, 7, 5, 0, 0, 2, 3, 0, 0, 8, 0, 0, 4, -17, 0,
        // State 22
        49, 47, 48, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 6, 7, 5, 0, 0, 2, 3, 0, 0, 8, 0, 0, 4, 0, 0,
        // State 23
        49, 47, 48, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 6, 7, 5, 0, 0, 2, 3, 0, 0, 8, 0, 0, 4, 0, 0,
        // State 24
        49, 47, 48, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 6, 7, 5, 0, 0, 2, 3, 0, 0, 8, 0, 0, 4, 0, 0,
        // State 25
        49, 47, 48, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 6, 7, 5, 0, 0, 2, 3, 0, 0, 8, 0, 0, 4, 0, 0,
        // State 26
        49, 47, 48, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 6, 7, 5, 0, 0, 2, 3, 0, 0, 8, 0, 0, 4, 0, 0,
        // State 27
        49, 47, 48, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 6, 7, 5, 0, 0, 2, 3, 0, 0, 8, 0, 0, 4, 0, 0,
        // State 28
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, -18, 30,
        // State 29
        49, 47, 48, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 6, 7, 5, 0, 0, 2, 3, 0, 0, 8, 0, 0, 4, 0, 0,
        // State 30
        49, 47, 48, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 6, 7, 5, 0, 0, 2, 3, 0, 0, 8, 0, 0, 4, 0, 0,
        // State 31
        0, 0, 0, 0, -45, -45, -45, -45, -45, -45, -45, -45, -45, 9, 10, 0, 0, 0, 0, -45, -45, -45, 0, -45, -45, 0, -45, -45,
        // State 32
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        // State 33
        0, 0, 0, 0, -16, -16, -16, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 11, -16, -16, 0, 0, 0, 0, -16, -16,
        // State 34
        0, 0, 0, 0, -25, -25, -25, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 12, 0, 0, 0, 0, 0, -25, -25,
        // State 35
        0, 0, 0, 0, -14, -14, -14, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, -14, 13, 0, 0, 0, 0, -14, -14,
        // State 36
        0, 0, 0, 0, -12, -12, -12, 15, 14, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, -12, -12, -12, 0, 0, 0, 0, -12, -12,
        // State 37
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        // State 38
        0, 0, 0, 0, -27, -27, 16, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, -27, -27,
        // State 39
        0, 0, 0, 0, 17, 18, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, -10, -10,
        // State 40
        0, 0, 0, 0, -8, -8, -8, -8, -8, -8, -8, -8, -8, -8, -8, 20, 21, 19, 0, -8, -8, -8, 0, -8, -8, 0, -8, -8,
        // State 41
        0, 0, 0, 0, -52, -52, -52, -52, -52, -52, -52, -52, -52, -52, -52, -52, -52, -52, 0, -52, -52, -52, 0, -52, -52, 22, -52, -52,
        // State 42
        0, 0, 0, 0, -33, -33, -33, -33, -33, -33, -33, -33, -33, -33, -33, -33, -33, -33, 0, -33, -33, -33, 0, -33, -33, -33, -33, -33,
        // State 43
        0, 0, 0, 0, -22, -22, -22, -22, -22, 24, 26, 23, 25, 0, 0, 0, 0, 0, 0, -22, -22, -22, 0, 0, 0, 0, -22, -22,
        // State 44
        0, 0, 0, 0, -42, -42, -42, -42, -42, -42, -42, -42, -42, 0, 0, 0, 0, 0, 0, -42, -42, -42, 0, 27, 28, 0, -42, -42,
        // State 45
        0, 0, 0, 0, -31, -31, -31, -31, -31, -31, -31, -31, -31, -31, -31, -31, -31, -31, 0, -31, -31, -31, 0, -31, -31, 0, -31, -31,
        // State 46
        0, 0, 0, 0, -35, -35, -35, -35, -35, -35, -35, -35, -35, -35, -35, -35, -35, -35, 0, -35, -35, -35, 0, -35, -35, -35, -35, -35,
        // State 47
        0, 0, 0, 0, -36, -36, -36, -36, -36, -36, -36, -36, -36, -36, -36, -36, -36, -36, 0, -36, -36, -36, 0, -36, -36, -36, -36, -36,
        // State 48
        0, 0, 0, 0, -34, -34, -34, -34, -34, -34, -34, -34, -34, -34, -34, -34, -34, -34, 0, -34, -34, -34, 0, -34, -34, -34, -34, -34,
        // State 49
        0, 0, 0, 0, -48, -48, -48, -48, -48, -48, -48, -48, -48, -48, -48, -48, -48, -48, 0, -48, -48, -48, 0, -48, -48, 0, -48, -48,
        // State 50
        0, 0, 0, 0, -50, -50, -50, -50, -50, -50, -50, -50, -50, -50, -50, -50, -50, -50, 0, -50, -50, -50, 0, -50, -50, 0, -50, -50,
        // State 51
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 77, 0,
        // State 52
        0, 0, 0, 0, -51, -51, -51, -51, -51, -51, -51, -51, -51, -51, -51, -51, -51, -51, 0, -51, -51, -51, 0, -51, -51, 0, -51, -51,
        // State 53
        0, 0, 0, 0, -46, -46, -46, -46, -46, -46, -46, -46, -46, -46, -46, -46, -46, -46, 0, -46, -46, -46, 0, -46, -46, 0, -46, -46,
        // State 54
        0, 0, 0, 0, -47, -47, -47, -47, -47, -47, -47, -47, -47, -47, -47, -47, -47, -47, 0, -47, -47, -47, 0, -47, -47, 0, -47, -47,
        // State 55
        0, 0, 0, 0, -49, -49, -49, -49, -49, -49, -49, -49, -49, -49, -49, -49, -49, -49, 0, -49, -49, -49, 0, -49, -49, 0, -49, -49,
        // State 56
        0, 0, 0, 0, -6, -6, -6, -6, -6, -6, -6, -6, -6, -6, -6, 20, 21, 19, 0, -6, -6, -6, 0, -6, -6, 0, -6, -6,
        // State 57
        0, 0, 0, 0, -7, -7, -7, -7, -7, -7, -7, -7, -7, -7, -7, 20, 21, 19, 0, -7, -7, -7, 0, -7, -7, 0, -7, -7,
        // State 58
        0, 0, 0, 0, -11, -11, -11, 15, 14, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, -11, -11, -11, 0, 0, 0, 0, -11, -11,
        // State 59
        0, 0, 0, 0, -13, -13, -13, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, -13, 13, 0, 0, 0, 0, -13, -13,
        // State 60
        0, 0, 0, 0, -15, -15, -15, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 11, -15, -15, 0, 0, 0, 0, -15, -15,
        // State 61
        0, 0, 0, 0, -21, -21, -21, -21, -21, 24, 26, 23, 25, 0, 0, 0, 0, 0, 0, -21, -21, -21, 0, 0, 0, 0, -21, -21,
        // State 62
        0, 0, 0, 0, -20, -20, -20, -20, -20, 24, 26, 23, 25, 0, 0, 0, 0, 0, 0, -20, -20, -20, 0, 0, 0, 0, -20, -20,
        // State 63
        0, 0, 0, 0, -24, -24, -24, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 12, 0, 0, 0, 0, 0, -24, -24,
        // State 64
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, -9, -9,
        // State 65
        0, 0, 0, 0, -26, -26, 16, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, -26, -26,
        // State 66
        0, 0, 0, 0, -30, -30, -30, -30, -30, -30, -30, -30, -30, -30, -30, -30, -30, -30, 0, -30, -30, -30, 0, -30, -30, 0, -30, -30,
        // State 67
        0, 0, 0, 0, -28, -28, -28, -28, -28, -28, -28, -28, -28, -28, -28, -28, -28, -28, 0, -28, -28, -28, 0, -28, -28, 0, -28, -28,
        // State 68
        0, 0, 0, 0, -29, -29, -29, -29, -29, -29, -29, -29, -29, -29, -29, -29, -29, -29, 0, -29, -29, -29, 0, -29, -29, 0, -29, -29,
        // State 69
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 79, 0,
        // State 70
        0, 0, 0, 0, -38, -38, -38, -38, -38, -38, -38, -38, -38, 0, 0, 0, 0, 0, 0, -38, -38, -38, 0, 27, 28, 0, -38, -38,
        // State 71
        0, 0, 0, 0, -40, -40, -40, -40, -40, -40, -40, -40, -40, 0, 0, 0, 0, 0, 0, -40, -40, -40, 0, 27, 28, 0, -40, -40,
        // State 72
        0, 0, 0, 0, -39, -39, -39, -39, -39, -39, -39, -39, -39, 0, 0, 0, 0, 0, 0, -39, -39, -39, 0, 27, 28, 0, -39, -39,
        // State 73
        0, 0, 0, 0, -41, -41, -41, -41, -41, -41, -41, -41, -41, 0, 0, 0, 0, 0, 0, -41, -41, -41, 0, 27, 28, 0, -41, -41,
        // State 74
        0, 0, 0, 0, -43, -43, -43, -43, -43, -43, -43, -43, -43, 9, 10, 0, 0, 0, 0, -43, -43, -43, 0, -43, -43, 0, -43, -43,
        // State 75
        0, 0, 0, 0, -44, -44, -44, -44, -44, -44, -44, -44, -44, 9, 10, 0, 0, 0, 0, -44, -44, -44, 0, -44, -44, 0, -44, -44,
        // State 76
        0, 0, 0, 0, -37, -37, -37, -37, -37, -37, -37, -37, -37, -37, -37, -37, -37, -37, 0, -37, -37, -37, 0, -37, -37, -37, -37, -37,
        // State 77
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, -19, 31,
        // State 78
        0, 0, 0, 0, -32, -32, -32, -32, -32, -32, -32, -32, -32, -32, -32, -32, -32, -32, 0, -32, -32, -32, 0, -32, -32, -32, -32, -32,
        // State 79
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, -4, -4,
        // State 80
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, -5, -5,
    ];
    fn __action(state: i8, integer: usize) -> i8 {
        __ACTION[(state as usize) * 28 + integer]
    }
    const __EOF_ACTION: &[i8] = &[
        // State 0
        0,
        // State 1
        0,
        // State 2
        0,
        // State 3
        0,
        // State 4
        0,
        // State 5
        0,
        // State 6
        0,
        // State 7
        0,
        // State 8
        0,
        // State 9
        0,
        // State 10
        0,
        // State 11
        0,
        // State 12
        0,
        // State 13
        0,
        // State 14
        0,
        // State 15
        0,
        // State 16
        0,
        // State 17
        0,
        // State 18
        0,
        // State 19
        0,
        // State 20
        0,
        // State 21
        0,
        // State 22
        0,
        // State 23
        0,
        // State 24
        0,
        // State 25
        0,
        // State 26
        0,
        // State 27
        0,
        // State 28
        0,
        // State 29
        0,
        // State 30
        0,
        // State 31
        -45,
        // State 32
        -23,
        // State 33
        -16,
        // State 34
        -25,
        // State 35
        -14,
        // State 36
        -12,
        // State 37
        -53,
        // State 38
        -27,
        // State 39
        -10,
        // State 40
        -8,
        // State 41
        -52,
        // State 42
        -33,
        // State 43
        -22,
        // State 44
        -42,
        // State 45
        -31,
        // State 46
        -35,
        // State 47
        -36,
        // State 48
        -34,
        // State 49
        -48,
        // State 50
        -50,
        // State 51
        0,
        // State 52
        -51,
        // State 53
        -46,
        // State 54
        -47,
        // State 55
        -49,
        // State 56
        -6,
        // State 57
        -7,
        // State 58
        -11,
        // State 59
        -13,
        // State 60
        -15,
        // State 61
        -21,
        // State 62
        -20,
        // State 63
        -24,
        // State 64
        -9,
        // State 65
        -26,
        // State 66
        -30,
        // State 67
        -28,
        // State 68
        -29,
        // State 69
        0,
        // State 70
        -38,
        // State 71
        -40,
        // State 72
        -39,
        // State 73
        -41,
        // State 74
        -43,
        // State 75
        -44,
        // State 76
        -37,
        // State 77
        0,
        // State 78
        -32,
        // State 79
        0,
        // State 80
        0,
    ];
    fn __goto(state: i8, nt: usize) -> i8 {
        match nt {
            2 => 77,
            3 => match state {
                26 => 74,
                27 => 75,
                _ => 31,
            },
            4 => match state {
                0 => 32,
                3 => 51,
                16 => 64,
                29 => 79,
                30 => 80,
                _ => 28,
            },
            5 => match state {
                12 => 60,
                _ => 33,
            },
            6 => match state {
                15 => 63,
                _ => 34,
            },
            7 => match state {
                11 => 59,
                _ => 35,
            },
            8 => 69,
            9 => match state {
                10 => 58,
                _ => 36,
            },
            10 => 37,
            11 => match state {
                17 => 65,
                _ => 38,
            },
            12 => 39,
            13 => match state {
                8 => 56,
                9 => 57,
                _ => 40,
            },
            14 => 41,
            15 => 42,
            16 => match state {
                13 => 61,
                14 => 62,
                _ => 43,
            },
            17 => match state {
                22 => 70,
                23 => 71,
                24 => 72,
                25 => 73,
                _ => 44,
            },
            18 => match state {
                1 => 49,
                2 => 50,
                4 => 52,
                5 => 53,
                6 => 54,
                7 => 55,
                18 => 66,
                19 => 67,
                20 => 68,
                _ => 45,
            },
            _ => 0,
        }
    }
    #[allow(clippy::needless_raw_string_hashes)]
    const __TERMINAL: &[&str] = &[
        r###""number""###,
        r###""character""###,
        r###""ident""###,
        r###""invalid""###,
        r###""=""###,
        r###""||""###,
        r###""&&""###,
        r###""==""###,
        r###""!=""###,
        r###""<=""###,
        r###"">=""###,
        r###""<""###,
        r###"">""###,
        r###""+""###,
        r###""-""###,
        r###""*""###,
        r###""/""###,
        r###""%""###,
        r###""!""###,
        r###""&""###,
        r###""|""###,
        r###""^""###,
        r###""~""###,
        r###""<<""###,
        r###"">>""###,
        r###""(""###,
        r###"")""###,
        r###"",""###,
    ];
    fn __expected_tokens(__state: i8) -> alloc::vec::Vec<alloc::string::String> {
        __TERMINAL.iter().enumerate().filter_map(|(index, terminal)| {
            let next_state = __action(__state, index);
            if next_state == 0 {
                None
            } else {
                Some(alloc::string::ToString::to_string(terminal))
            }
        }).collect()
    }
    fn __expected_tokens_from_states<
        'input,
    >(
        __states: &[i8],
        _: core::marker::PhantomData<(&'input ())>,
    ) -> alloc::vec::Vec<alloc::string::String>
    {
        __TERMINAL.iter().enumerate().filter_map(|(index, terminal)| {
            if __accepts(None, __states, Some(index), core::marker::PhantomData::<(&())>) {
                Some(alloc::string::ToString::to_string(terminal))
            } else {
                None
            }
        }).collect()
    }
    struct __StateMachine<'input>
    where 
    {
        __phantom: core::marker::PhantomData<(&'input ())>,
    }
    impl<'input> __state_machine::ParserDefinition for __StateMachine<'input>
    where 
    {
        type Location = usize;
        type Error = &'static str;
        type Token = ExprToken<'input>;
        type TokenIndex = usize;
        type Symbol = __Symbol<'input>;
        type Success = Expr;
        type StateIndex = i8;
        type Action = i8;
        type ReduceIndex = i8;
        type NonterminalIndex = usize;

        #[inline]
        fn start_location(&self) -> Self::Location {
              Default::default()
        }

        #[inline]
        fn start_state(&self) -> Self::StateIndex {
              0
        }

        #[inline]
        fn token_to_index(&self, token: &Self::Token) -> Option<usize> {
            __token_to_integer(token, core::marker::PhantomData::<(&())>)
        }

        #[inline]
        fn action(&self, state: i8, integer: usize) -> i8 {
            __action(state, integer)
        }

        #[inline]
        fn error_action(&self, state: i8) -> i8 {
            __action(state, 28 - 1)
        }

        #[inline]
        fn eof_action(&self, state: i8) -> i8 {
            __EOF_ACTION[state as usize]
        }

        #[inline]
        fn goto(&self, state: i8, nt: usize) -> i8 {
            __goto(state, nt)
        }

        fn token_to_symbol(&self, token_index: usize, token: Self::Token) -> Self::Symbol {
            __token_to_symbol(token_index, token, core::marker::PhantomData::<(&())>)
        }

        fn expected_tokens(&self, state: i8) -> alloc::vec::Vec<alloc::string::String> {
            __expected_tokens(state)
        }

        fn expected_tokens_from_states(&self, states: &[i8]) -> alloc::vec::Vec<alloc::string::String> {
            __expected_tokens_from_states(states, core::marker::PhantomData::<(&())>)
        }

        #[inline]
        fn uses_error_recovery(&self) -> bool {
            false
        }

        #[inline]
        fn error_recovery_symbol(
            &self,
            recovery: __state_machine::ErrorRecovery<Self>,
        ) -> Self::Symbol {
            panic!("error recovery not enabled for this grammar")
        }

        fn reduce(
            &mut self,
            action: i8,
            start_location: Option<&Self::Location>,
            states: &mut alloc::vec::Vec<i8>,
            symbols: &mut alloc::vec::Vec<__state_machine::SymbolTriple<Self>>,
        ) -> Option<__state_machine::ParseResult<Self>> {
            __reduce(
                action,
                start_location,
                states,
                symbols,
                core::marker::PhantomData::<(&())>,
            )
        }

        fn simulate_reduce(&self, action: i8) -> __state_machine::SimulatedReduce<Self> {
            __simulate_reduce(action, core::marker::PhantomData::<(&())>)
        }
    }
    fn __token_to_integer<
        'input,
    >(
        __token: &ExprToken<'input>,
        _: core::marker::PhantomData<(&'input ())>,
    ) -> Option<usize>
    {
        #[warn(unused_variables)]
        match __token {
            ExprToken::Number(_) if true => Some(0),
            ExprToken::Character(_) if true => Some(1),
            ExprToken::Ident(_) if true => Some(2),
            ExprToken::Invalid(_) if true => Some(3),
            ExprToken::Assign if true => Some(4),
            ExprToken::OrOr if true => Some(5),
            ExprToken::AndAnd if true => Some(6),
            ExprToken::EqEq if true => Some(7),
            ExprToken::NotEq if true => Some(8),
            ExprToken::LessEq if true => Some(9),
            ExprToken::GreaterEq if true => Some(10),
            ExprToken::Less if true => Some(11),
            ExprToken::Greater if true => Some(12),
            ExprToken::Plus if true => Some(13),
            ExprToken::Minus if true => Some(14),
            ExprToken::Star if true => Some(15),
            ExprToken::Slash if true => Some(16),
            ExprToken::Percent if true => Some(17),
            ExprToken::Bang if true => Some(18),
            ExprToken::Amp if true => Some(19),
            ExprToken::Pipe if true => Some(20),
            ExprToken::Caret if true => Some(21),
            ExprToken::Tilde if true => Some(22),
            ExprToken::ShiftLeft if true => Some(23),
            ExprToken::ShiftRight if true => Some(24),
            ExprToken::LParen if true => Some(25),
            ExprToken::RParen if true => Some(26),
            ExprToken::Comma if true => Some(27),
            _ => None,
        }
    }
    fn __token_to_symbol<
        'input,
    >(
        __token_index: usize,
        __token: ExprToken<'input>,
        _: core::marker::PhantomData<(&'input ())>,
    ) -> __Symbol<'input>
    {
        #[allow(clippy::manual_range_patterns)]match __token_index {
            0 | 1 | 2 | 3 => match __token {
                ExprToken::Number(__tok0) | ExprToken::Character(__tok0) | ExprToken::Ident(__tok0) | ExprToken::Invalid(__tok0) if true => __Symbol::Variant0(__tok0),
                _ => unreachable!(),
            },
            4 | 5 | 6 | 7 | 8 | 9 | 10 | 11 | 12 | 13 | 14 | 15 | 16 | 17 | 18 | 19 | 20 | 21 | 22 | 23 | 24 | 25 | 26 | 27 => __Symbol::Variant1(__token),
            _ => unreachable!(),
        }
    }
    fn __simulate_reduce<
        'input,
    >(
        __reduce_index: i8,
        _: core::marker::PhantomData<(&'input ())>,
    ) -> __state_machine::SimulatedReduce<__StateMachine<'input>>
    {
        match __reduce_index {
            0 => {
                __state_machine::SimulatedReduce::Reduce {
                    states_to_pop: 2,
                    nonterminal_produced: 0,
                }
            }
            1 => {
                __state_machine::SimulatedReduce::Reduce {
                    states_to_pop: 0,
                    nonterminal_produced: 1,
                }
            }
            2 => {
                __state_machine::SimulatedReduce::Reduce {
                    states_to_pop: 1,
                    nonterminal_produced: 1,
                }
            }
            3 => {
                __state_machine::SimulatedReduce::Reduce {
                    states_to_pop: 2,
                    nonterminal_produced: 2,
                }
            }
            4 => {
                __state_machine::SimulatedReduce::Reduce {
                    states_to_pop: 3,
                    nonterminal_produced: 2,
                }
            }
            5 => {
                __state_machine::SimulatedReduce::Reduce {
                    states_to_pop: 3,
                    nonterminal_produced: 3,
                }
            }
            6 => {
                __state_machine::SimulatedReduce::Reduce {
                    states_to_pop: 3,
                    nonterminal_produced: 3,
                }
            }
            7 => {
                __state_machine::SimulatedReduce::Reduce {
                    states_to_pop: 1,
                    nonterminal_produced: 3,
                }
            }
            8 => {
                __state_machine::SimulatedReduce::Reduce {
                    states_to_pop: 3,
                    nonterminal_produced: 4,
                }
            }
            9 => {
                __state_machine::SimulatedReduce::Reduce {
                    states_to_pop: 1,
                    nonterminal_produced: 4,
                }
            }
            10 => {
                __state_machine::SimulatedReduce::Reduce {
                    states_to_pop: 3,
                    nonterminal_produced: 5,
                }
            }
            11 => {
                __state_machine::SimulatedReduce::Reduce {
                    states_to_pop: 1,
                    nonterminal_produced: 5,
                }
            }
            12 => {
                __state_machine::SimulatedReduce::Reduce {
                    states_to_pop: 3,
                    nonterminal_produced: 6,
                }
            }
            13 => {
                __state_machine::SimulatedReduce::Reduce {
                    states_to_pop: 1,
                    nonterminal_produced: 6,
                }
            }
            14 => {
                __state_machine::SimulatedReduce::Reduce {
                    states_to_pop: 3,
                    nonterminal_produced: 7,
                }
            }
            15 => {
                __state_machine::SimulatedReduce::Reduce {
                    states_to_pop: 1,
                    nonterminal_produced: 7,
                }
            }
            16 => {
                __state_machine::SimulatedReduce::Reduce {
                    states_to_pop: 0,
                    nonterminal_produced: 8,
                }
            }
            17 => {
                __state_machine::SimulatedReduce::Reduce {
                    states_to_pop: 1,
                    nonterminal_produced: 8,
                }
            }
            18 => {
                __state_machine::SimulatedReduce::Reduce {
                    states_to_pop: 2,
                    nonterminal_produced: 8,
                }
            }
            19 => {
                __state_machine::SimulatedReduce::Reduce {
                    states_to_pop: 3,
                    nonterminal_produced: 9,
                }
            }
            20 => {
                __state_machine::SimulatedReduce::Reduce {
                    states_to_pop: 3,
                    nonterminal_produced: 9,
                }
            }
            21 => {
                __state_machine::SimulatedReduce::Reduce {
                    states_to_pop: 1,
                    nonterminal_produced: 9,
                }
            }
            22 => {
                __state_machine::SimulatedReduce::Reduce {
                    states_to_pop: 1,
                    nonterminal_produced: 10,
                }
            }
            23 => {
                __state_machine::SimulatedReduce::Reduce {
                    states_to_pop: 3,
                    nonterminal_produced: 11,
                }
            }
            24 => {
                __state_machine::SimulatedReduce::Reduce {
                    states_to_pop: 1,
                    nonterminal_produced: 11,
                }
            }
            25 => {
                __state_machine::SimulatedReduce::Reduce {
                    states_to_pop: 3,
                    nonterminal_produced: 12,
                }
            }
            26 => {
                __state_machine::SimulatedReduce::Reduce {
                    states_to_pop: 1,
                    nonterminal_produced: 12,
                }
            }
            27 => {
                __state_machine::SimulatedReduce::Reduce {
                    states_to_pop: 3,
                    nonterminal_produced: 13,
                }
            }
            28 => {
                __state_machine::SimulatedReduce::Reduce {
                    states_to_pop: 3,
                    nonterminal_produced: 13,
                }
            }
            29 => {
                __state_machine::SimulatedReduce::Reduce {
                    states_to_pop: 3,
                    nonterminal_produced: 13,
                }
            }
            30 => {
                __state_machine::SimulatedReduce::Reduce {
                    states_to_pop: 1,
                    nonterminal_produced: 13,
                }
            }
            31 => {
                __state_machine::SimulatedReduce::Reduce {
                    states_to_pop: 4,
                    nonterminal_produced: 14,
                }
            }
            32 => {
                __state_machine::SimulatedReduce::Reduce {
                    states_to_pop: 1,
                    nonterminal_produced: 14,
                }
            }
            33 => {
                __state_machine::SimulatedReduce::Reduce {
                    states_to_pop: 1,
                    nonterminal_produced: 15,
                }
            }
            34 => {
                __state_machine::SimulatedReduce::Reduce {
                    states_to_pop: 1,
                    nonterminal_produced: 15,
                }
            }
            35 => {
                __state_machine::SimulatedReduce::Reduce {
                    states_to_pop: 1,
                    nonterminal_produced: 15,
                }
            }
            36 => {
                __state_machine::SimulatedReduce::Reduce {
                    states_to_pop: 3,
                    nonterminal_produced: 15,
                }
            }
            37 => {
                __state_machine::SimulatedReduce::Reduce {
                    states_to_pop: 3,
                    nonterminal_produced: 16,
                }
            }
            38 => {
                __state_machine::SimulatedReduce::Reduce {
                    states_to_pop: 3,
                    nonterminal_produced: 16,
                }
            }
            39 => {
                __state_machine::SimulatedReduce::Reduce {
                    states_to_pop: 3,
                    nonterminal_produced: 16,
                }
            }
            40 => {
                __state_machine::SimulatedReduce::Reduce {
                    states_to_pop: 3,
                    nonterminal_produced: 16,
                }
            }
            41 => {
                __state_machine::SimulatedReduce::Reduce {
                    states_to_pop: 1,
                    nonterminal_produced: 16,
                }
            }
            42 => {
                __state_machine::SimulatedReduce::Reduce {
                    states_to_pop: 3,
                    nonterminal_produced: 17,
                }
            }
            43 => {
                __state_machine::SimulatedReduce::Reduce {
                    states_to_pop: 3,
                    nonterminal_produced: 17,
                }
            }
            44 => {
                __state_machine::SimulatedReduce::Reduce {
                    states_to_pop: 1,
                    nonterminal_produced: 17,
                }
            }
            45 => {
                __state_machine::SimulatedReduce::Reduce {
                    states_to_pop: 2,
                    nonterminal_produced: 18,
                }
            }
            46 => {
                __state_machine::SimulatedReduce::Reduce {
                    states_to_pop: 2,
                    nonterminal_produced: 18,
                }
            }
            47 => {
                __state_machine::SimulatedReduce::Reduce {
                    states_to_pop: 2,
                    nonterminal_produced: 18,
                }
            }
            48 => {
                __state_machine::SimulatedReduce::Reduce {
                    states_to_pop: 2,
                    nonterminal_produced: 18,
                }
            }
            49 => {
                __state_machine::SimulatedReduce::Reduce {
                    states_to_pop: 2,
                    nonterminal_produced: 18,
                }
            }
            50 => {
                __state_machine::SimulatedReduce::Reduce {
                    states_to_pop: 2,
                    nonterminal_produced: 18,
                }
            }
            51 => {
                __state_machine::SimulatedReduce::Reduce {
                    states_to_pop: 1,
                    nonterminal_produced: 18,
                }
            }
            52 => __state_machine::SimulatedReduce::Accept,
            _ => panic!("invalid reduction index {__reduce_index}")
        }
    }
    pub struct ExpressionParser {
        _priv: (),
    }

    impl Default for ExpressionParser { fn default() -> Self { Self::new() } }
    impl ExpressionParser {
        pub fn new() -> ExpressionParser {
            ExpressionParser {
                _priv: (),
            }
        }

        #[allow(dead_code)]
        pub fn parse<
            'input,
            __TOKEN: __ToTriple<'input, >,
            __TOKENS: IntoIterator<Item=__TOKEN>,
        >(
            &self,
            __tokens0: __TOKENS,
        ) -> Result<Expr, __lalrpop_util::ParseError<usize, ExprToken<'input>, &'static str>>
        {
            let __tokens = __tokens0.into_iter();
            let mut __tokens = __tokens.map(|t| __ToTriple::to_triple(t));
            __state_machine::Parser::drive(
                __StateMachine {
                    __phantom: core::marker::PhantomData::<(&())>,
                },
                __tokens,
            )
        }
    }
    fn __accepts<
        'input,
    >(
        __error_state: Option<i8>,
        __states: &[i8],
        __opt_integer: Option<usize>,
        _: core::marker::PhantomData<(&'input ())>,
    ) -> bool
    {
        let mut __states = __states.to_vec();
        __states.extend(__error_state);
        loop {
            let mut __states_len = __states.len();
            let __top = __states[__states_len - 1];
            let __action = match __opt_integer {
                None => __EOF_ACTION[__top as usize],
                Some(__integer) => __action(__top, __integer),
            };
            if __action == 0 { return false; }
            if __action > 0 { return true; }
            let (__to_pop, __nt) = match __simulate_reduce(-(__action + 1), core::marker::PhantomData::<(&())>) {
                __state_machine::SimulatedReduce::Reduce {
                    states_to_pop, nonterminal_produced
                } => (states_to_pop, nonterminal_produced),
                __state_machine::SimulatedReduce::Accept => return true,
            };
            __states_len -= __to_pop;
            __states.truncate(__states_len);
            let __top = __states[__states_len - 1];
            let __next_state = __goto(__top, __nt);
            __states.push(__next_state);
        }
    }
    fn __reduce<
        'input,
    >(
        __action: i8,
        __lookahead_start: Option<&usize>,
        __states: &mut alloc::vec::Vec<i8>,
        __symbols: &mut alloc::vec::Vec<(usize,__Symbol<'input>,usize)>,
        _: core::marker::PhantomData<(&'input ())>,
    ) -> Option<Result<Expr,__lalrpop_util::ParseError<usize, ExprToken<'input>, &'static str>>>
    {
        let (__pop_states, __nonterminal) = match __action {
            0 => {
                __reduce0(__lookahead_start, __symbols, core::marker::PhantomData::<(&())>)
            }
            1 => {
                __reduce1(__lookahead_start, __symbols, core::marker::PhantomData::<(&())>)
            }
            2 => {
                __reduce2(__lookahead_start, __symbols, core::marker::PhantomData::<(&())>)
            }
            3 => {
                __reduce3(__lookahead_start, __symbols, core::marker::PhantomData::<(&())>)
            }
            4 => {
                __reduce4(__lookahead_start, __symbols, core::marker::PhantomData::<(&())>)
            }
            5 => {
                __reduce5(__lookahead_start, __symbols, core::marker::PhantomData::<(&())>)
            }
            6 => {
                __reduce6(__lookahead_start, __symbols, core::marker::PhantomData::<(&())>)
            }
            7 => {
                __reduce7(__lookahead_start, __symbols, core::marker::PhantomData::<(&())>)
            }
            8 => {
                __reduce8(__lookahead_start, __symbols, core::marker::PhantomData::<(&())>)
            }
            9 => {
                __reduce9(__lookahead_start, __symbols, core::marker::PhantomData::<(&())>)
            }
            10 => {
                __reduce10(__lookahead_start, __symbols, core::marker::PhantomData::<(&())>)
            }
            11 => {
                __reduce11(__lookahead_start, __symbols, core::marker::PhantomData::<(&())>)
            }
            12 => {
                __reduce12(__lookahead_start, __symbols, core::marker::PhantomData::<(&())>)
            }
            13 => {
                __reduce13(__lookahead_start, __symbols, core::marker::PhantomData::<(&())>)
            }
            14 => {
                __reduce14(__lookahead_start, __symbols, core::marker::PhantomData::<(&())>)
            }
            15 => {
                __reduce15(__lookahead_start, __symbols, core::marker::PhantomData::<(&())>)
            }
            16 => {
                __reduce16(__lookahead_start, __symbols, core::marker::PhantomData::<(&())>)
            }
            17 => {
                __reduce17(__lookahead_start, __symbols, core::marker::PhantomData::<(&())>)
            }
            18 => {
                __reduce18(__lookahead_start, __symbols, core::marker::PhantomData::<(&())>)
            }
            19 => {
                __reduce19(__lookahead_start, __symbols, core::marker::PhantomData::<(&())>)
            }
            20 => {
                __reduce20(__lookahead_start, __symbols, core::marker::PhantomData::<(&())>)
            }
            21 => {
                __reduce21(__lookahead_start, __symbols, core::marker::PhantomData::<(&())>)
            }
            22 => {
                __reduce22(__lookahead_start, __symbols, core::marker::PhantomData::<(&())>)
            }
            23 => {
                __reduce23(__lookahead_start, __symbols, core::marker::PhantomData::<(&())>)
            }
            24 => {
                __reduce24(__lookahead_start, __symbols, core::marker::PhantomData::<(&())>)
            }
            25 => {
                __reduce25(__lookahead_start, __symbols, core::marker::PhantomData::<(&())>)
            }
            26 => {
                __reduce26(__lookahead_start, __symbols, core::marker::PhantomData::<(&())>)
            }
            27 => {
                __reduce27(__lookahead_start, __symbols, core::marker::PhantomData::<(&())>)
            }
            28 => {
                __reduce28(__lookahead_start, __symbols, core::marker::PhantomData::<(&())>)
            }
            29 => {
                __reduce29(__lookahead_start, __symbols, core::marker::PhantomData::<(&())>)
            }
            30 => {
                __reduce30(__lookahead_start, __symbols, core::marker::PhantomData::<(&())>)
            }
            31 => {
                __reduce31(__lookahead_start, __symbols, core::marker::PhantomData::<(&())>)
            }
            32 => {
                __reduce32(__lookahead_start, __symbols, core::marker::PhantomData::<(&())>)
            }
            33 => {
                // Primary = "number" => ActionFn(43);
                let __sym0 = __pop_Variant0(__symbols);
                let __start = __sym0.0.clone();
                let __end = __sym0.2.clone();
                let __nt = match super::__action43::<>(__sym0) {
                    Ok(v) => v,
                    Err(e) => return Some(Err(e)),
                };
                __symbols.push((__start, __Symbol::Variant2(__nt), __end));
                (1, 15)
            }
            34 => {
                // Primary = "character" => ActionFn(44);
                let __sym0 = __pop_Variant0(__symbols);
                let __start = __sym0.0.clone();
                let __end = __sym0.2.clone();
                let __nt = match super::__action44::<>(__sym0) {
                    Ok(v) => v,
                    Err(e) => return Some(Err(e)),
                };
                __symbols.push((__start, __Symbol::Variant2(__nt), __end));
                (1, 15)
            }
            35 => {
                __reduce35(__lookahead_start, __symbols, core::marker::PhantomData::<(&())>)
            }
            36 => {
                __reduce36(__lookahead_start, __symbols, core::marker::PhantomData::<(&())>)
            }
            37 => {
                __reduce37(__lookahead_start, __symbols, core::marker::PhantomData::<(&())>)
            }
            38 => {
                __reduce38(__lookahead_start, __symbols, core::marker::PhantomData::<(&())>)
            }
            39 => {
                __reduce39(__lookahead_start, __symbols, core::marker::PhantomData::<(&())>)
            }
            40 => {
                __reduce40(__lookahead_start, __symbols, core::marker::PhantomData::<(&())>)
            }
            41 => {
                __reduce41(__lookahead_start, __symbols, core::marker::PhantomData::<(&())>)
            }
            42 => {
                __reduce42(__lookahead_start, __symbols, core::marker::PhantomData::<(&())>)
            }
            43 => {
                __reduce43(__lookahead_start, __symbols, core::marker::PhantomData::<(&())>)
            }
            44 => {
                __reduce44(__lookahead_start, __symbols, core::marker::PhantomData::<(&())>)
            }
            45 => {
                __reduce45(__lookahead_start, __symbols, core::marker::PhantomData::<(&())>)
            }
            46 => {
                __reduce46(__lookahead_start, __symbols, core::marker::PhantomData::<(&())>)
            }
            47 => {
                __reduce47(__lookahead_start, __symbols, core::marker::PhantomData::<(&())>)
            }
            48 => {
                __reduce48(__lookahead_start, __symbols, core::marker::PhantomData::<(&())>)
            }
            49 => {
                __reduce49(__lookahead_start, __symbols, core::marker::PhantomData::<(&())>)
            }
            50 => {
                __reduce50(__lookahead_start, __symbols, core::marker::PhantomData::<(&())>)
            }
            51 => {
                __reduce51(__lookahead_start, __symbols, core::marker::PhantomData::<(&())>)
            }
            52 => {
                // __Expression = Expression => ActionFn(0);
                let __sym0 = __pop_Variant2(__symbols);
                let __start = __sym0.0.clone();
                let __end = __sym0.2.clone();
                let __nt = super::__action0::<>(__sym0);
                return Some(Ok(__nt));
            }
            _ => panic!("invalid action code {__action}")
        };
        let __states_len = __states.len();
        __states.truncate(__states_len - __pop_states);
        let __state = *__states.last().unwrap();
        let __next_state = __goto(__state, __nonterminal);
        __states.push(__next_state);
        None
    }
    #[inline(never)]
    fn __symbol_type_mismatch() -> ! {
        panic!("symbol type mismatch")
    }
    fn __pop_Variant2<
      'input,
    >(
        __symbols: &mut alloc::vec::Vec<(usize,__Symbol<'input>,usize)>
    ) -> (usize, Expr, usize)
     {
        match __symbols.pop() {
            Some((__l, __Symbol::Variant2(__v), __r)) => (__l, __v, __r),
            _ => __symbol_type_mismatch()
        }
    }
    fn __pop_Variant1<
      'input,
    >(
        __symbols: &mut alloc::vec::Vec<(usize,__Symbol<'input>,usize)>
    ) -> (usize, ExprToken<'input>, usize)
     {
        match __symbols.pop() {
            Some((__l, __Symbol::Variant1(__v), __r)) => (__l, __v, __r),
            _ => __symbol_type_mismatch()
        }
    }
    fn __pop_Variant4<
      'input,
    >(
        __symbols: &mut alloc::vec::Vec<(usize,__Symbol<'input>,usize)>
    ) -> (usize, Vec<Expr>, usize)
     {
        match __symbols.pop() {
            Some((__l, __Symbol::Variant4(__v), __r)) => (__l, __v, __r),
            _ => __symbol_type_mismatch()
        }
    }
    fn __pop_Variant3<
      'input,
    >(
        __symbols: &mut alloc::vec::Vec<(usize,__Symbol<'input>,usize)>
    ) -> (usize, alloc::vec::Vec<Expr>, usize)
     {
        match __symbols.pop() {
            Some((__l, __Symbol::Variant3(__v), __r)) => (__l, __v, __r),
            _ => __symbol_type_mismatch()
        }
    }
    fn __pop_Variant0<
      'input,
    >(
        __symbols: &mut alloc::vec::Vec<(usize,__Symbol<'input>,usize)>
    ) -> (usize, &'input str, usize)
     {
        match __symbols.pop() {
            Some((__l, __Symbol::Variant0(__v), __r)) => (__l, __v, __r),
            _ => __symbol_type_mismatch()
        }
    }
    fn __reduce0<
        'input,
    >(
        __lookahead_start: Option<&usize>,
        __symbols: &mut alloc::vec::Vec<(usize,__Symbol<'input>,usize)>,
        _: core::marker::PhantomData<(&'input ())>,
    ) -> (usize, usize)
    {
        // ("," <Assign>) = ",", Assign => ActionFn(49);
        assert!(__symbols.len() >= 2);
        let __sym1 = __pop_Variant2(__symbols);
        let __sym0 = __pop_Variant1(__symbols);
        let __start = __sym0.0.clone();
        let __end = __sym1.2.clone();
        let __nt = super::__action49::<>(__sym0, __sym1);
        __symbols.push((__start, __Symbol::Variant2(__nt), __end));
        (2, 0)
    }
    fn __reduce1<
        'input,
    >(
        __lookahead_start: Option<&usize>,
        __symbols: &mut alloc::vec::Vec<(usize,__Symbol<'input>,usize)>,
        _: core::marker::PhantomData<(&'input ())>,
    ) -> (usize, usize)
    {
        // ("," <Assign>)* =  => ActionFn(47);
        let __start = __lookahead_start.cloned().or_else(|| __symbols.last().map(|s| s.2)).unwrap_or_default();
        let __end = __start;
        let __nt = super::__action47::<>(&__start, &__end);
        __symbols.push((__start, __Symbol::Variant3(__nt), __end));
        (0, 1)
    }
    fn __reduce2<
        'input,
    >(
        __lookahead_start: Option<&usize>,
        __symbols: &mut alloc::vec::Vec<(usize,__Symbol<'input>,usize)>,
        _: core::marker::PhantomData<(&'input ())>,
    ) -> (usize, usize)
    {
        // ("," <Assign>)* = ("," <Assign>)+ => ActionFn(48);
        let __sym0 = __pop_Variant3(__symbols);
        let __start = __sym0.0.clone();
        let __end = __sym0.2.clone();
        let __nt = super::__action48::<>(__sym0);
        __symbols.push((__start, __Symbol::Variant3(__nt), __end));
        (1, 1)
    }
    fn __reduce3<
        'input,
    >(
        __lookahead_start: Option<&usize>,
        __symbols: &mut alloc::vec::Vec<(usize,__Symbol<'input>,usize)>,
        _: core::marker::PhantomData<(&'input ())>,
    ) -> (usize, usize)
    {
        // ("," <Assign>)+ = ",", Assign => ActionFn(52);
        assert!(__symbols.len() >= 2);
        let __sym1 = __pop_Variant2(__symbols);
        let __sym0 = __pop_Variant1(__symbols);
        let __start = __sym0.0.clone();
        let __end = __sym1.2.clone();
        let __nt = super::__action52::<>(__sym0, __sym1);
        __symbols.push((__start, __Symbol::Variant3(__nt), __end));
        (2, 2)
    }
    fn __reduce4<
        'input,
    >(
        __lookahead_start: Option<&usize>,
        __symbols: &mut alloc::vec::Vec<(usize,__Symbol<'input>,usize)>,
        _: core::marker::PhantomData<(&'input ())>,
    ) -> (usize, usize)
    {
        // ("," <Assign>)+ = ("," <Assign>)+, ",", Assign => ActionFn(53);
        assert!(__symbols.len() >= 3);
        let __sym2 = __pop_Variant2(__symbols);
        let __sym1 = __pop_Variant1(__symbols);
        let __sym0 = __pop_Variant3(__symbols);
        let __start = __sym0.0.clone();
        let __end = __sym2.2.clone();
        let __nt = super::__action53::<>(__sym0, __sym1, __sym2);
        __symbols.push((__start, __Symbol::Variant3(__nt), __end));
        (3, 2)
    }
    fn __reduce5<
        'input,
    >(
        __lookahead_start: Option<&usize>,
        __symbols: &mut alloc::vec::Vec<(usize,__Symbol<'input>,usize)>,
        _: core::marker::PhantomData<(&'input ())>,
    ) -> (usize, usize)
    {
        // Additive = Additive, "+", Multiplicative => ActionFn(25);
        assert!(__symbols.len() >= 3);
        let __sym2 = __pop_Variant2(__symbols);
        let __sym1 = __pop_Variant1(__symbols);
        let __sym0 = __pop_Variant2(__symbols);
        let __start = __sym0.0.clone();
        let __end = __sym2.2.clone();
        let __nt = super::__action25::<>(__sym0, __sym1, __sym2);
        __symbols.push((__start, __Symbol::Variant2(__nt), __end));
        (3, 3)
    }
    fn __reduce6<
        'input,
    >(
        __lookahead_start: Option<&usize>,
        __symbols: &mut alloc::vec::Vec<(usize,__Symbol<'input>,usize)>,
        _: core::marker::PhantomData<(&'input ())>,
    ) -> (usize, usize)
    {
        // Additive = Additive, "-", Multiplicative => ActionFn(26);
        assert!(__symbols.len() >= 3);
        let __sym2 = __pop_Variant2(__symbols);
        let __sym1 = __pop_Variant1(__symbols);
        let __sym0 = __pop_Variant2(__symbols);
        let __start = __sym0.0.clone();
        let __end = __sym2.2.clone();
        let __nt = super::__action26::<>(__sym0, __sym1, __sym2);
        __symbols.push((__start, __Symbol::Variant2(__nt), __end));
        (3, 3)
    }
    fn __reduce7<
        'input,
    >(
        __lookahead_start: Option<&usize>,
        __symbols: &mut alloc::vec::Vec<(usize,__Symbol<'input>,usize)>,
        _: core::marker::PhantomData<(&'input ())>,
    ) -> (usize, usize)
    {
        // Additive = Multiplicative => ActionFn(27);
        let __sym0 = __pop_Variant2(__symbols);
        let __start = __sym0.0.clone();
        let __end = __sym0.2.clone();
        let __nt = super::__action27::<>(__sym0);
        __symbols.push((__start, __Symbol::Variant2(__nt), __end));
        (1, 3)
    }
    fn __reduce8<
        'input,
    >(
        __lookahead_start: Option<&usize>,
        __symbols: &mut alloc::vec::Vec<(usize,__Symbol<'input>,usize)>,
        _: core::marker::PhantomData<(&'input ())>,
    ) -> (usize, usize)
    {
        // Assign = LogicalOr, "=", Assign => ActionFn(2);
        assert!(__symbols.len() >= 3);
        let __sym2 = __pop_Variant2(__symbols);
        let __sym1 = __pop_Variant1(__symbols);
        let __sym0 = __pop_Variant2(__symbols);
        let __start = __sym0.0.clone();
        let __end = __sym2.2.clone();
        let __nt = super::__action2::<>(__sym0, __sym1, __sym2);
        __symbols.push((__start, __Symbol::Variant2(__nt), __end));
        (3, 4)
    }
    fn __reduce9<
        'input,
    >(
        __lookahead_start: Option<&usize>,
        __symbols: &mut alloc::vec::Vec<(usize,__Symbol<'input>,usize)>,
        _: core::marker::PhantomData<(&'input ())>,
    ) -> (usize, usize)
    {
        // Assign = LogicalOr => ActionFn(3);
        let __sym0 = __pop_Variant2(__symbols);
        let __start = __sym0.0.clone();
        let __end = __sym0.2.clone();
        let __nt = super::__action3::<>(__sym0);
        __symbols.push((__start, __Symbol::Variant2(__nt), __end));
        (1, 4)
    }
    fn __reduce10<
        'input,
    >(
        __lookahead_start: Option<&usize>,
        __symbols: &mut alloc::vec::Vec<(usize,__Symbol<'input>,usize)>,
        _: core::marker::PhantomData<(&'input ())>,
    ) -> (usize, usize)
    {
        // BitAnd = BitAnd, "&", Equality => ActionFn(12);
        assert!(__symbols.len() >= 3);
        let __sym2 = __pop_Variant2(__symbols);
        let __sym1 = __pop_Variant1(__symbols);
        let __sym0 = __pop_Variant2(__symbols);
        let __start = __sym0.0.clone();
        let __end = __sym2.2.clone();
        let __nt = super::__action12::<>(__sym0, __sym1, __sym2);
        __symbols.push((__start, __Symbol::Variant2(__nt), __end));
        (3, 5)
    }
    fn __reduce11<
        'input,
    >(
        __lookahead_start: Option<&usize>,
        __symbols: &mut alloc::vec::Vec<(usize,__Symbol<'input>,usize)>,
        _: core::marker::PhantomData<(&'input ())>,
    ) -> (usize, usize)
    {
        // BitAnd = Equality => ActionFn(13);
        let __sym0 = __pop_Variant2(__symbols);
        let __start = __sym0.0.clone();
        let __end = __sym0.2.clone();
        let __nt = super::__action13::<>(__sym0);
        __symbols.push((__start, __Symbol::Variant2(__nt), __end));
        (1, 5)
    }
    fn __reduce12<
        'input,
    >(
        __lookahead_start: Option<&usize>,
        __symbols: &mut alloc::vec::Vec<(usize,__Symbol<'input>,usize)>,
        _: core::marker::PhantomData<(&'input ())>,
    ) -> (usize, usize)
    {
        // BitOr = BitOr, "|", BitXor => ActionFn(8);
        assert!(__symbols.len() >= 3);
        let __sym2 = __pop_Variant2(__symbols);
        let __sym1 = __pop_Variant1(__symbols);
        let __sym0 = __pop_Variant2(__symbols);
        let __start = __sym0.0.clone();
        let __end = __sym2.2.clone();
        let __nt = super::__action8::<>(__sym0, __sym1, __sym2);
        __symbols.push((__start, __Symbol::Variant2(__nt), __end));
        (3, 6)
    }
    fn __reduce13<
        'input,
    >(
        __lookahead_start: Option<&usize>,
        __symbols: &mut alloc::vec::Vec<(usize,__Symbol<'input>,usize)>,
        _: core::marker::PhantomData<(&'input ())>,
    ) -> (usize, usize)
    {
        // BitOr = BitXor => ActionFn(9);
        let __sym0 = __pop_Variant2(__symbols);
        let __start = __sym0.0.clone();
        let __end = __sym0.2.clone();
        let __nt = super::__action9::<>(__sym0);
        __symbols.push((__start, __Symbol::Variant2(__nt), __end));
        (1, 6)
    }
    fn __reduce14<
        'input,
    >(
        __lookahead_start: Option<&usize>,
        __symbols: &mut alloc::vec::Vec<(usize,__Symbol<'input>,usize)>,
        _: core::marker::PhantomData<(&'input ())>,
    ) -> (usize, usize)
    {
        // BitXor = BitXor, "^", BitAnd => ActionFn(10);
        assert!(__symbols.len() >= 3);
        let __sym2 = __pop_Variant2(__symbols);
        let __sym1 = __pop_Variant1(__symbols);
        let __sym0 = __pop_Variant2(__symbols);
        let __start = __sym0.0.clone();
        let __end = __sym2.2.clone();
        let __nt = super::__action10::<>(__sym0, __sym1, __sym2);
        __symbols.push((__start, __Symbol::Variant2(__nt), __end));
        (3, 7)
    }
    fn __reduce15<
        'input,
    >(
        __lookahead_start: Option<&usize>,
        __symbols: &mut alloc::vec::Vec<(usize,__Symbol<'input>,usize)>,
        _: core::marker::PhantomData<(&'input ())>,
    ) -> (usize, usize)
    {
        // BitXor = BitAnd => ActionFn(11);
        let __sym0 = __pop_Variant2(__symbols);
        let __start = __sym0.0.clone();
        let __end = __sym0.2.clone();
        let __nt = super::__action11::<>(__sym0);
        __symbols.push((__start, __Symbol::Variant2(__nt), __end));
        (1, 7)
    }
    fn __reduce16<
        'input,
    >(
        __lookahead_start: Option<&usize>,
        __symbols: &mut alloc::vec::Vec<(usize,__Symbol<'input>,usize)>,
        _: core::marker::PhantomData<(&'input ())>,
    ) -> (usize, usize)
    {
        // CallArgs =  => ActionFn(41);
        let __start = __lookahead_start.cloned().or_else(|| __symbols.last().map(|s| s.2)).unwrap_or_default();
        let __end = __start;
        let __nt = super::__action41::<>(&__start, &__end);
        __symbols.push((__start, __Symbol::Variant4(__nt), __end));
        (0, 8)
    }
    fn __reduce17<
        'input,
    >(
        __lookahead_start: Option<&usize>,
        __symbols: &mut alloc::vec::Vec<(usize,__Symbol<'input>,usize)>,
        _: core::marker::PhantomData<(&'input ())>,
    ) -> (usize, usize)
    {
        // CallArgs = Assign => ActionFn(54);
        let __sym0 = __pop_Variant2(__symbols);
        let __start = __sym0.0.clone();
        let __end = __sym0.2.clone();
        let __nt = super::__action54::<>(__sym0);
        __symbols.push((__start, __Symbol::Variant4(__nt), __end));
        (1, 8)
    }
    fn __reduce18<
        'input,
    >(
        __lookahead_start: Option<&usize>,
        __symbols: &mut alloc::vec::Vec<(usize,__Symbol<'input>,usize)>,
        _: core::marker::PhantomData<(&'input ())>,
    ) -> (usize, usize)
    {
        // CallArgs = Assign, ("," <Assign>)+ => ActionFn(55);
        assert!(__symbols.len() >= 2);
        let __sym1 = __pop_Variant3(__symbols);
        let __sym0 = __pop_Variant2(__symbols);
        let __start = __sym0.0.clone();
        let __end = __sym1.2.clone();
        let __nt = super::__action55::<>(__sym0, __sym1);
        __symbols.push((__start, __Symbol::Variant4(__nt), __end));
        (2, 8)
    }
    fn __reduce19<
        'input,
    >(
        __lookahead_start: Option<&usize>,
        __symbols: &mut alloc::vec::Vec<(usize,__Symbol<'input>,usize)>,
        _: core::marker::PhantomData<(&'input ())>,
    ) -> (usize, usize)
    {
        // Equality = Equality, "==", Relational => ActionFn(14);
        assert!(__symbols.len() >= 3);
        let __sym2 = __pop_Variant2(__symbols);
        let __sym1 = __pop_Variant1(__symbols);
        let __sym0 = __pop_Variant2(__symbols);
        let __start = __sym0.0.clone();
        let __end = __sym2.2.clone();
        let __nt = super::__action14::<>(__sym0, __sym1, __sym2);
        __symbols.push((__start, __Symbol::Variant2(__nt), __end));
        (3, 9)
    }
    fn __reduce20<
        'input,
    >(
        __lookahead_start: Option<&usize>,
        __symbols: &mut alloc::vec::Vec<(usize,__Symbol<'input>,usize)>,
        _: core::marker::PhantomData<(&'input ())>,
    ) -> (usize, usize)
    {
        // Equality = Equality, "!=", Relational => ActionFn(15);
        assert!(__symbols.len() >= 3);
        let __sym2 = __pop_Variant2(__symbols);
        let __sym1 = __pop_Variant1(__symbols);
        let __sym0 = __pop_Variant2(__symbols);
        let __start = __sym0.0.clone();
        let __end = __sym2.2.clone();
        let __nt = super::__action15::<>(__sym0, __sym1, __sym2);
        __symbols.push((__start, __Symbol::Variant2(__nt), __end));
        (3, 9)
    }
    fn __reduce21<
        'input,
    >(
        __lookahead_start: Option<&usize>,
        __symbols: &mut alloc::vec::Vec<(usize,__Symbol<'input>,usize)>,
        _: core::marker::PhantomData<(&'input ())>,
    ) -> (usize, usize)
    {
        // Equality = Relational => ActionFn(16);
        let __sym0 = __pop_Variant2(__symbols);
        let __start = __sym0.0.clone();
        let __end = __sym0.2.clone();
        let __nt = super::__action16::<>(__sym0);
        __symbols.push((__start, __Symbol::Variant2(__nt), __end));
        (1, 9)
    }
    fn __reduce22<
        'input,
    >(
        __lookahead_start: Option<&usize>,
        __symbols: &mut alloc::vec::Vec<(usize,__Symbol<'input>,usize)>,
        _: core::marker::PhantomData<(&'input ())>,
    ) -> (usize, usize)
    {
        // Expression = Assign => ActionFn(1);
        let __sym0 = __pop_Variant2(__symbols);
        let __start = __sym0.0.clone();
        let __end = __sym0.2.clone();
        let __nt = super::__action1::<>(__sym0);
        __symbols.push((__start, __Symbol::Variant2(__nt), __end));
        (1, 10)
    }
    fn __reduce23<
        'input,
    >(
        __lookahead_start: Option<&usize>,
        __symbols: &mut alloc::vec::Vec<(usize,__Symbol<'input>,usize)>,
        _: core::marker::PhantomData<(&'input ())>,
    ) -> (usize, usize)
    {
        // LogicalAnd = LogicalAnd, "&&", BitOr => ActionFn(6);
        assert!(__symbols.len() >= 3);
        let __sym2 = __pop_Variant2(__symbols);
        let __sym1 = __pop_Variant1(__symbols);
        let __sym0 = __pop_Variant2(__symbols);
        let __start = __sym0.0.clone();
        let __end = __sym2.2.clone();
        let __nt = super::__action6::<>(__sym0, __sym1, __sym2);
        __symbols.push((__start, __Symbol::Variant2(__nt), __end));
        (3, 11)
    }
    fn __reduce24<
        'input,
    >(
        __lookahead_start: Option<&usize>,
        __symbols: &mut alloc::vec::Vec<(usize,__Symbol<'input>,usize)>,
        _: core::marker::PhantomData<(&'input ())>,
    ) -> (usize, usize)
    {
        // LogicalAnd = BitOr => ActionFn(7);
        let __sym0 = __pop_Variant2(__symbols);
        let __start = __sym0.0.clone();
        let __end = __sym0.2.clone();
        let __nt = super::__action7::<>(__sym0);
        __symbols.push((__start, __Symbol::Variant2(__nt), __end));
        (1, 11)
    }
    fn __reduce25<
        'input,
    >(
        __lookahead_start: Option<&usize>,
        __symbols: &mut alloc::vec::Vec<(usize,__Symbol<'input>,usize)>,
        _: core::marker::PhantomData<(&'input ())>,
    ) -> (usize, usize)
    {
        // LogicalOr = LogicalOr, "||", LogicalAnd => ActionFn(4);
        assert!(__symbols.len() >= 3);
        let __sym2 = __pop_Variant2(__symbols);
        let __sym1 = __pop_Variant1(__symbols);
        let __sym0 = __pop_Variant2(__symbols);
        let __start = __sym0.0.clone();
        let __end = __sym2.2.clone();
        let __nt = super::__action4::<>(__sym0, __sym1, __sym2);
        __symbols.push((__start, __Symbol::Variant2(__nt), __end));
        (3, 12)
    }
    fn __reduce26<
        'input,
    >(
        __lookahead_start: Option<&usize>,
        __symbols: &mut alloc::vec::Vec<(usize,__Symbol<'input>,usize)>,
        _: core::marker::PhantomData<(&'input ())>,
    ) -> (usize, usize)
    {
        // LogicalOr = LogicalAnd => ActionFn(5);
        let __sym0 = __pop_Variant2(__symbols);
        let __start = __sym0.0.clone();
        let __end = __sym0.2.clone();
        let __nt = super::__action5::<>(__sym0);
        __symbols.push((__start, __Symbol::Variant2(__nt), __end));
        (1, 12)
    }
    fn __reduce27<
        'input,
    >(
        __lookahead_start: Option<&usize>,
        __symbols: &mut alloc::vec::Vec<(usize,__Symbol<'input>,usize)>,
        _: core::marker::PhantomData<(&'input ())>,
    ) -> (usize, usize)
    {
        // Multiplicative = Multiplicative, "*", Unary => ActionFn(28);
        assert!(__symbols.len() >= 3);
        let __sym2 = __pop_Variant2(__symbols);
        let __sym1 = __pop_Variant1(__symbols);
        let __sym0 = __pop_Variant2(__symbols);
        let __start = __sym0.0.clone();
        let __end = __sym2.2.clone();
        let __nt = super::__action28::<>(__sym0, __sym1, __sym2);
        __symbols.push((__start, __Symbol::Variant2(__nt), __end));
        (3, 13)
    }
    fn __reduce28<
        'input,
    >(
        __lookahead_start: Option<&usize>,
        __symbols: &mut alloc::vec::Vec<(usize,__Symbol<'input>,usize)>,
        _: core::marker::PhantomData<(&'input ())>,
    ) -> (usize, usize)
    {
        // Multiplicative = Multiplicative, "/", Unary => ActionFn(29);
        assert!(__symbols.len() >= 3);
        let __sym2 = __pop_Variant2(__symbols);
        let __sym1 = __pop_Variant1(__symbols);
        let __sym0 = __pop_Variant2(__symbols);
        let __start = __sym0.0.clone();
        let __end = __sym2.2.clone();
        let __nt = super::__action29::<>(__sym0, __sym1, __sym2);
        __symbols.push((__start, __Symbol::Variant2(__nt), __end));
        (3, 13)
    }
    fn __reduce29<
        'input,
    >(
        __lookahead_start: Option<&usize>,
        __symbols: &mut alloc::vec::Vec<(usize,__Symbol<'input>,usize)>,
        _: core::marker::PhantomData<(&'input ())>,
    ) -> (usize, usize)
    {
        // Multiplicative = Multiplicative, "%", Unary => ActionFn(30);
        assert!(__symbols.len() >= 3);
        let __sym2 = __pop_Variant2(__symbols);
        let __sym1 = __pop_Variant1(__symbols);
        let __sym0 = __pop_Variant2(__symbols);
        let __start = __sym0.0.clone();
        let __end = __sym2.2.clone();
        let __nt = super::__action30::<>(__sym0, __sym1, __sym2);
        __symbols.push((__start, __Symbol::Variant2(__nt), __end));
        (3, 13)
    }
    fn __reduce30<
        'input,
    >(
        __lookahead_start: Option<&usize>,
        __symbols: &mut alloc::vec::Vec<(usize,__Symbol<'input>,usize)>,
        _: core::marker::PhantomData<(&'input ())>,
    ) -> (usize, usize)
    {
        // Multiplicative = Unary => ActionFn(31);
        let __sym0 = __pop_Variant2(__symbols);
        let __start = __sym0.0.clone();
        let __end = __sym0.2.clone();
        let __nt = super::__action31::<>(__sym0);
        __symbols.push((__start, __Symbol::Variant2(__nt), __end));
        (1, 13)
    }
    fn __reduce31<
        'input,
    >(
        __lookahead_start: Option<&usize>,
        __symbols: &mut alloc::vec::Vec<(usize,__Symbol<'input>,usize)>,
        _: core::marker::PhantomData<(&'input ())>,
    ) -> (usize, usize)
    {
        // Postfix = Postfix, "(", CallArgs, ")" => ActionFn(39);
        assert!(__symbols.len() >= 4);
        let __sym3 = __pop_Variant1(__symbols);
        let __sym2 = __pop_Variant4(__symbols);
        let __sym1 = __pop_Variant1(__symbols);
        let __sym0 = __pop_Variant2(__symbols);
        let __start = __sym0.0.clone();
        let __end = __sym3.2.clone();
        let __nt = super::__action39::<>(__sym0, __sym1, __sym2, __sym3);
        __symbols.push((__start, __Symbol::Variant2(__nt), __end));
        (4, 14)
    }
    fn __reduce32<
        'input,
    >(
        __lookahead_start: Option<&usize>,
        __symbols: &mut alloc::vec::Vec<(usize,__Symbol<'input>,usize)>,
        _: core::marker::PhantomData<(&'input ())>,
    ) -> (usize, usize)
    {
        // Postfix = Primary => ActionFn(40);
        let __sym0 = __pop_Variant2(__symbols);
        let __start = __sym0.0.clone();
        let __end = __sym0.2.clone();
        let __nt = super::__action40::<>(__sym0);
        __symbols.push((__start, __Symbol::Variant2(__nt), __end));
        (1, 14)
    }
    fn __reduce35<
        'input,
    >(
        __lookahead_start: Option<&usize>,
        __symbols: &mut alloc::vec::Vec<(usize,__Symbol<'input>,usize)>,
        _: core::marker::PhantomData<(&'input ())>,
    ) -> (usize, usize)
    {
        // Primary = "ident" => ActionFn(45);
        let __sym0 = __pop_Variant0(__symbols);
        let __start = __sym0.0.clone();
        let __end = __sym0.2.clone();
        let __nt = super::__action45::<>(__sym0);
        __symbols.push((__start, __Symbol::Variant2(__nt), __end));
        (1, 15)
    }
    fn __reduce36<
        'input,
    >(
        __lookahead_start: Option<&usize>,
        __symbols: &mut alloc::vec::Vec<(usize,__Symbol<'input>,usize)>,
        _: core::marker::PhantomData<(&'input ())>,
    ) -> (usize, usize)
    {
        // Primary = "(", Assign, ")" => ActionFn(46);
        assert!(__symbols.len() >= 3);
        let __sym2 = __pop_Variant1(__symbols);
        let __sym1 = __pop_Variant2(__symbols);
        let __sym0 = __pop_Variant1(__symbols);
        let __start = __sym0.0.clone();
        let __end = __sym2.2.clone();
        let __nt = super::__action46::<>(__sym0, __sym1, __sym2);
        __symbols.push((__start, __Symbol::Variant2(__nt), __end));
        (3, 15)
    }
    fn __reduce37<
        'input,
    >(
        __lookahead_start: Option<&usize>,
        __symbols: &mut alloc::vec::Vec<(usize,__Symbol<'input>,usize)>,
        _: core::marker::PhantomData<(&'input ())>,
    ) -> (usize, usize)
    {
        // Relational = Relational, "<", Shift => ActionFn(17);
        assert!(__symbols.len() >= 3);
        let __sym2 = __pop_Variant2(__symbols);
        let __sym1 = __pop_Variant1(__symbols);
        let __sym0 = __pop_Variant2(__symbols);
        let __start = __sym0.0.clone();
        let __end = __sym2.2.clone();
        let __nt = super::__action17::<>(__sym0, __sym1, __sym2);
        __symbols.push((__start, __Symbol::Variant2(__nt), __end));
        (3, 16)
    }
    fn __reduce38<
        'input,
    >(
        __lookahead_start: Option<&usize>,
        __symbols: &mut alloc::vec::Vec<(usize,__Symbol<'input>,usize)>,
        _: core::marker::PhantomData<(&'input ())>,
    ) -> (usize, usize)
    {
        // Relational = Relational, ">", Shift => ActionFn(18);
        assert!(__symbols.len() >= 3);
        let __sym2 = __pop_Variant2(__symbols);
        let __sym1 = __pop_Variant1(__symbols);
        let __sym0 = __pop_Variant2(__symbols);
        let __start = __sym0.0.clone();
        let __end = __sym2.2.clone();
        let __nt = super::__action18::<>(__sym0, __sym1, __sym2);
        __symbols.push((__start, __Symbol::Variant2(__nt), __end));
        (3, 16)
    }
    fn __reduce39<
        'input,
    >(
        __lookahead_start: Option<&usize>,
        __symbols: &mut alloc::vec::Vec<(usize,__Symbol<'input>,usize)>,
        _: core::marker::PhantomData<(&'input ())>,
    ) -> (usize, usize)
    {
        // Relational = Relational, "<=", Shift => ActionFn(19);
        assert!(__symbols.len() >= 3);
        let __sym2 = __pop_Variant2(__symbols);
        let __sym1 = __pop_Variant1(__symbols);
        let __sym0 = __pop_Variant2(__symbols);
        let __start = __sym0.0.clone();
        let __end = __sym2.2.clone();
        let __nt = super::__action19::<>(__sym0, __sym1, __sym2);
        __symbols.push((__start, __Symbol::Variant2(__nt), __end));
        (3, 16)
    }
    fn __reduce40<
        'input,
    >(
        __lookahead_start: Option<&usize>,
        __symbols: &mut alloc::vec::Vec<(usize,__Symbol<'input>,usize)>,
        _: core::marker::PhantomData<(&'input ())>,
    ) -> (usize, usize)
    {
        // Relational = Relational, ">=", Shift => ActionFn(20);
        assert!(__symbols.len() >= 3);
        let __sym2 = __pop_Variant2(__symbols);
        let __sym1 = __pop_Variant1(__symbols);
        let __sym0 = __pop_Variant2(__symbols);
        let __start = __sym0.0.clone();
        let __end = __sym2.2.clone();
        let __nt = super::__action20::<>(__sym0, __sym1, __sym2);
        __symbols.push((__start, __Symbol::Variant2(__nt), __end));
        (3, 16)
    }
    fn __reduce41<
        'input,
    >(
        __lookahead_start: Option<&usize>,
        __symbols: &mut alloc::vec::Vec<(usize,__Symbol<'input>,usize)>,
        _: core::marker::PhantomData<(&'input ())>,
    ) -> (usize, usize)
    {
        // Relational = Shift => ActionFn(21);
        let __sym0 = __pop_Variant2(__symbols);
        let __start = __sym0.0.clone();
        let __end = __sym0.2.clone();
        let __nt = super::__action21::<>(__sym0);
        __symbols.push((__start, __Symbol::Variant2(__nt), __end));
        (1, 16)
    }
    fn __reduce42<
        'input,
    >(
        __lookahead_start: Option<&usize>,
        __symbols: &mut alloc::vec::Vec<(usize,__Symbol<'input>,usize)>,
        _: core::marker::PhantomData<(&'input ())>,
    ) -> (usize, usize)
    {
        // Shift = Shift, "<<", Additive => ActionFn(22);
        assert!(__symbols.len() >= 3);
        let __sym2 = __pop_Variant2(__symbols);
        let __sym1 = __pop_Variant1(__symbols);
        let __sym0 = __pop_Variant2(__symbols);
        let __start = __sym0.0.clone();
        let __end = __sym2.2.clone();
        let __nt = super::__action22::<>(__sym0, __sym1, __sym2);
        __symbols.push((__start, __Symbol::Variant2(__nt), __end));
        (3, 17)
    }
    fn __reduce43<
        'input,
    >(
        __lookahead_start: Option<&usize>,
        __symbols: &mut alloc::vec::Vec<(usize,__Symbol<'input>,usize)>,
        _: core::marker::PhantomData<(&'input ())>,
    ) -> (usize, usize)
    {
        // Shift = Shift, ">>", Additive => ActionFn(23);
        assert!(__symbols.len() >= 3);
        let __sym2 = __pop_Variant2(__symbols);
        let __sym1 = __pop_Variant1(__symbols);
        let __sym0 = __pop_Variant2(__symbols);
        let __start = __sym0.0.clone();
        let __end = __sym2.2.clone();
        let __nt = super::__action23::<>(__sym0, __sym1, __sym2);
        __symbols.push((__start, __Symbol::Variant2(__nt), __end));
        (3, 17)
    }
    fn __reduce44<
        'input,
    >(
        __lookahead_start: Option<&usize>,
        __symbols: &mut alloc::vec::Vec<(usize,__Symbol<'input>,usize)>,
        _: core::marker::PhantomData<(&'input ())>,
    ) -> (usize, usize)
    {
        // Shift = Additive => ActionFn(24);
        let __sym0 = __pop_Variant2(__symbols);
        let __start = __sym0.0.clone();
        let __end = __sym0.2.clone();
        let __nt = super::__action24::<>(__sym0);
        __symbols.push((__start, __Symbol::Variant2(__nt), __end));
        (1, 17)
    }
    fn __reduce45<
        'input,
    >(
        __lookahead_start: Option<&usize>,
        __symbols: &mut alloc::vec::Vec<(usize,__Symbol<'input>,usize)>,
        _: core::marker::PhantomData<(&'input ())>,
    ) -> (usize, usize)
    {
        // Unary = "+", Unary => ActionFn(32);
        assert!(__symbols.len() >= 2);
        let __sym1 = __pop_Variant2(__symbols);
        let __sym0 = __pop_Variant1(__symbols);
        let __start = __sym0.0.clone();
        let __end = __sym1.2.clone();
        let __nt = super::__action32::<>(__sym0, __sym1);
        __symbols.push((__start, __Symbol::Variant2(__nt), __end));
        (2, 18)
    }
    fn __reduce46<
        'input,
    >(
        __lookahead_start: Option<&usize>,
        __symbols: &mut alloc::vec::Vec<(usize,__Symbol<'input>,usize)>,
        _: core::marker::PhantomData<(&'input ())>,
    ) -> (usize, usize)
    {
        // Unary = "-", Unary => ActionFn(33);
        assert!(__symbols.len() >= 2);
        let __sym1 = __pop_Variant2(__symbols);
        let __sym0 = __pop_Variant1(__symbols);
        let __start = __sym0.0.clone();
        let __end = __sym1.2.clone();
        let __nt = super::__action33::<>(__sym0, __sym1);
        __symbols.push((__start, __Symbol::Variant2(__nt), __end));
        (2, 18)
    }
    fn __reduce47<
        'input,
    >(
        __lookahead_start: Option<&usize>,
        __symbols: &mut alloc::vec::Vec<(usize,__Symbol<'input>,usize)>,
        _: core::marker::PhantomData<(&'input ())>,
    ) -> (usize, usize)
    {
        // Unary = "!", Unary => ActionFn(34);
        assert!(__symbols.len() >= 2);
        let __sym1 = __pop_Variant2(__symbols);
        let __sym0 = __pop_Variant1(__symbols);
        let __start = __sym0.0.clone();
        let __end = __sym1.2.clone();
        let __nt = super::__action34::<>(__sym0, __sym1);
        __symbols.push((__start, __Symbol::Variant2(__nt), __end));
        (2, 18)
    }
    fn __reduce48<
        'input,
    >(
        __lookahead_start: Option<&usize>,
        __symbols: &mut alloc::vec::Vec<(usize,__Symbol<'input>,usize)>,
        _: core::marker::PhantomData<(&'input ())>,
    ) -> (usize, usize)
    {
        // Unary = "~", Unary => ActionFn(35);
        assert!(__symbols.len() >= 2);
        let __sym1 = __pop_Variant2(__symbols);
        let __sym0 = __pop_Variant1(__symbols);
        let __start = __sym0.0.clone();
        let __end = __sym1.2.clone();
        let __nt = super::__action35::<>(__sym0, __sym1);
        __symbols.push((__start, __Symbol::Variant2(__nt), __end));
        (2, 18)
    }
    fn __reduce49<
        'input,
    >(
        __lookahead_start: Option<&usize>,
        __symbols: &mut alloc::vec::Vec<(usize,__Symbol<'input>,usize)>,
        _: core::marker::PhantomData<(&'input ())>,
    ) -> (usize, usize)
    {
        // Unary = "&", Unary => ActionFn(36);
        assert!(__symbols.len() >= 2);
        let __sym1 = __pop_Variant2(__symbols);
        let __sym0 = __pop_Variant1(__symbols);
        let __start = __sym0.0.clone();
        let __end = __sym1.2.clone();
        let __nt = super::__action36::<>(__sym0, __sym1);
        __symbols.push((__start, __Symbol::Variant2(__nt), __end));
        (2, 18)
    }
    fn __reduce50<
        'input,
    >(
        __lookahead_start: Option<&usize>,
        __symbols: &mut alloc::vec::Vec<(usize,__Symbol<'input>,usize)>,
        _: core::marker::PhantomData<(&'input ())>,
    ) -> (usize, usize)
    {
        // Unary = "*", Unary => ActionFn(37);
        assert!(__symbols.len() >= 2);
        let __sym1 = __pop_Variant2(__symbols);
        let __sym0 = __pop_Variant1(__symbols);
        let __start = __sym0.0.clone();
        let __end = __sym1.2.clone();
        let __nt = super::__action37::<>(__sym0, __sym1);
        __symbols.push((__start, __Symbol::Variant2(__nt), __end));
        (2, 18)
    }
    fn __reduce51<
        'input,
    >(
        __lookahead_start: Option<&usize>,
        __symbols: &mut alloc::vec::Vec<(usize,__Symbol<'input>,usize)>,
        _: core::marker::PhantomData<(&'input ())>,
    ) -> (usize, usize)
    {
        // Unary = Postfix => ActionFn(38);
        let __sym0 = __pop_Variant2(__symbols);
        let __start = __sym0.0.clone();
        let __end = __sym0.2.clone();
        let __nt = super::__action38::<>(__sym0);
        __symbols.push((__start, __Symbol::Variant2(__nt), __end));
        (1, 18)
    }
}
#[allow(unused_imports)]
pub use self::__parse__Expression::ExpressionParser;

#[allow(clippy::too_many_arguments, clippy::needless_lifetimes, clippy::just_underscores_and_digits)]
fn __action0<
    'input,
>(
    (_, __0, _): (usize, Expr, usize),
) -> Expr
{
    __0
}

#[allow(clippy::too_many_arguments, clippy::needless_lifetimes, clippy::just_underscores_and_digits)]
fn __action1<
    'input,
>(
    (_, e, _): (usize, Expr, usize),
) -> Expr
{
    e
}

#[allow(clippy::too_many_arguments, clippy::needless_lifetimes, clippy::just_underscores_and_digits)]
fn __action2<
    'input,
>(
    (_, l, _): (usize, Expr, usize),
    (_, _, _): (usize, ExprToken<'input>, usize),
    (_, r, _): (usize, Expr, usize),
) -> Expr
{
    Expr::Assign(Box::new(l), Box::new(r))
}

#[allow(clippy::too_many_arguments, clippy::needless_lifetimes, clippy::just_underscores_and_digits)]
fn __action3<
    'input,
>(
    (_, __0, _): (usize, Expr, usize),
) -> Expr
{
    __0
}

#[allow(clippy::too_many_arguments, clippy::needless_lifetimes, clippy::just_underscores_and_digits)]
fn __action4<
    'input,
>(
    (_, l, _): (usize, Expr, usize),
    (_, _, _): (usize, ExprToken<'input>, usize),
    (_, r, _): (usize, Expr, usize),
) -> Expr
{
    Expr::Binary(BinOp::Or, Box::new(l), Box::new(r))
}

#[allow(clippy::too_many_arguments, clippy::needless_lifetimes, clippy::just_underscores_and_digits)]
fn __action5<
    'input,
>(
    (_, __0, _): (usize, Expr, usize),
) -> Expr
{
    __0
}

#[allow(clippy::too_many_arguments, clippy::needless_lifetimes, clippy::just_underscores_and_digits)]
fn __action6<
    'input,
>(
    (_, l, _): (usize, Expr, usize),
    (_, _, _): (usize, ExprToken<'input>, usize),
    (_, r, _): (usize, Expr, usize),
) -> Expr
{
    Expr::Binary(BinOp::And, Box::new(l), Box::new(r))
}

#[allow(clippy::too_many_arguments, clippy::needless_lifetimes, clippy::just_underscores_and_digits)]
fn __action7<
    'input,
>(
    (_, __0, _): (usize, Expr, usize),
) -> Expr
{
    __0
}

#[allow(clippy::too_many_arguments, clippy::needless_lifetimes, clippy::just_underscores_and_digits)]
fn __action8<
    'input,
>(
    (_, l, _): (usize, Expr, usize),
    (_, _, _): (usize, ExprToken<'input>, usize),
    (_, r, _): (usize, Expr, usize),
) -> Expr
{
    Expr::Binary(BinOp::BitOr, Box::new(l), Box::new(r))
}

#[allow(clippy::too_many_arguments, clippy::needless_lifetimes, clippy::just_underscores_and_digits)]
fn __action9<
    'input,
>(
    (_, __0, _): (usize, Expr, usize),
) -> Expr
{
    __0
}

#[allow(clippy::too_many_arguments, clippy::needless_lifetimes, clippy::just_underscores_and_digits)]
fn __action10<
    'input,
>(
    (_, l, _): (usize, Expr, usize),
    (_, _, _): (usize, ExprToken<'input>, usize),
    (_, r, _): (usize, Expr, usize),
) -> Expr
{
    Expr::Binary(BinOp::BitXor, Box::new(l), Box::new(r))
}

#[allow(clippy::too_many_arguments, clippy::needless_lifetimes, clippy::just_underscores_and_digits)]
fn __action11<
    'input,
>(
    (_, __0, _): (usize, Expr, usize),
) -> Expr
{
    __0
}

#[allow(clippy::too_many_arguments, clippy::needless_lifetimes, clippy::just_underscores_and_digits)]
fn __action12<
    'input,
>(
    (_, l, _): (usize, Expr, usize),
    (_, _, _): (usize, ExprToken<'input>, usize),
    (_, r, _): (usize, Expr, usize),
) -> Expr
{
    Expr::Binary(BinOp::BitAnd, Box::new(l), Box::new(r))
}

#[allow(clippy::too_many_arguments, clippy::needless_lifetimes, clippy::just_underscores_and_digits)]
fn __action13<
    'input,
>(
    (_, __0, _): (usize, Expr, usize),
) -> Expr
{
    __0
}

#[allow(clippy::too_many_arguments, clippy::needless_lifetimes, clippy::just_underscores_and_digits)]
fn __action14<
    'input,
>(
    (_, l, _): (usize, Expr, usize),
    (_, _, _): (usize, ExprToken<'input>, usize),
    (_, r, _): (usize, Expr, usize),
) -> Expr
{
    Expr::Binary(BinOp::Eq, Box::new(l), Box::new(r))
}

#[allow(clippy::too_many_arguments, clippy::needless_lifetimes, clippy::just_underscores_and_digits)]
fn __action15<
    'input,
>(
    (_, l, _): (usize, Expr, usize),
    (_, _, _): (usize, ExprToken<'input>, usize),
    (_, r, _): (usize, Expr, usize),
) -> Expr
{
    Expr::Binary(BinOp::Ne, Box::new(l), Box::new(r))
}

#[allow(clippy::too_many_arguments, clippy::needless_lifetimes, clippy::just_underscores_and_digits)]
fn __action16<
    'input,
>(
    (_, __0, _): (usize, Expr, usize),
) -> Expr
{
    __0
}

#[allow(clippy::too_many_arguments, clippy::needless_lifetimes, clippy::just_underscores_and_digits)]
fn __action17<
    'input,
>(
    (_, l, _): (usize, Expr, usize),
    (_, _, _): (usize, ExprToken<'input>, usize),
    (_, r, _): (usize, Expr, usize),
) -> Expr
{
    Expr::Binary(BinOp::Lt, Box::new(l), Box::new(r))
}

#[allow(clippy::too_many_arguments, clippy::needless_lifetimes, clippy::just_underscores_and_digits)]
fn __action18<
    'input,
>(
    (_, l, _): (usize, Expr, usize),
    (_, _, _): (usize, ExprToken<'input>, usize),
    (_, r, _): (usize, Expr, usize),
) -> Expr
{
    Expr::Binary(BinOp::Gt, Box::new(l), Box::new(r))
}

#[allow(clippy::too_many_arguments, clippy::needless_lifetimes, clippy::just_underscores_and_digits)]
fn __action19<
    'input,
>(
    (_, l, _): (usize, Expr, usize),
    (_, _, _): (usize, ExprToken<'input>, usize),
    (_, r, _): (usize, Expr, usize),
) -> Expr
{
    Expr::Binary(BinOp::Le, Box::new(l), Box::new(r))
}

#[allow(clippy::too_many_arguments, clippy::needless_lifetimes, clippy::just_underscores_and_digits)]
fn __action20<
    'input,
>(
    (_, l, _): (usize, Expr, usize),
    (_, _, _): (usize, ExprToken<'input>, usize),
    (_, r, _): (usize, Expr, usize),
) -> Expr
{
    Expr::Binary(BinOp::Ge, Box::new(l), Box::new(r))
}

#[allow(clippy::too_many_arguments, clippy::needless_lifetimes, clippy::just_underscores_and_digits)]
fn __action21<
    'input,
>(
    (_, __0, _): (usize, Expr, usize),
) -> Expr
{
    __0
}

#[allow(clippy::too_many_arguments, clippy::needless_lifetimes, clippy::just_underscores_and_digits)]
fn __action22<
    'input,
>(
    (_, l, _): (usize, Expr, usize),
    (_, _, _): (usize, ExprToken<'input>, usize),
    (_, r, _): (usize, Expr, usize),
) -> Expr
{
    Expr::Binary(BinOp::Shl, Box::new(l), Box::new(r))
}

#[allow(clippy::too_many_arguments, clippy::needless_lifetimes, clippy::just_underscores_and_digits)]
fn __action23<
    'input,
>(
    (_, l, _): (usize, Expr, usize),
    (_, _, _): (usize, ExprToken<'input>, usize),
    (_, r, _): (usize, Expr, usize),
) -> Expr
{
    Expr::Binary(BinOp::Shr, Box::new(l), Box::new(r))
}

#[allow(clippy::too_many_arguments, clippy::needless_lifetimes, clippy::just_underscores_and_digits)]
fn __action24<
    'input,
>(
    (_, __0, _): (usize, Expr, usize),
) -> Expr
{
    __0
}

#[allow(clippy::too_many_arguments, clippy::needless_lifetimes, clippy::just_underscores_and_digits)]
fn __action25<
    'input,
>(
    (_, l, _): (usize, Expr, usize),
    (_, _, _): (usize, ExprToken<'input>, usize),
    (_, r, _): (usize, Expr, usize),
) -> Expr
{
    Expr::Binary(BinOp::Add, Box::new(l), Box::new(r))
}

#[allow(clippy::too_many_arguments, clippy::needless_lifetimes, clippy::just_underscores_and_digits)]
fn __action26<
    'input,
>(
    (_, l, _): (usize, Expr, usize),
    (_, _, _): (usize, ExprToken<'input>, usize),
    (_, r, _): (usize, Expr, usize),
) -> Expr
{
    Expr::Binary(BinOp::Sub, Box::new(l), Box::new(r))
}

#[allow(clippy::too_many_arguments, clippy::needless_lifetimes, clippy::just_underscores_and_digits)]
fn __action27<
    'input,
>(
    (_, __0, _): (usize, Expr, usize),
) -> Expr
{
    __0
}

#[allow(clippy::too_many_arguments, clippy::needless_lifetimes, clippy::just_underscores_and_digits)]
fn __action28<
    'input,
>(
    (_, l, _): (usize, Expr, usize),
    (_, _, _): (usize, ExprToken<'input>, usize),
    (_, r, _): (usize, Expr, usize),
) -> Expr
{
    Expr::Binary(BinOp::Mul, Box::new(l), Box::new(r))
}

#[allow(clippy::too_many_arguments, clippy::needless_lifetimes, clippy::just_underscores_and_digits)]
fn __action29<
    'input,
>(
    (_, l, _): (usize, Expr, usize),
    (_, _, _): (usize, ExprToken<'input>, usize),
    (_, r, _): (usize, Expr, usize),
) -> Expr
{
    Expr::Binary(BinOp::Div, Box::new(l), Box::new(r))
}

#[allow(clippy::too_many_arguments, clippy::needless_lifetimes, clippy::just_underscores_and_digits)]
fn __action30<
    'input,
>(
    (_, l, _): (usize, Expr, usize),
    (_, _, _): (usize, ExprToken<'input>, usize),
    (_, r, _): (usize, Expr, usize),
) -> Expr
{
    Expr::Binary(BinOp::Mod, Box::new(l), Box::new(r))
}

#[allow(clippy::too_many_arguments, clippy::needless_lifetimes, clippy::just_underscores_and_digits)]
fn __action31<
    'input,
>(
    (_, __0, _): (usize, Expr, usize),
) -> Expr
{
    __0
}

#[allow(clippy::too_many_arguments, clippy::needless_lifetimes, clippy::just_underscores_and_digits)]
fn __action32<
    'input,
>(
    (_, _, _): (usize, ExprToken<'input>, usize),
    (_, e, _): (usize, Expr, usize),
) -> Expr
{
    Expr::Unary(UnOp::Plus, Box::new(e))
}

#[allow(clippy::too_many_arguments, clippy::needless_lifetimes, clippy::just_underscores_and_digits)]
fn __action33<
    'input,
>(
    (_, _, _): (usize, ExprToken<'input>, usize),
    (_, e, _): (usize, Expr, usize),
) -> Expr
{
    Expr::Unary(UnOp::Neg, Box::new(e))
}

#[allow(clippy::too_many_arguments, clippy::needless_lifetimes, clippy::just_underscores_and_digits)]
fn __action34<
    'input,
>(
    (_, _, _): (usize, ExprToken<'input>, usize),
    (_, e, _): (usize, Expr, usize),
) -> Expr
{
    Expr::Unary(UnOp::Not, Box::new(e))
}

#[allow(clippy::too_many_arguments, clippy::needless_lifetimes, clippy::just_underscores_and_digits)]
fn __action35<
    'input,
>(
    (_, _, _): (usize, ExprToken<'input>, usize),
    (_, e, _): (usize, Expr, usize),
) -> Expr
{
    Expr::Unary(UnOp::BitNot, Box::new(e))
}

#[allow(clippy::too_many_arguments, clippy::needless_lifetimes, clippy::just_underscores_and_digits)]
fn __action36<
    'input,
>(
    (_, _, _): (usize, ExprToken<'input>, usize),
    (_, e, _): (usize, Expr, usize),
) -> Expr
{
    Expr::Unary(UnOp::Address, Box::new(e))
}

#[allow(clippy::too_many_arguments, clippy::needless_lifetimes, clippy::just_underscores_and_digits)]
fn __action37<
    'input,
>(
    (_, _, _): (usize, ExprToken<'input>, usize),
    (_, e, _): (usize, Expr, usize),
) -> Expr
{
    Expr::Unary(UnOp::Deref, Box::new(e))
}

#[allow(clippy::too_many_arguments, clippy::needless_lifetimes, clippy::just_underscores_and_digits)]
fn __action38<
    'input,
>(
    (_, __0, _): (usize, Expr, usize),
) -> Expr
{
    __0
}

#[allow(clippy::too_many_arguments, clippy::needless_lifetimes, clippy::just_underscores_and_digits)]
fn __action39<
    'input,
>(
    (_, callee, _): (usize, Expr, usize),
    (_, _, _): (usize, ExprToken<'input>, usize),
    (_, args, _): (usize, Vec<Expr>, usize),
    (_, _, _): (usize, ExprToken<'input>, usize),
) -> Expr
{
    Expr::Call(Box::new(callee), args)
}

#[allow(clippy::too_many_arguments, clippy::needless_lifetimes, clippy::just_underscores_and_digits)]
fn __action40<
    'input,
>(
    (_, __0, _): (usize, Expr, usize),
) -> Expr
{
    __0
}

#[allow(clippy::too_many_arguments, clippy::needless_lifetimes, clippy::just_underscores_and_digits)]
fn __action41<
    'input,
>(
    __lookbehind: &usize,
    __lookahead: &usize,
) -> Vec<Expr>
{
    vec![]
}

#[allow(clippy::too_many_arguments, clippy::needless_lifetimes, clippy::just_underscores_and_digits)]
fn __action42<
    'input,
>(
    (_, first, _): (usize, Expr, usize),
    (_, rest, _): (usize, alloc::vec::Vec<Expr>, usize),
) -> Vec<Expr>
{
    {
        let mut args = vec![first];
        args.extend(rest);
        args
    }
}

#[allow(clippy::too_many_arguments, clippy::needless_lifetimes, clippy::just_underscores_and_digits)]
fn __action43<
    'input,
>(
    (_, n, _): (usize, &'input str, usize),
) -> Result<Expr,__lalrpop_util::ParseError<usize,ExprToken<'input>,&'static str>>
{
    n.parse::<u64>().map(Expr::Int).map_err(|_| lalrpop_util::ParseError::User { error: "integer literal exceeds u64 range" })
}

#[allow(clippy::too_many_arguments, clippy::needless_lifetimes, clippy::just_underscores_and_digits)]
fn __action44<
    'input,
>(
    (_, literal, _): (usize, &'input str, usize),
) -> Result<Expr,__lalrpop_util::ParseError<usize,ExprToken<'input>,&'static str>>
{
    crate::char_literal::parse_ascii_character(literal).map(Expr::Int).map_err(|reason| lalrpop_util::ParseError::User { error: reason })
}

#[allow(clippy::too_many_arguments, clippy::needless_lifetimes, clippy::just_underscores_and_digits)]
fn __action45<
    'input,
>(
    (_, n, _): (usize, &'input str, usize),
) -> Expr
{
    Expr::Name(n.to_owned())
}

#[allow(clippy::too_many_arguments, clippy::needless_lifetimes, clippy::just_underscores_and_digits)]
fn __action46<
    'input,
>(
    (_, _, _): (usize, ExprToken<'input>, usize),
    (_, e, _): (usize, Expr, usize),
    (_, _, _): (usize, ExprToken<'input>, usize),
) -> Expr
{
    e
}

#[allow(clippy::too_many_arguments, clippy::needless_lifetimes, clippy::just_underscores_and_digits)]
fn __action47<
    'input,
>(
    __lookbehind: &usize,
    __lookahead: &usize,
) -> alloc::vec::Vec<Expr>
{
    alloc::vec![]
}

#[allow(clippy::too_many_arguments, clippy::needless_lifetimes, clippy::just_underscores_and_digits)]
fn __action48<
    'input,
>(
    (_, v, _): (usize, alloc::vec::Vec<Expr>, usize),
) -> alloc::vec::Vec<Expr>
{
    v
}

#[allow(clippy::too_many_arguments, clippy::needless_lifetimes, clippy::just_underscores_and_digits)]
fn __action49<
    'input,
>(
    (_, _, _): (usize, ExprToken<'input>, usize),
    (_, __0, _): (usize, Expr, usize),
) -> Expr
{
    __0
}

#[allow(clippy::too_many_arguments, clippy::needless_lifetimes, clippy::just_underscores_and_digits)]
fn __action50<
    'input,
>(
    (_, __0, _): (usize, Expr, usize),
) -> alloc::vec::Vec<Expr>
{
    alloc::vec![__0]
}

#[allow(clippy::too_many_arguments, clippy::needless_lifetimes, clippy::just_underscores_and_digits)]
fn __action51<
    'input,
>(
    (_, v, _): (usize, alloc::vec::Vec<Expr>, usize),
    (_, e, _): (usize, Expr, usize),
) -> alloc::vec::Vec<Expr>
{
    { let mut v = v; v.push(e); v }
}

#[allow(clippy::too_many_arguments, clippy::needless_lifetimes,
    clippy::just_underscores_and_digits)]
fn __action52<
    'input,
>(
    __0: (usize, ExprToken<'input>, usize),
    __1: (usize, Expr, usize),
) -> alloc::vec::Vec<Expr>
{
    let __start0 = __0.0;
    let __end0 = __1.2;
    let __temp0 = __action49(
        __0,
        __1,
    );
    let __temp0 = (__start0, __temp0, __end0);
    __action50(
        __temp0,
    )
}

#[allow(clippy::too_many_arguments, clippy::needless_lifetimes,
    clippy::just_underscores_and_digits)]
fn __action53<
    'input,
>(
    __0: (usize, alloc::vec::Vec<Expr>, usize),
    __1: (usize, ExprToken<'input>, usize),
    __2: (usize, Expr, usize),
) -> alloc::vec::Vec<Expr>
{
    let __start0 = __1.0;
    let __end0 = __2.2;
    let __temp0 = __action49(
        __1,
        __2,
    );
    let __temp0 = (__start0, __temp0, __end0);
    __action51(
        __0,
        __temp0,
    )
}

#[allow(clippy::too_many_arguments, clippy::needless_lifetimes,
    clippy::just_underscores_and_digits)]
fn __action54<
    'input,
>(
    __0: (usize, Expr, usize),
) -> Vec<Expr>
{
    let __start0 = __0.2;
    let __end0 = __0.2;
    let __temp0 = __action47(
        &__start0,
        &__end0,
    );
    let __temp0 = (__start0, __temp0, __end0);
    __action42(
        __0,
        __temp0,
    )
}

#[allow(clippy::too_many_arguments, clippy::needless_lifetimes,
    clippy::just_underscores_and_digits)]
fn __action55<
    'input,
>(
    __0: (usize, Expr, usize),
    __1: (usize, alloc::vec::Vec<Expr>, usize),
) -> Vec<Expr>
{
    let __start0 = __1.0;
    let __end0 = __1.2;
    let __temp0 = __action48(
        __1,
    );
    let __temp0 = (__start0, __temp0, __end0);
    __action42(
        __0,
        __temp0,
    )
}

#[allow(clippy::type_complexity, dead_code)]
pub trait __ToTriple<'input, >
{
    fn to_triple(self) -> Result<(usize,ExprToken<'input>,usize), __lalrpop_util::ParseError<usize, ExprToken<'input>, &'static str>>;
}

impl<'input, > __ToTriple<'input, > for (usize, ExprToken<'input>, usize)
{
    fn to_triple(self) -> Result<(usize,ExprToken<'input>,usize), __lalrpop_util::ParseError<usize, ExprToken<'input>, &'static str>> {
        Ok(self)
    }
}
impl<'input, > __ToTriple<'input, > for Result<(usize, ExprToken<'input>, usize), &'static str>
{
    fn to_triple(self) -> Result<(usize,ExprToken<'input>,usize), __lalrpop_util::ParseError<usize, ExprToken<'input>, &'static str>> {
        self.map_err(|error| __lalrpop_util::ParseError::User { error })
    }
}
