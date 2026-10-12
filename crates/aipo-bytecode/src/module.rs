//! Binary format and in-memory representation of compiled Aipo bytecode modules.

use crate::opcode::Constant;
use aipo_source::SourceSpan;
use serde::{Deserialize, Serialize};

/// Magic header bytes for Aipo Bytecode: `AIBC`.
pub const AIBC_MAGIC: [u8; 4] = *b"AIBC";
/// Current bytecode version.
pub const AIBC_VERSION: u16 = 1;

/// Metadata for one compiled function body in the module.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FunctionInfo {
    /// Function name; `__top_level__` for the module entry script and `Type.method`
    /// for `impl` methods.
    pub name: String,
    /// Absolute byte offset of the function entry point (before the slot prologue).
    pub entry_ip: usize,
    /// Number of declared parameters, which is also the expected call arity.
    pub params: usize,
    /// Number of declared locals after the parameters.
    pub locals: usize,
    /// Number of captured upvalues.
    #[serde(default)]
    pub upvalues: usize,
    /// `true` for `async fn`: calling produces a `Task` instead of running.
    pub is_async: bool,
}

/// A compiled, self-contained Aipo bytecode module.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BytecodeModule {
    /// Magic header bytes (`AIBC`).
    pub magic: [u8; 4],
    /// Module format version.
    pub version: u16,
    /// Constant pool.
    pub constants: Vec<Constant>,
    /// Interned symbol/identifier names.
    pub names: Vec<String>,
    /// Bytecode instructions.
    pub code: Vec<u8>,
    /// Source span mapping for instruction byte offsets.
    pub spans: Vec<(usize, SourceSpan)>,
    /// Compiled function table, indexed by the operand of `MakeFunction`/`MakeClosure`.
    pub functions: Vec<FunctionInfo>,
    /// Declared struct layouts, used to materialize instances with canonical field names.
    pub structs: Vec<StructInfo>,
}

/// Layout metadata for one declared struct type.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StructInfo {
    /// Struct type name.
    pub name: String,
    /// Declared fields in canonical order: `(field name, is_fixed)`.
    pub fields: Vec<(String, bool)>,
}

use byteorder::{BigEndian, ReadBytesExt, WriteBytesExt};
use std::io::{self, Error, ErrorKind, Read, Write};

impl BytecodeModule {
    /// Constructs a new empty bytecode module with valid magic and version.
    #[must_use]
    pub fn new() -> Self {
        Self {
            magic: AIBC_MAGIC,
            version: AIBC_VERSION,
            constants: Vec::new(),
            names: Vec::new(),
            code: Vec::new(),
            spans: Vec::new(),
            functions: Vec::new(),
            structs: Vec::new(),
        }
    }

    /// Looks up a compiled function by its canonical name.
    #[must_use]
    pub fn function_by_name(&self, name: &str) -> Option<&FunctionInfo> {
        self.functions.iter().rev().find(|f| f.name == name)
    }

