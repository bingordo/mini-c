use mini_c_fuzz::{generate, parse_case};
use std::env;
use std::process::ExitCode;
fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    let seed = match args.next().as_deref().unwrap_or("1").parse::<u64>() {
        Ok(seed) => seed,
        Err(_) => { eprintln!("usage: mini-c-fuzz [seed] [steps]"); return ExitCode::FAILURE; }
    };
    let steps = match args.next().as_deref().unwrap_or("100").parse::<usize>() {
        Ok(n) if n <= 10000 => n,
        _ => { eprintln!("steps must be at most 10000"); return ExitCode::FAILURE; }
    };
    let case = generate(seed, steps);
    if let Err(e) = parse_case(&case) {
        eprintln!("seed {seed}: {e}");
        return ExitCode::FAILURE;
    }
    print!("{}", case.source);
    ExitCode::SUCCESS
}
