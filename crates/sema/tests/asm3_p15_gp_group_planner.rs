//! P15 source-only GNU GP group planner tests: arbitrary output count,
 //! whole-statement alternatives and checked matching indices. Physical
 //! register capacity is target-owned and not an arbitrary C syntax limit.
use mini_c_sema::asm_output_roles::plan_gp_output_group;

#[test]
fn selects_group_alternative_and_a_high_index_tie_without_source_cap() {
    for n in [3usize,4,6,7,8,32,128] {
        let outputs=(0..n).map(|i|
            if i==n-1 {"=m,&r"} else {"=m,r"}
        ).collect::<Vec<_>>();
        let input=format!("r,{}",n-2);
        let plan=plan_gp_output_group(&outputs,&[&input]).unwrap();
        assert_eq!(plan.alternative,1);
        assert_eq!(plan.input_tie,Some(n-2));
        assert_eq!(plan.early_clobber.len(),n);
        assert_eq!(plan.input_constraint(),(n-2).to_string());
        assert!(plan.early_clobber[n-1]);
        assert!(plan.early_clobber[..n-1].iter().all(|x|!*x));
    }
    let plan=plan_gp_output_group(&["=&r","=r","=&r"],&["r"]).unwrap();
    assert_eq!(plan.early_clobber,[true,false,true]);
    assert_eq!(plan.input_tie,None);
}

#[test]
fn incompatible_alternatives_or_invalid_matching_are_rejected() {
    for (outputs,input) in [
        (vec!["=&r","=&r","=&r"],"2"),
        (vec!["=r","=&r","=r"],"1"),
        (vec!["=r","=r","=r"],"3"),
        (vec!["=r,r","=r,r","=r"],"r,r"),
        (vec!["=r,r","=r,r","=r,r"],"r"),
        (vec!["=r,,r","=r,r,r","=r,r,r"],"r,r,r"),
        (vec!["=m,m","=m,m","=m,m"],"r,r"),
        (vec!["=r","=rm","=r"],"r"),
        (vec!["=r","=r","=r"],"r,0"),
    ]{
        assert!(plan_gp_output_group(&outputs,&[input]).is_none(),
            "invalid source group admitted: {outputs:?}/{input}");
    }
    assert!(plan_gp_output_group(&["=r","=r"],&["r"]).is_none());
    assert!(plan_gp_output_group(&["=r","=r","=r"],&["r","r"]).is_none());
}
