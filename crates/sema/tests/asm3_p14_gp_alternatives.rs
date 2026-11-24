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

#[test]
fn numeric_ties_select_the_named_non_early_output() {
    let cases=[
        (["=r","=&r"],"0",0,Some(0),[false,true]),
        (["=&r","=r"],"1",0,Some(1),[true,false]),
        (["=m,r","=m,&r"],"r,0",1,Some(0),[false,true]),
        (["=&r,r","=r,r"],"0,1",1,Some(1),[false,false]),
        (["=r,&r","=r,r"],"1,r",0,Some(1),[false,false]),
    ];
    for (outputs,input,alternative,input_tie,early_clobber) in cases {
        let plan=plan_dual_gp(&outputs,&[input]).expect("valid tied alternative");
        assert_eq!(plan,DualGpOutputPlan{
            alternative,input_tie,early_clobber,
        });
        assert_eq!(plan.input_constraint(),match input_tie {
            Some(0)=>"0",Some(1)=>"1",None=>"r",_=>unreachable!(),
        });
    }
}
