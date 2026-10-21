use std::{env, fs, path::PathBuf, process, time::{SystemTime, UNIX_EPOCH}};
fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = env::args().skip(1);
    let mode = args.next().ok_or("expected --write or --check")?;
    if !matches!(mode.as_str(), "--write" | "--check") || args.next().is_some() {
        return Err("usage: parser-codegen --write | --check".into());
    }
    let source_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../crates/syntax/src").canonicalize()?;
    let checked_in = source_dir.join("grammar.rs");
    let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let temp = env::temp_dir().join(format!("mini-c-lalrpop-{}-{nonce}", process::id()));
    fs::create_dir(&temp)?;
    let result = (|| -> Result<(), Box<dyn std::error::Error>> {
        env::set_current_dir(&source_dir)?;
        lalrpop::Configuration::new()
            .never_use_colors()
            .set_out_dir(&temp)
            .force_build(true)
            .process_file("grammar.lalrpop")?;
        let generated = fs::read(temp.join("grammar.rs"))?;
        if mode == "--write" {
            fs::write(&checked_in, &generated)?;
            println!("Generated {} ({} bytes)", checked_in.display(), generated.len());
        } else if fs::read(&checked_in).ok().as_deref() != Some(generated.as_slice()) {
            return Err("parser is stale; run --write and commit grammar.rs".into());
        } else {
            println!("Checked-in parser matches the grammar");
        }
        Ok(())
    })();
    let _ = fs::remove_dir_all(temp);
    result
}
fn main() {
    if let Err(e) = run() { eprintln!("parser-codegen: {e}"); process::exit(1); }
}
