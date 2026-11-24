//! Source-side GNU GP output role planning, independent of target mnemonics.
//!
//! The checked Mini-C frontend owns C lvalue and operand grammar acceptance.
//! Two independent register writes may be normal `=r` or early `=&r`.
//! Their distinct physical registers and the independent input staging are
//! separately proven by compiler's target-owned machine contract. A tie,
//! read/write output, constraint alternative, memory role or extra operand
//! never gets silently mapped to this GP-only plan.

/// Source roles for one dual-GP statement with its independent register input.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DualGpOutputPlan {
    pub early_clobber: [bool;2],
}

/// Admit precisely two GP write outputs and one independent GP input.
///
/// The result captures source early-clobber flags; it does NOT select
/// physical registers or assert that unknown target instructions are legal.
pub fn plan_dual_gp(outputs:&[&str],inputs:&[&str])->Option<DualGpOutputPlan> {
    if outputs.len()!=2 || inputs!=["r"] {return None;}
    let role=|text:&str|match text {
        "=r"=>Some(false),
        "=&r"=>Some(true),
        _=>None,
    };
    Some(DualGpOutputPlan {
        early_clobber:[role(outputs[0])?,role(outputs[1])?],
    })
}
