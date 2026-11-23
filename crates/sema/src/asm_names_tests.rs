//! Independent checks of GNU symbolic operand name to positional rewriting.
use super::asm_names::positional_template;

#[test]
fn symbolic_outputs_inputs_modifiers_and_escapes() {
    let names=[Some("result"),Some("left"),Some("right")];
    assert_eq!(
        positional_template(
            "movl %[left], %[result]; addl %k[right],%k[result]; xorl %2,%0; movl %%eax,%%ecx",
            &names,
        ).unwrap(),
        "movl %1, %0; addl %k2,%k0; xorl %2,%0; movl %%eax,%%ecx",
    );
    assert_eq!(positional_template("%[result] %q[left] %b[right]",&names).unwrap(),
        "%0 %q1 %b2");
    assert_eq!(positional_template("%%[left] %1 %[right]",&names).unwrap(),
        "%%[left] %1 %2");
    assert_eq!(positional_template("nop",&[None,None]).unwrap(),"nop");
}

#[test]
fn symbolic_names_fail_closed_without_rebinding_or_guesswork() {
    for (template, names) in [
        ("movl %[missing],%0",vec![Some("output")]),
        ("movl %[output],%0",vec![Some("output"),Some("output")]),
        ("movl %[bad-name],%0",vec![Some("bad-name")]),
        ("movl %[unterminated,%0",vec![Some("output")]),
        ("movl %[],%0",vec![Some("output")]),
        ("movl %[output.%0",vec![Some("output")]),
    ] {
        assert!(positional_template(template,&names).is_err(),
            "bad named operand admitted: {template:?} {names:?}");
    }
    // Unknown modifiers are preserved for backend rejection, never guessed.
    assert_eq!(positional_template("%z[operand]",&[Some("operand")]).unwrap(),"%z0");
}