    /// Serializes the module into the binary `.aibc` format.
    ///
    /// # Errors
    /// Returns an I/O error if writing fails.
    pub fn write_to<W: Write>(&self, writer: &mut W) -> io::Result<()> {
        writer.write_all(&self.magic)?;
        writer.write_u16::<BigEndian>(self.version)?;

        // Names
        writer.write_u32::<BigEndian>(self.names.len() as u32)?;
        for name in &self.names {
            writer.write_u32::<BigEndian>(name.len() as u32)?;
            writer.write_all(name.as_bytes())?;
        }

        // Constants
        writer.write_u32::<BigEndian>(self.constants.len() as u32)?;
        for c in &self.constants {
            match c {
                Constant::Nil => writer.write_u8(0)?,
                Constant::Bool(b) => {
                    writer.write_u8(1)?;
                    writer.write_u8(u8::from(*b))?;
                }
                Constant::Int(n) => {
                    writer.write_u8(2)?;
                    writer.write_i64::<BigEndian>(*n)?;
                }
                Constant::Float(f) => {
                    writer.write_u8(3)?;
                    writer.write_f64::<BigEndian>(*f)?;
                }
                Constant::String(s) => {
                    writer.write_u8(4)?;
                    writer.write_u32::<BigEndian>(s.len() as u32)?;
                    writer.write_all(s.as_bytes())?;
                }
            }
        }

        // Structs
        writer.write_u32::<BigEndian>(self.structs.len() as u32)?;
        for s in &self.structs {
            writer.write_u32::<BigEndian>(s.name.len() as u32)?;
            writer.write_all(s.name.as_bytes())?;
            writer.write_u32::<BigEndian>(s.fields.len() as u32)?;
            for (fname, fixed) in &s.fields {
                writer.write_u32::<BigEndian>(fname.len() as u32)?;
                writer.write_all(fname.as_bytes())?;
                writer.write_u8(u8::from(*fixed))?;
            }
        }

        // Functions
        writer.write_u32::<BigEndian>(self.functions.len() as u32)?;
        for f in &self.functions {
            writer.write_u32::<BigEndian>(f.name.len() as u32)?;
            writer.write_all(f.name.as_bytes())?;
            writer.write_u64::<BigEndian>(f.entry_ip as u64)?;
            writer.write_u32::<BigEndian>(f.params as u32)?;
            writer.write_u32::<BigEndian>(f.locals as u32)?;
            writer.write_u32::<BigEndian>(f.upvalues as u32)?;
            writer.write_u8(u8::from(f.is_async))?;
        }

        // Code
        writer.write_u32::<BigEndian>(self.code.len() as u32)?;
        writer.write_all(&self.code)?;

        // Spans
        writer.write_u32::<BigEndian>(self.spans.len() as u32)?;
        for (offset, span) in &self.spans {
            writer.write_u64::<BigEndian>(*offset as u64)?;
            writer.write_u64::<BigEndian>(span.start as u64)?;
            writer.write_u64::<BigEndian>(span.end as u64)?;
        }

        Ok(())
    }

