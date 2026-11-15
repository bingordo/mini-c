//! Bounded, cancellation-aware internal CPP token delivery.
//!
//! This is an opt-in bridge for incremental parser integration. The worker
//! owns all rcc session state, while the consumer owns individual spellings;
//! no source-map borrow or whole expanded output String crosses threads.
use super::{options_from_args, MAX_OUTPUT_BYTES, MAX_SOURCE_BYTES};
use rcc_errors::Level;
use rcc_lexer::PpTokenKind;
use rcc_session::Session;
use std::collections::{HashMap, VecDeque};
use std::ffi::OsString;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc};
use std::thread::{self, JoinHandle};

const TOKENS_PER_BATCH: usize = 128;
const CHANNEL_BATCHES: usize = 2;

/// An expanded token with a stable owned spelling and its logical origin.
/// Source-map buffers can be released without invalidating this token.
#[derive(Debug, Clone)]
pub struct OwnedPpToken {
    pub spelling: Arc<str>,
    pub file: Arc<str>,
    pub line: u32,
    pub column: usize,
}

enum Message {
    Tokens(Vec<OwnedPpToken>),
    Error(String),
}

/// One-shot bounded stream of independently owned expanded tokens.
///
/// At most two queued batches of 128 tokens plus the consumer's current
/// batch are held by the channel. Active macro expansions and source-map
/// buffers still have their existing separate limits.
pub struct TokenStream {
    receiver: Option<mpsc::Receiver<Message>>,
    pending: VecDeque<OwnedPpToken>,
    worker: Option<JoinHandle<()>>,
}

impl Iterator for TokenStream {
    type Item = Result<OwnedPpToken, String>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if let Some(token) = self.pending.pop_front() {
                return Some(Ok(token));
            }
            let message = self.receiver.as_ref()?.recv().ok()?;
            match message {
                Message::Tokens(tokens) => self.pending = tokens.into(),
                Message::Error(message) => {
                    // This is terminal; do not emit any more tokens after
                    // a fatal preprocessing or output-budget diagnostic.
                    self.receiver.take();
                    return Some(Err(message));
                }
            }
        }
    }
}

