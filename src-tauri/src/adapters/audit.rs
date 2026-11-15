use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::Path,
};
// Callers supply only static operation/result codes. Never serialize requests.
pub fn record(dir: &Path, operation: &'static str, result: &'static str, duration_ms: u128) {
    let path = dir.join("audit.jsonl");
    if fs::metadata(&path)
        .map(|m| m.len() > 5 * 1024 * 1024 - 1024)
        .unwrap_or(false)
    {
        let previous = dir.join("audit.previous.jsonl");
        let _ = fs::remove_file(&previous);
        let _ = fs::rename(&path, previous);
    }
    let mut options = OpenOptions::new();
    options.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    if let Ok(mut file) = options.open(path) {
        let _ = writeln!(
            file,
            "{{\"operation\":\"{operation}\",\"result\":\"{result}\",\"durationMs\":{duration_ms}}}"
        );
    }
}