    /// Deserializes a module from the binary `.aibc` format.
    ///
    /// # Errors
    /// Returns an I/O error if reading fails, or `ErrorKind::InvalidData` if the
    /// header, version, or contents are malformed.
    pub fn read_from<R: Read>(reader: &mut R) -> io::Result<Self> {
        let mut magic = [0u8; 4];
        reader.read_exact(&mut magic)?;
        if magic != AIBC_MAGIC {
            return Err(Error::new(
                ErrorKind::InvalidData,
                format!("invalid AIBC magic: expected {AIBC_MAGIC:?}, got {magic:?}"),
            ));
        }

        let version = reader.read_u16::<BigEndian>()?;
        if version != AIBC_VERSION {
            return Err(Error::new(
                ErrorKind::InvalidData,
                format!("unsupported AIBC version: expected {AIBC_VERSION}, got {version}"),
            ));
        }

        // Names
        let name_count = reader.read_u32::<BigEndian>()? as usize;
        let mut names = Vec::with_capacity(name_count);
        for _ in 0..name_count {
            let len = reader.read_u32::<BigEndian>()? as usize;
            let mut buf = vec![0u8; len];
            reader.read_exact(&mut buf)?;
            let s = String::from_utf8(buf).map_err(|e| Error::new(ErrorKind::InvalidData, e))?;
            names.push(s);
        }

        // Constants
        let const_count = reader.read_u32::<BigEndian>()? as usize;
        let mut constants = Vec::with_capacity(const_count);
        for _ in 0..const_count {
            let tag = reader.read_u8()?;
            let c = match tag {
                0 => Constant::Nil,
                1 => Constant::Bool(reader.read_u8()? != 0),
                2 => Constant::Int(reader.read_i64::<BigEndian>()?),
                3 => Constant::Float(reader.read_f64::<BigEndian>()?),
                4 => {
                    let len = reader.read_u32::<BigEndian>()? as usize;
                    let mut buf = vec![0u8; len];
                    reader.read_exact(&mut buf)?;
                    let s = String::from_utf8(buf)
                        .map_err(|e| Error::new(ErrorKind::InvalidData, e))?;
                    Constant::String(s)
                }
                other => {
                    return Err(Error::new(
                        ErrorKind::InvalidData,
                        format!("invalid constant tag: {other}"),
                    ));
                }
            };
            constants.push(c);
        }

        // Structs
        let struct_count = reader.read_u32::<BigEndian>()? as usize;
        let mut structs = Vec::with_capacity(struct_count);
        for _ in 0..struct_count {
            let nlen = reader.read_u32::<BigEndian>()? as usize;
            let mut nbuf = vec![0u8; nlen];
            reader.read_exact(&mut nbuf)?;
            let name =
                String::from_utf8(nbuf).map_err(|e| Error::new(ErrorKind::InvalidData, e))?;

            let field_count = reader.read_u32::<BigEndian>()? as usize;
            let mut fields = Vec::with_capacity(field_count);
            for _ in 0..field_count {
                let flen = reader.read_u32::<BigEndian>()? as usize;
                let mut fbuf = vec![0u8; flen];
                reader.read_exact(&mut fbuf)?;
                let fname =
                    String::from_utf8(fbuf).map_err(|e| Error::new(ErrorKind::InvalidData, e))?;
                let fixed = reader.read_u8()? != 0;
                fields.push((fname, fixed));
            }
            structs.push(StructInfo { name, fields });
        }

        // Functions
        let func_count = reader.read_u32::<BigEndian>()? as usize;
        let mut functions = Vec::with_capacity(func_count);
        for _ in 0..func_count {
            let nlen = reader.read_u32::<BigEndian>()? as usize;
            let mut nbuf = vec![0u8; nlen];
            reader.read_exact(&mut nbuf)?;
            let name =
                String::from_utf8(nbuf).map_err(|e| Error::new(ErrorKind::InvalidData, e))?;
            let entry_ip = reader.read_u64::<BigEndian>()? as usize;
            let params = reader.read_u32::<BigEndian>()? as usize;
            let locals = reader.read_u32::<BigEndian>()? as usize;
            let upvalues = reader.read_u32::<BigEndian>()? as usize;
            let is_async = reader.read_u8()? != 0;
            functions.push(FunctionInfo {
                name,
                entry_ip,
                params,
                locals,
                upvalues,
                is_async,
            });
        }

        // Code
        let code_len = reader.read_u32::<BigEndian>()? as usize;
        let mut code = vec![0u8; code_len];
        reader.read_exact(&mut code)?;

        // Spans
        let span_count = reader.read_u32::<BigEndian>()? as usize;
        let mut spans = Vec::with_capacity(span_count);
        for _ in 0..span_count {
            let offset = reader.read_u64::<BigEndian>()? as usize;
            let start = reader.read_u64::<BigEndian>()? as usize;
            let end = reader.read_u64::<BigEndian>()? as usize;
            spans.push((offset, SourceSpan::new(start, end)));
        }

        Ok(Self {
            magic,
            version,
            constants,
            names,
            code,
            spans,
            functions,
            structs,
        })
    }

    /// Serializes the module to an in-memory byte buffer.
    #[must_use]
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::new();
        self.write_to(&mut bytes)
            .expect("in-memory write must not fail");
        bytes
    }

    /// Deserializes a module from an in-memory byte slice.
    ///
    /// # Errors
    /// Returns an I/O error if the buffer is malformed.
    pub fn from_bytes(mut bytes: &[u8]) -> io::Result<Self> {
        Self::read_from(&mut bytes)
    }
}

impl Default for BytecodeModule {
    fn default() -> Self {
        Self::new()
    }
}