impl Drop for TokenStream {
    fn drop(&mut self) {
        // Disconnect the receiver FIRST. A worker blocked on the bounded
        // channel then observes SendError, requests preprocessor cancel,
        // and exits before the join. Joining with receiver live deadlocks.
        self.receiver.take();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

/// Start bounded, pull-consumable preprocessing. The only permitted worker
/// is a per-invocation OS thread; dropping the iterator stops it.
pub fn start(path: &Path, args: &[OsString]) -> Result<TokenStream, String> {
    let path = path.to_path_buf();
    let args = args.to_vec();
    let (sender, receiver) = mpsc::sync_channel(CHANNEL_BATCHES);
    let worker = thread::Builder::new()
        .name("mini-c-internal-cpp".to_owned())
        .spawn(move || {
            // Keep one sender alive until the worker has reported a final
            // failure (or dropped it on success).
            if let Err(error) = produce(&path, &args, &sender) {
                let _ = sender.send(Message::Error(error));
            }
        })
        .map_err(|err| format!("cannot launch internal CPP worker: {err}"))?;
    Ok(TokenStream {
        receiver: Some(receiver),
        pending: VecDeque::new(),
        worker: Some(worker),
    })
}

fn produce(path: &Path, args: &[OsString], sender: &mpsc::SyncSender<Message>)
    -> Result<(), String>
{
    let opts = options_from_args(args)?;
    let (mut session, captured) = Session::for_test();
    session.opts = opts;
    // Maintain the current root/source budgets. Streaming output tokens
    // alone does not make the underlying source map a streaming reader.
    let mut root_bytes = Vec::new();
    std::fs::File::open(path)
        .map_err(|e| format!("cannot read C source {}: {e}", path.display()))?
        .take((MAX_SOURCE_BYTES + 1) as u64)
        .read_to_end(&mut root_bytes)
        .map_err(|e| format!("cannot read C source {}: {e}", path.display()))?;
    if root_bytes.len() > MAX_SOURCE_BYTES {
        return Err(format!(
            "preprocessing source byte limit exceeded ({} bytes) for {}",
            MAX_SOURCE_BYTES, path.display()
        ));
    }
    let source = String::from_utf8(root_bytes)
        .map_err(|e| format!("cannot decode C source {}: {e}", path.display()))?;
    let root = session.source_map.write()
        .map_err(|_| "internal source map lock poisoned")?
        .add_file(path.to_path_buf(), Arc::from(source));

    let mut pp = rcc_preprocess::Preprocessor::new(&mut session);
    let mut by_file: HashMap<u32, Arc<str>> = HashMap::new();
    let mut buffered = Vec::with_capacity(TOKENS_PER_BATCH);
    let mut emitted_bytes = 0usize;
    let mut output_error = None;
    let mut disconnected = false;

    pp.run_with_context_sink(root, &mut |context, chunk| {
        if output_error.is_some() || disconnected {
            context.request_cancel();
            return;
        }
        let map = match context.session.source_map.read() {
            Ok(map) => map,
            Err(e) => {
                output_error = Some(format!("internal source map lock poisoned: {e}"));
                context.request_cancel();
                return;
            }
        };
        for token in chunk {
            if matches!(token.kind,
                PpTokenKind::Whitespace | PpTokenKind::Newline | PpTokenKind::Eof)
            {
                continue;
            }
            let physical = map.file(token.span.file);
            let start = token.span.lo.0 as usize;
            let end = token.span.hi.0 as usize;
            let Some(spelling) = physical.src.get(start..end) else {
                output_error = Some("preprocessor returned an invalid token span".into());
                context.request_cancel();
                break;
            };
            emitted_bytes = emitted_bytes.saturating_add(spelling.len().saturating_add(1));
            if emitted_bytes > MAX_OUTPUT_BYTES {
                output_error = Some("internal preprocessor output exceeded 16 MiB limit".into());
                context.request_cancel();
                break;
            }
            let logical_file = context.line_overrides.effective_file(
                &map, token.span.file, token.span.lo,
            );
            let file = by_file.entry(logical_file.0).or_insert_with(|| {
                Arc::<str>::from(map.file(logical_file).name.to_string_lossy().as_ref())
            }).clone();
            let line = context.line_overrides.effective_line(
                &map, token.span.file, token.span.lo,
            );
            let column = map.lookup_line_col(token.span.file, token.span.lo).col as usize;
            buffered.push(OwnedPpToken {
                spelling: Arc::from(spelling), file, line, column,
            });
            if buffered.len() == TOKENS_PER_BATCH &&
                sender.send(Message::Tokens(std::mem::take(&mut buffered))).is_err()
            {
                disconnected = true;
                context.request_cancel();
                break;
            }
        }
    });
    if disconnected {
        // The parser has been dropped (possibly after a syntax error).
        return Ok(());
    }
    if let Some(error) = output_error {
        return Err(error);
    }
    let errors: Vec<String> = {
        let map = pp.session.source_map.read()
            .map_err(|e| format!("internal source map lock poisoned: {e}"))?;
        captured.diagnostics().into_iter()
            .filter(|d| matches!(d.level, Level::Error | Level::Bug))
            .map(|d| {
                let location = d.labels.iter().find(|label| label.primary)
                    .or_else(|| d.labels.first())
                    .and_then(|label| {
                        if label.span.file.0 == u32::MAX { return None; }
                        let name = pp.line_overrides.effective_file(
                            &map, label.span.file, label.span.lo,
                        );
                        let line = pp.line_overrides.effective_line(
                            &map, label.span.file, label.span.lo,
                        );
                        Some(format!("{}:{line}: ", map.file(name).name.display()))
                    }).unwrap_or_default();
                format!("{location}{}", d.message)
            }).collect()
    };
    if !errors.is_empty() {
        return Err(errors.join("\n"));
    }
    if !buffered.is_empty() {
        let _ = sender.send(Message::Tokens(buffered));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn directory() -> PathBuf {
        let unique = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let dir = std::env::temp_dir().join(
            format!("mini-c-cpp-stream-{}-{unique}", std::process::id())
        );
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn bounded_stream_expands_nested_macro_and_includes() {
        let dir = directory();
        let root = dir.join("main.c");
        std::fs::write(dir.join("value.h"), "#define PLUS(x) ((x)+1)\n").unwrap();
        std::fs::write(&root, "#include \"value.h\"\nint answer=PLUS\n(41);\n").unwrap();
        let stream = start(&root, &[OsString::from("-E"), OsString::from("-nostdinc")]).unwrap();
        let output = stream.map(|t| t.unwrap().spelling.to_string())
            .collect::<Vec<_>>().join(" ");
        assert!(output.contains("int answer = ( ( 41 ) + 1 ) ;"), "{output}");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn late_error_fails_even_after_valid_tokens_were_delivered() {
        let dir = directory();
        let root = dir.join("late.c");
        std::fs::write(&root, "int first;\n#define X 1\nint second;\n#error late_failure\n").unwrap();
        let mut stream = start(&root, &[OsString::from("-E"), OsString::from("-nostdinc")]).unwrap();
        let mut seen = 0;
        let mut error = None;
        for token in &mut stream {
            match token {
                Ok(_) => seen += 1,
                Err(message) => { error = Some(message); break; }
            }
        }
        assert!(seen > 0);
        assert!(error.expect("late error must be delivered").contains("late_failure"));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn dropping_stream_does_not_deadlock_a_full_queue() {
        let dir = directory();
        let root = dir.join("many.c");
        let mut text = String::new();
        for i in 0..2_000 { text.push_str(&format!("int value_{i} = {i};\n")); }
        std::fs::write(&root, text).unwrap();
        let mut stream = start(&root, &[OsString::from("-E"), OsString::from("-nostdinc")]).unwrap();
        assert!(stream.next().is_some());
        drop(stream);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
