//! Multiline input and bounded shell history.
use aipo_lexer::{Lexer, TokenKind};
use aipo_source::{Source, SourceId};
use std::io::{self, Write};
use std::path::PathBuf;

const MAX_HISTORY_BYTES: usize = 8 * 1024 * 1024;

/// Checks delimiter balance using the language lexer, ignoring strings and comments.
pub fn is_complete(text: &str) -> bool {
    let source = Source::new(SourceId::next(), "<repl>", text);
    let (tokens, diagnostics) = Lexer::new(&source).tokenize();
    let mut depth = 0i64;
    for token in tokens {
        match token.kind {
            TokenKind::LBrace | TokenKind::LParen | TokenKind::LBracket => depth += 1,
            TokenKind::RBrace | TokenKind::RParen | TokenKind::RBracket => depth -= 1,
            _ => {}
        }
    }
    depth <= 0
        && !diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code.to_string().contains("UNTERMINATED"))
}
/// History contains at most 1,000 source units. An empty AIPO_HISTORY_FILE disables storage.
pub struct History {
    path: Option<PathBuf>,
    entries: Vec<String>,
}
impl History {
    /// Reads JSON lines, retaining only valid source strings.
    pub fn load() -> Self {
        let path = match std::env::var_os("AIPO_HISTORY_FILE") {
            Some(path) if path.is_empty() => None,
            Some(path) => Some(path.into()),
            None => std::env::var_os("XDG_STATE_HOME")
                .map(PathBuf::from)
                .or_else(|| {
                    std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/state"))
                })
                .map(|dir| dir.join("aipo/history.jsonl")),
        };
        let entries = path
            .as_ref()
            .and_then(|path| {
                use std::io::Read;
                let mut text = String::new();
                std::fs::File::open(path)
                    .ok()?
                    .take(MAX_HISTORY_BYTES as u64 + 1)
                    .read_to_string(&mut text)
                    .ok()?;
                (text.len() <= MAX_HISTORY_BYTES).then_some(text)
            })
            .map(|text| {
                text.lines()
                    .filter_map(|line| serde_json::from_str::<String>(line).ok())
                    .collect()
            })
            .unwrap_or_default();
        let mut result = Self { path, entries };
        result.trim();
        result
    }
    fn trim(&mut self) {
        if self.entries.len() > 1000 {
            self.entries.drain(..self.entries.len() - 1000);
        }
        let mut bytes: usize = self.entries.iter().map(String::len).sum();
        let mut remove = 0;
        while bytes > MAX_HISTORY_BYTES {
            bytes -= self.entries[remove].len();
            remove += 1;
        }
        self.entries.drain(..remove);
    }
    /// Appends a completed unit, suppressing adjacent duplicates.
    pub fn push(&mut self, text: String) {
        if self.entries.last() != Some(&text) {
            self.entries.push(text);
            self.trim();
        }
    }
    /// Source units in chronological order.
    pub fn entries(&self) -> &[String] {
        &self.entries
    }
    /// Persists atomically with private file permissions on Unix.
    pub fn save(&self) -> io::Result<()> {
        let Some(path) = &self.path else {
            return Ok(());
        };
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            std::fs::create_dir_all(parent)?;
        }
        let temporary = path.with_extension(format!("{}.tmp", std::process::id()));
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temporary)?;
        let result = (|| {
            for entry in &self.entries {
                serde_json::to_writer(&mut file, entry)?;
                file.write_all(b"\n")?;
            }
            file.sync_all()?;
            drop(file);
            std::fs::rename(&temporary, path)
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(&temporary);
        }
        result
    }
}
