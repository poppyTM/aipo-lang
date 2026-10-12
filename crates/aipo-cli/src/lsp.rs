//! LSP 3.17 over stdio: full document sync, diagnostics, completion and formatting.
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    io::{self, BufRead, Read, Write},
    path::PathBuf,
};

const MAX_MESSAGE: usize = 8 * 1024 * 1024;
struct Document {
    text: String,
    version: i64,
}
fn send(out: &mut dyn Write, message: Value) -> io::Result<()> {
    let bytes = serde_json::to_vec(&message)?;
    write!(out, "Content-Length: {}\r\n\r\n", bytes.len())?;
    out.write_all(&bytes)?;
    out.flush()
}
fn response(out: &mut dyn Write, id: Value, result: Value) -> io::Result<()> {
    send(out, json!({"jsonrpc":"2.0","id":id,"result":result}))
}
fn error(out: &mut dyn Write, id: Value, code: i32, message: &str) -> io::Result<()> {
    send(
        out,
        json!({"jsonrpc":"2.0","id":id,"error":{"code":code,"message":message}}),
    )
}
fn read(input: &mut impl BufRead) -> io::Result<Option<Result<Value, serde_json::Error>>> {
    let mut length = None;
    let mut total = 0;
    loop {
        let mut line = String::new();
        let bytes = (&mut *input).take(8193).read_line(&mut line)?;
        if bytes == 0 {
            return if total == 0 {
                Ok(None)
            } else {
                Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "truncated LSP header",
                ))
            };
        }
        total += bytes;
        if total > 8192 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "LSP header exceeds 8192 bytes",
            ));
        }
        if line == "\r\n" || line == "\n" {
            break;
        }
        if let Some((name, value)) = line.split_once(':')
            && name.eq_ignore_ascii_case("Content-Length")
        {
            if length.is_some() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "duplicate Content-Length",
                ));
            }
            length = Some(value.trim().parse::<usize>().map_err(|_| {
                io::Error::new(io::ErrorKind::InvalidData, "invalid Content-Length")
            })?);
        }
    }
    let length = length
        .filter(|length| *length <= MAX_MESSAGE)
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "missing or oversized Content-Length",
            )
        })?;
    let mut bytes = vec![0; length];
    input.read_exact(&mut bytes)?;
    Ok(Some(serde_json::from_slice(&bytes)))
}
fn position(text: &str, offset: usize) -> Value {
    let offset = offset.min(text.len());
    let mut boundary = offset;
    while !text.is_char_boundary(boundary) {
        boundary -= 1;
    }
    let prefix = &text[..boundary];
    let line = prefix.bytes().filter(|byte| *byte == b'\n').count();
    let tail = prefix.rsplit('\n').next().unwrap_or("");
    json!({"line":line,"character":tail.encode_utf16().count()})
}
fn file_path(uri: &str) -> Option<PathBuf> {
    let uri = uri.strip_prefix("file://")?;
    let uri = uri.strip_prefix("localhost").unwrap_or(uri);
    if !uri.starts_with('/') {
        return None;
    }
    let mut bytes = Vec::new();
    let mut input = uri.as_bytes().iter().copied();
    while let Some(byte) = input.next() {
        if byte == b'%' {
            let a = (input.next()? as char).to_digit(16)?;
            let b = (input.next()? as char).to_digit(16)?;
            bytes.push((a * 16 + b) as u8);
        } else {
            bytes.push(byte);
        }
    }
    let decoded = String::from_utf8(bytes).ok()?;
    #[cfg(windows)]
    let decoded = decoded.strip_prefix('/').unwrap_or(&decoded).to_string();
    Some(decoded.into())
}
fn publish(out: &mut dyn Write, uri: &str, doc: &Document) -> io::Result<()> {
    let source = aipo_source::Source::new(aipo_source::SourceId::next(), uri, &doc.text);
    let path = file_path(uri).unwrap_or_else(|| PathBuf::from("<lsp>"));
    let compiled = match crate::resolve_local_package_paths(&path, None, Some(doc.text.as_bytes()))
    {
        Ok(paths) => crate::analyze(&source, &path, paths.as_ref()),
        Err(error) => crate::Compiled {
            module: aipo_bytecode::BytecodeModule::new(),
            diagnostics: vec![match error {
                crate::CliError::Diagnostic(diagnostic) => *diagnostic,
                crate::CliError::Usage(message) => aipo_diagnostics::Diagnostic::error(
                    aipo_diagnostics::DiagnosticCode::AIPO_PKG_RESOLUTION,
                    message,
                ),
            }],
        },
    };
    let diagnostics:Vec<_>=compiled.diagnostics.iter().map(|diagnostic| {
        let span=diagnostic.primary_span.as_ref().filter(|span| span.file == source.name()).map(|span|aipo_source::SourceSpan::new(span.start,span.end)).unwrap_or_default();
        let message = match &diagnostic.primary_span {
            Some(span) if span.file != source.name() => format!("{}:{}:{}: {}", span.file, span.line, span.column, diagnostic.message),
            _ => diagnostic.message.clone(),
        };
        json!({"range":{"start":position(&doc.text,span.start),"end":position(&doc.text,span.end)},"severity":match diagnostic.severity {aipo_diagnostics::Severity::Error=>1,aipo_diagnostics::Severity::Warning=>2,_=>3},"code":diagnostic.code.to_string(),"source":"aipo","message":message})
    }).collect();
    send(
        out,
        json!({"jsonrpc":"2.0","method":"textDocument/publishDiagnostics","params":{"uri":uri,"version":doc.version,"diagnostics":diagnostics}}),
    )
}
fn completions(doc: Option<&Document>) -> Value {
    let mut names: Vec<_> = crate::prelude_surface()
        .iter()
        .map(|(name, _)| name.to_string())
        .collect();
    names.extend(
        [
            "let", "var", "fn", "struct", "if", "else", "each", "in", "return", "attempt",
            "failed", "match", "async", "await",
        ]
        .map(str::to_string),
    );
    if let Some(doc) = doc {
        let source =
            aipo_source::Source::new(aipo_source::SourceId::next(), "<completion>", &doc.text);
        let (tokens, _) = aipo_lexer::Lexer::new(&source).tokenize();
        for pair in tokens.windows(2) {
            if matches!(
                pair[0].kind,
                aipo_lexer::TokenKind::Let
                    | aipo_lexer::TokenKind::Var
                    | aipo_lexer::TokenKind::Fn
                    | aipo_lexer::TokenKind::Struct
            ) && let aipo_lexer::TokenKind::Identifier(name) = &pair[1].kind
            {
                names.push(name.clone());
            }
        }
    }
    names.sort();
    names.dedup();
    json!({"isIncomplete":false,"items":names.into_iter().map(|label|json!({"label":label,"kind":6})).collect::<Vec<_>>()})
}
pub(crate) fn serve(mut input: impl BufRead, out: &mut dyn Write) -> io::Result<u8> {
    let mut documents = HashMap::<String, Document>::new();
    let mut initialized = false;
    let mut shutdown = false;
    while let Some(message) = read(&mut input)? {
        let message = match message {
            Ok(message) => message,
            Err(_) => {
                error(out, Value::Null, -32700, "Parse error")?;
                continue;
            }
        };
        if !message.is_object()
            || message["jsonrpc"] != "2.0"
            || !message["method"].is_string()
            || message
                .get("id")
                .is_some_and(|id| !id.is_null() && !id.is_string() && !id.is_number())
        {
            error(out, Value::Null, -32600, "Invalid Request")?;
            continue;
        }
        let id = message.get("id").cloned();
        let method = message.get("method").and_then(Value::as_str).unwrap_or("");
        let params = &message["params"];
        if method == "exit" {
            return Ok(if shutdown { 0 } else { 1 });
        }
        if shutdown {
            if let Some(id) = id {
                error(out, id, -32600, "Server has shut down")?;
            }
            continue;
        }
        if method == "initialize" {
            if initialized {
                if let Some(id) = id {
                    error(out, id, -32600, "Already initialized")?;
                }
                continue;
            }
            initialized = true;
            if let Some(id) = id {
                response(
                    out,
                    id,
                    json!({"capabilities":{"positionEncoding":"utf-16","textDocumentSync":{"openClose":true,"change":1},"completionProvider":{"resolveProvider":false},"documentFormattingProvider":cfg!(feature="formatter")},"serverInfo":{"name":"aipo","version":env!("CARGO_PKG_VERSION")}}),
                )?;
            }
            continue;
        }
        if !initialized {
            if let Some(id) = id {
                error(out, id, -32002, "Server not initialized")?;
            }
            continue;
        }
        let uri = params["textDocument"]["uri"].as_str().unwrap_or("");
        match method {
            "initialized" | "$/cancelRequest" | "$/setTrace" => {}
            "shutdown" => {
                shutdown = true;
                if let Some(id) = id {
                    response(out, id, Value::Null)?;
                }
            }
            "textDocument/didOpen" => {
                if let (Some(text), Some(version)) = (
                    params["textDocument"]["text"].as_str(),
                    params["textDocument"]["version"].as_i64(),
                ) {
                    let doc = Document {
                        text: text.into(),
                        version,
                    };
                    publish(out, uri, &doc)?;
                    documents.insert(uri.into(), doc);
                }
            }
            "textDocument/didChange" => {
                if let Some(doc) = documents.get_mut(uri)
                    && let Some(version) = params["textDocument"]["version"].as_i64()
                {
                    if version <= doc.version {
                        continue;
                    }
                    if let Some(changes) = params["contentChanges"].as_array() {
                        for change in changes {
                            if change.get("range").is_none()
                                && let Some(text) = change["text"].as_str()
                            {
                                doc.text = text.into();
                            }
                        }
                        doc.version = version;
                        publish(out, uri, doc)?;
                    }
                }
            }
            "textDocument/didSave" => {}
            "textDocument/didClose" => {
                documents.remove(uri);
                send(
                    out,
                    json!({"jsonrpc":"2.0","method":"textDocument/publishDiagnostics","params":{"uri":uri,"diagnostics":[]}}),
                )?;
            }
            "textDocument/completion" => {
                if let Some(id) = id {
                    response(out, id, completions(documents.get(uri)))?;
                }
            }
            "textDocument/formatting" => {
                if let Some(id) = id {
                    #[cfg(feature = "formatter")]
                    if let Some(doc) = documents.get(uri) {
                        match aipo_formatter::format_text(uri, &doc.text) {
                            Ok(text) => {
                                response(
                                    out,
                                    id,
                                    json!([{"range":{"start":{"line":0,"character":0},"end":position(&doc.text,doc.text.len())},"newText":text}]),
                                )?;
                            }
                            Err(error_message) => {
                                error(out, id, -32602, &error_message.to_string())?;
                            }
                        }
                        continue;
                    }
                    response(out, id, Value::Null)?;
                }
            }
            _ => {
                if let Some(id) = id {
                    error(out, id, -32601, "Method not supported")?;
                }
            }
        }
    }
    Ok(if shutdown { 0 } else { 1 })
}
