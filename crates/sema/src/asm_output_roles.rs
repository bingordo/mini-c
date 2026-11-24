//! Deterministic source-side planning for GNU multi-alternative GP outputs.
//!
//! GNU output '=' applies to the whole alternative list. Per-alternative
//! '&' modifies early-clobber. We select ONE common alternative index for
//! both outputs and the input, never independently by operand and never by
//! decoding the instruction template. A checked GP alternative is lowered to
//! the existing dual-definition machine operation. Alternatives that require
//! an unsupported memory machine binder remain unselected; malformed input
//! syntax and mismatched alternative counts fail closed.
//!
//! This is source admission, not a machine register assignment. The compiler
//! independently checks normalized roles at the TypeId boundary.

/// Source roles of a selected two-result GP alternative.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DualGpOutputPlan {
    pub early_clobber: [bool;2],
    /// Zero-based index shared by every GNU source operand.
    pub alternative:usize,
    /// GNU matching input "0" or "1" names this output; None is "r".
    pub input_tie:Option<u8>,
}

impl DualGpOutputPlan {
    /// Return the exact existing checked-machine output constraint for index.
    pub fn output_constraint(self,index:usize)->&'static str {
        if self.early_clobber[index] {"=&r"} else {"=r"}
    }
    /// Return the selected independent or matching GNU input role.
    pub fn input_constraint(self)->&'static str {
        match self.input_tie {
            None=>"r",
            Some(0)=>"0",
            Some(1)=>"1",
            _=>unreachable!("source planner only selects output indices 0 or 1"),
        }
    }
}

/// Select the earliest fully compatible GP register alternative.
///
/// Output alternatives have global '=' followed by comma-separated 'r',
/// '&r', or 'm'. The 'm' option is allowed to remain *unselected* only;
/// it never authorizes memory emission. An input has a corresponding 'r',
/// '0' or '1' alternative. Numeric ties require a non-early GP output at
/// that index, and the target must stage the input into that very output.
/// The alternative list widths must match;
/// empty, malformed and unsupported combinations fail closed.
pub fn plan_dual_gp(outputs:&[&str],inputs:&[&str])->Option<DualGpOutputPlan> {
    if outputs.len()!=2 || inputs.len()!=1 {return None;}
    let output_modes=outputs.iter().map(|source|{
        let tail=source.strip_prefix('=')?;
        let modes=tail.split(',').collect::<Vec<_>>();
        if modes.is_empty() ||
            modes.iter().any(|m|!matches!(*m,"r"|"&r"|"m")) {
            return None;
        }
        Some(modes)
    }).collect::<Option<Vec<_>>>()?;
    let input_modes=inputs[0].split(',').collect::<Vec<_>>();
    let count=input_modes.len();
    if count==0 || output_modes.iter().any(|m|m.len()!=count)
        || input_modes.iter().any(|m|!matches!(*m,"r"|"0"|"1"))
    {
        return None;
    }
    (0..count).find_map(|alternative|{
        let role=|text:&str|match text {
            "r"=>Some(false),
            "&r"=>Some(true),
            _=>None,
        };
        let early_clobber=[
            role(output_modes[0][alternative])?,
            role(output_modes[1][alternative])?,
        ];
        let input_tie=match input_modes[alternative] {
            "r"=>None,
            "0"=>Some(0),
            "1"=>Some(1),
            _=>return None,
        };
        if input_tie.is_some_and(|i|early_clobber[i as usize]) {
            return None;
        }
        Some(DualGpOutputPlan {
            early_clobber,alternative,input_tie,
        })
    })
}
