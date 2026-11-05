//! Isolated index comparison, not a desktop IDE memory benchmark.
use ide_core::NullSink;
use ide_index::ProjectIndex;
use ide_language::{LanguageRegistry, Syntax};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    assert!(args.len() >= 2, "resource_probe <project> <cache> [--files-only]");
    let root = PathBuf::from(&args[0]);
    let cache = PathBuf::from(&args[1]);
    let symbols = !args.iter().any(|a| a == "--files-only");
    let start = Instant::now();
    let index = ProjectIndex::open_with_symbols(
        &root,
        &cache,
        Vec::new(),
        vec!["**/node_modules".into(), "**/target".into(), "**/dist".into()],
        Arc::new(LanguageRegistry::load(None).unwrap()),
        Arc::new(Syntax::new()),
        Arc::new(NullSink),
        symbols,
    )
    .unwrap();
    assert!(index.wait_idle(Duration::from_secs(180)), "index timed out");
    let status = index.status();
    println!(
        "{{\"files\":{},\"symbols\":{},\"elapsed_ms\":{:.2}}}",
        status.files,
        status.symbols,
        start.elapsed().as_secs_f64() * 1000.0
    );
    let mut line = String::new();
    std::io::stdin().read_line(&mut line).unwrap();
    drop(index);
}
