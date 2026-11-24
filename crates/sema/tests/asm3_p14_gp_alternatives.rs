//! P14 GNU multi-alternative GP planning contracts.
//!
//! One coherent alternative is selected across both outputs and the input.
//! The GP target's physical binder remains unchanged, and every source
//! mnemonic is opaque to this planner.
use mini_c_sema::asm_output_roles::{plan_dual_gp,DualGpOutputPlan};

#[test]
fn first_compatible_gp_alternative_selected_for_whole_statement() {
    let cases=[
        (["=r,r","=r,&r"],"r,r",0,[false,false]),
        (["=m,r","=m,&r"],"r,r",1,[false,true]),
        (["=m,&r,r","=m,m,&r"],"r,r,r",2,[false,true]),
        (["=&r,r","=r,&r"],"r,r",0,[true,false]),
        (["=r,&r","=&r,r"],"r,r",0,[false,true]),
        (["=m,&r","=r,r"],"r,r",1,[true,false]),
    ];
    for (outputs,input,alternative,early_clobber) in cases {
        let plan=plan_dual_gp(&outputs,&[input]).expect("compatible full GP alternative");
        assert_eq!(plan,DualGpOutputPlan{early_clobber,alternative,input_tie:None});
        for (index,expected) in early_clobber.iter().enumerate() {
            assert_eq!(plan.output_constraint(index),
                if *expected {"=&r"} else {"=r"});
        }
    }
}

#[test]
fn constraint_counts_and_unsupported_tied_input_alternatives_fail_closed() {
    let invalid=[
        (["=m,r","=m,r"],"r"),
        (["=m,r","=m"],"r,r"),
        (["=r,r","=r,r"],"r"),
        (["=r,r","=r,r"],"r,r,r"),
        (["=m,m","=r,r"],"r,r"),
        (["=r,,r","=r,r,r"],"r,r,r"),
        (["=r,r,","=r,r,r"],"r,r,r"),
        (["=&r,&r","=r,r"],"0,0"),
        (["=r,r","=&r,&r"],"1,1"),
        (["=r,&r","=r,r"],"r,m"),
        (["=rm,r","=r,r"],"r,r"),
        (["=r,r","=r,r"],"r,rm"),
        (["=r,r","=r,r"],"r,"),
        (["=r,r","=r,r"],"r,r,m"),
        (["=r,r","=r,r"],"r,r,"),
        (["=r,r","=r,r"],"r,,r"),
        (["=r,r","=r,r"],"r\tr"),
    ];
    for (outputs,input) in invalid {
        assert!(plan_dual_gp(&outputs,&[input]).is_none(),
            "unexpected alternative plan for {outputs:?}, {input}");
    }
}
