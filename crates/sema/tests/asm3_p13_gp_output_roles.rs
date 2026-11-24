//! Source grammar matrix for GNU two-output GP write roles.
//! These tests do not parse mnemonics or silently admit matching alternatives.
use mini_c_sema::asm_output_roles::{plan_dual_gp,DualGpOutputPlan};

#[test]
fn both_output_roles_are_preserved_without_register_assignment() {
    for a in [false,true] {
        for b in [false,true] {
            let outputs=[if a {"=&r"}else{"=r"},if b {"=&r"}else{"=r"}];
            assert_eq!(
                plan_dual_gp(&outputs,&["r"]),
                Some(DualGpOutputPlan{early_clobber:[a,b],alternative:0})
            );
        }
    }
}

#[test]
fn input_ties_readwrite_memory_alternatives_and_extras_fail_closed() {
    for outputs in [
        ["=r","+r"], ["=&r","=m"],["=r","+m"],
        ["=r","=rm"], ["=r","=r,m"],["=&r","=&r,r"],
        ["=r","0"],["+r","=&r"],["=&m","=&r"],
    ] {
        assert!(plan_dual_gp(&outputs,&["r"]).is_none(),
            "unexpected dual-GP source plan: {outputs:?}");
    }
    for inputs in [
        vec![],vec!["0"],vec!["1"],vec!["m"],
        vec!["r","r"],vec!["rm"],vec!["r,m"],
    ] {
        assert!(plan_dual_gp(&["=r","=&r"],&inputs).is_none());
    }
    assert!(plan_dual_gp(&["=r"],&["r"]).is_none());
    assert!(plan_dual_gp(&["=r","=&r","=r"],&["r"]).is_none());
}
