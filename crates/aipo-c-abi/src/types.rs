//! C ABI types, status codes, and value representations (ADP-009).

use aipo_host::{Handle, HostValue};
use aipo_vm::Value;
use std::ffi::c_char;
use unicode_normalization::UnicodeNormalization;

/// Largest integer the Aipo value model accepts: 2^53 - 1.
pub const MAX_SAFE_INT: i64 = (1_i64 << 53) - 1;
/// Smallest integer the Aipo value model accepts: -(2^53 - 1).
pub const MIN_SAFE_INT: i64 = -MAX_SAFE_INT;

/// Errors produced while validating a value crossing the C boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BoundaryError {
    /// The C `aipo_val_tag_t` carried a discriminant outside the declared enum.
    UnknownTag(i32),
    /// Integer outside ±(2^53 − 1).
    IntegerOutOfRange(i64),
    /// Float was NaN or infinite.
    NonFiniteFloat,
    /// The string bytes were not valid UTF-8.
    InvalidUtf8,
    /// A handle or failure value cannot be converted into the requested type.
    UnsupportedConversion(&'static str),
    /// A null pointer was supplied where a value was required.
    NullPointer,
    /// The tag was valid but the payload slot for it was absent.
    MissingPayload,
}

impl std::fmt::Display for BoundaryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownTag(t) => write!(f, "unknown value tag discriminant {t}"),
            Self::IntegerOutOfRange(n) => {
                write!(f, "integer {n} is outside ±{MAX_SAFE_INT}")
            }
            Self::NonFiniteFloat => write!(f, "float is not finite; NaN and infinity are faults"),
            Self::InvalidUtf8 => write!(f, "string data is not valid UTF-8"),
            Self::UnsupportedConversion(what) => write!(f, "{what}"),
            Self::NullPointer => write!(f, "null pointer where a value was required"),
            Self::MissingPayload => write!(f, "value tag has no payload in the required slot"),
        }
    }
}

impl std::error::Error for BoundaryError {}

/// Validates a raw C tag discriminant before it is turned into a Rust enum.
///
/// The C side is free to pass any integer, so reading the discriminant as the
/// `#[repr(C)]` enum directly would be undefined behaviour for out-of-range values.
///
/// # Errors
/// Returns [`BoundaryError::UnknownTag`] when `raw` is not a declared tag.
pub fn checked_tag(raw: i32) -> Result<aipo_val_tag_t, BoundaryError> {
    let tag = match raw {
        0 => aipo_val_tag_t::AIPO_VAL_NONE,
        1 => aipo_val_tag_t::AIPO_VAL_BOOL,
        2 => aipo_val_tag_t::AIPO_VAL_INT,
        3 => aipo_val_tag_t::AIPO_VAL_FLOAT,
        4 => aipo_val_tag_t::AIPO_VAL_STRING,
        5 => aipo_val_tag_t::AIPO_VAL_BYTES,
        6 => aipo_val_tag_t::AIPO_VAL_HANDLE,
        7 => aipo_val_tag_t::AIPO_VAL_FAILURE,
        other => return Err(BoundaryError::UnknownTag(other)),
    };
    Ok(tag)
}

/// Return / status code for C ABI operations.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(non_camel_case_types)]
pub enum aipo_status_t {
    /// Operation succeeded.
    AIPO_OK = 0,
    /// Usage error: invalid argument or null pointer.
    AIPO_ERR_USAGE = 1,
    /// Compilation or semantic diagnostic error.
    AIPO_ERR_DIAGNOSTIC = 2,
    /// Uncaught failure (Model B) returned by Aipo code.
    AIPO_ERR_UNCAUGHT_FAILURE = 3,
    /// Runtime fault (e.g. division by zero, type mismatch).
    AIPO_ERR_FAULT = 4,
    /// Operation refused because host capability was not granted.
    AIPO_ERR_CAPABILITY_DENIED = 5,
    /// Attempted to use a released or stale generational handle.
    AIPO_ERR_STALE_HANDLE = 6,
    /// Unexpected null pointer passed across the boundary.
    AIPO_ERR_NULL_POINTER = 7,
}

/// Tag discriminator for [`aipo_value_t`].
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(non_camel_case_types)]
pub enum aipo_val_tag_t {
    /// None value.
    AIPO_VAL_NONE = 0,
    /// Boolean value.
    AIPO_VAL_BOOL = 1,
    /// 64-bit integer within ±(2^53 - 1).
    AIPO_VAL_INT = 2,
    /// Finite 64-bit float.
    AIPO_VAL_FLOAT = 3,
    /// UTF-8 string snapshot.
    AIPO_VAL_STRING = 4,
    /// Byte sequence.
    AIPO_VAL_BYTES = 5,
    /// Generational host handle.
    AIPO_VAL_HANDLE = 6,
    /// Failure value with message.
    AIPO_VAL_FAILURE = 7,
}

/// Generational handle representation across the C ABI.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[allow(non_camel_case_types)]
pub struct aipo_handle_t {
    /// Slot index in the handle table.
    pub index: u64,
    /// Generation count of the slot.
    pub generation: u32,
}

impl From<Handle> for aipo_handle_t {
    fn from(handle: Handle) -> Self {
        Self {
            index: handle.index() as u64,
            generation: handle.generation(),
        }
    }
}

impl From<aipo_handle_t> for Handle {
    fn from(h: aipo_handle_t) -> Self {
        Handle::from_raw(h.index as usize, h.generation)
    }
}

/// Opaque runtime handle for C ABI.
#[allow(non_camel_case_types)]
pub type aipo_runtime_t = crate::runtime::AipoRuntime;

/// Host native callback signature for C ABI.
#[allow(non_camel_case_types)]
pub type aipo_host_fn_t = extern "C" fn(
    rt: *mut aipo_runtime_t,
    args: *const aipo_value_t,
    argc: usize,
    out_result: *mut aipo_value_t,
) -> aipo_status_t;

/// Plain, C-compatible value representation.
///
/// Simple values are copied by value. Strings and bytes point to UTF-8 / byte buffers
/// owned either by the host or kept in the runtime's memory pool during the call.
#[repr(C)]
#[derive(Clone, Copy)]
#[allow(non_camel_case_types)]
pub struct aipo_value_t {
    /// Type tag discriminator.
    pub tag: aipo_val_tag_t,
    /// Boolean payload.
    pub bool_val: bool,
    /// Integer payload.
    pub int_val: i64,
    /// Float payload.
    pub float_val: f64,
    /// Pointer to string data (null-terminated or bounded by `str_len`).
    pub str_ptr: *const c_char,
    /// Byte length of string data.
    pub str_len: usize,
    /// Pointer to raw bytes.
    pub bytes_ptr: *const u8,
    /// Byte length of raw bytes.
    pub bytes_len: usize,
    /// Generational handle payload.
    pub handle_val: aipo_handle_t,
}

impl Default for aipo_value_t {
    fn default() -> Self {
        Self {
            tag: aipo_val_tag_t::AIPO_VAL_NONE,
            bool_val: false,
            int_val: 0,
            float_val: 0.0,
            str_ptr: std::ptr::null(),
            str_len: 0,
            bytes_ptr: std::ptr::null(),
            bytes_len: 0,
            handle_val: aipo_handle_t::default(),
        }
    }
}

impl aipo_value_t {
    /// Creates a none value.
    #[must_use]
    pub const fn none() -> Self {
        Self {
            tag: aipo_val_tag_t::AIPO_VAL_NONE,
            bool_val: false,
            int_val: 0,
            float_val: 0.0,
            str_ptr: std::ptr::null(),
            str_len: 0,
            bytes_ptr: std::ptr::null(),
            bytes_len: 0,
            handle_val: aipo_handle_t {
                index: 0,
                generation: 0,
            },
        }
    }

    /// Creates a boolean value.
    #[must_use]
    pub const fn bool_val(val: bool) -> Self {
        let mut v = Self::none();
        v.tag = aipo_val_tag_t::AIPO_VAL_BOOL;
        v.bool_val = val;
        v
    }

    /// Creates an integer value.
    #[must_use]
    pub const fn int_val(val: i64) -> Self {
        let mut v = Self::none();
        v.tag = aipo_val_tag_t::AIPO_VAL_INT;
        v.int_val = val;
        v
    }

    /// Creates a float value.
    #[must_use]
    pub const fn float_val(val: f64) -> Self {
        let mut v = Self::none();
        v.tag = aipo_val_tag_t::AIPO_VAL_FLOAT;
        v.float_val = val;
        v
    }

    /// Creates a handle value.
    #[must_use]
    pub const fn handle(h: aipo_handle_t) -> Self {
        let mut v = Self::none();
        v.tag = aipo_val_tag_t::AIPO_VAL_HANDLE;
        v.handle_val = h;
        v
    }

    /// Converts a VM [`Value`] into an [`aipo_value_t`] using an owned snapshot.
    ///
    /// Heap payloads are copied into a freshly allocated buffer handed back through
    /// `on_bytes` together with its length. The returned pointer does not borrow
    /// from `val`, so it stays valid after the value that produced it is dropped.
    /// The caller owns the result and releases it with [`crate::aipo_value_release`].
    pub fn from_vm_value<F>(val: &Value, mut on_bytes: F) -> Self
    where
        F: FnMut(&[u8]) -> *const u8,
    {
        let mut string = |s: &str| -> Self {
            let mut v = Self::none();
            v.tag = aipo_val_tag_t::AIPO_VAL_STRING;
            v.str_ptr = on_bytes(s.as_bytes()).cast::<c_char>();
            v.str_len = s.len();
            v
        };

        match val {
            Value::None => Self::none(),
            Value::Bool(b) => Self::bool_val(*b),
            Value::Int(i) => Self::int_val(*i),
            Value::Float(f) => Self::float_val(*f),
            Value::String(s) => string(s.as_str()),
            Value::Bytes(b) => {
                // Snapshot: the `RefCell<Vec<u8>>` can be mutated or reallocated by
                // Aipo code, so borrowing its buffer would dangle or alias.
                let snapshot = b.borrow().clone();
                let mut v = Self::none();
                v.tag = aipo_val_tag_t::AIPO_VAL_BYTES;
                v.bytes_len = snapshot.len();
                v.bytes_ptr = on_bytes(&snapshot);
                v
            }
            Value::HostHandle(h) => Self::handle((*h).into()),
            Value::Failure(f) => {
                let mut v = string(&f.message);
                v.tag = aipo_val_tag_t::AIPO_VAL_FAILURE;
                v
            }
            other => string(&other.to_string()),
        }
    }

    /// Reads a length-delimited string, normalizing it to NFC.
    ///
    /// Reading `str_len` bytes instead of scanning for a NUL keeps an embedded NUL
    /// intact and makes a wrong `str_len` a wrong string rather than a read past
    /// the end of the allocation.
    fn read_str(&self) -> Result<String, BoundaryError> {
        if self.str_ptr.is_null() {
            return if self.str_len == 0 {
                Ok(String::new())
            } else {
                Err(BoundaryError::NullPointer)
            };
        }
        let bytes = unsafe { std::slice::from_raw_parts(self.str_ptr.cast::<u8>(), self.str_len) };
        let text = std::str::from_utf8(bytes).map_err(|_| BoundaryError::InvalidUtf8)?;
        Ok(text.nfc().collect())
    }

    /// Converts an [`aipo_value_t`] into an [`aipo_host::HostValue`], validating
    /// every contract the VM holds internally.
    ///
    /// # Errors
    /// Returns [`BoundaryError`] when a numeric range, finiteness, or UTF-8
    /// invariant is violated, or the conversion is not defined for that tag.
    pub fn to_host_value(&self) -> Result<HostValue, BoundaryError> {
        match self.tag {
            aipo_val_tag_t::AIPO_VAL_NONE => Ok(HostValue::None),
            aipo_val_tag_t::AIPO_VAL_BOOL => Ok(HostValue::Bool(self.bool_val)),
            aipo_val_tag_t::AIPO_VAL_INT => {
                if !(MIN_SAFE_INT..=MAX_SAFE_INT).contains(&self.int_val) {
                    return Err(BoundaryError::IntegerOutOfRange(self.int_val));
                }
                Ok(HostValue::Int(self.int_val))
            }
            aipo_val_tag_t::AIPO_VAL_FLOAT => {
                if !self.float_val.is_finite() {
                    return Err(BoundaryError::NonFiniteFloat);
                }
                Ok(HostValue::Float(self.float_val))
            }
            aipo_val_tag_t::AIPO_VAL_STRING => Ok(HostValue::String(self.read_str()?)),
            aipo_val_tag_t::AIPO_VAL_BYTES => {
                if self.bytes_ptr.is_null() {
                    return if self.bytes_len == 0 {
                        Ok(HostValue::Bytes(Vec::new()))
                    } else {
                        Err(BoundaryError::NullPointer)
                    };
                }
                let slice = unsafe { std::slice::from_raw_parts(self.bytes_ptr, self.bytes_len) };
                Ok(HostValue::Bytes(slice.to_vec()))
            }
            aipo_val_tag_t::AIPO_VAL_HANDLE => Err(BoundaryError::UnsupportedConversion(
                "a handle cannot become a host value without resolving it first",
            )),
            aipo_val_tag_t::AIPO_VAL_FAILURE => Err(BoundaryError::UnsupportedConversion(
                "a failure cannot be passed as a host argument",
            )),
        }
    }

    /// Converts an [`aipo_value_t`] into a VM [`Value`], validating every contract
    /// the VM holds internally.
    ///
    /// # Errors
    /// Returns [`BoundaryError`] when a numeric range, finiteness, or UTF-8
    /// invariant is violated.
    pub fn to_vm_value(&self) -> Result<Value, BoundaryError> {
        match self.tag {
            aipo_val_tag_t::AIPO_VAL_NONE => Ok(Value::None),
            aipo_val_tag_t::AIPO_VAL_BOOL => Ok(Value::Bool(self.bool_val)),
            aipo_val_tag_t::AIPO_VAL_INT => {
                if !(MIN_SAFE_INT..=MAX_SAFE_INT).contains(&self.int_val) {
                    return Err(BoundaryError::IntegerOutOfRange(self.int_val));
                }
                Ok(Value::Int(self.int_val))
            }
            aipo_val_tag_t::AIPO_VAL_FLOAT => {
                if !self.float_val.is_finite() {
                    return Err(BoundaryError::NonFiniteFloat);
                }
                Ok(Value::Float(self.float_val))
            }
            aipo_val_tag_t::AIPO_VAL_STRING => {
                Ok(Value::String(std::rc::Rc::new(self.read_str()?)))
            }
            aipo_val_tag_t::AIPO_VAL_BYTES => {
                if self.bytes_ptr.is_null() {
                    return if self.bytes_len == 0 {
                        Ok(Value::Bytes(std::rc::Rc::new(std::cell::RefCell::new(
                            Vec::new().into(),
                        ))))
                    } else {
                        Err(BoundaryError::NullPointer)
                    };
                }
                let slice = unsafe { std::slice::from_raw_parts(self.bytes_ptr, self.bytes_len) };
                Ok(Value::Bytes(std::rc::Rc::new(std::cell::RefCell::new(
                    slice.to_vec().into(),
                ))))
            }
            aipo_val_tag_t::AIPO_VAL_HANDLE => Ok(Value::HostHandle(self.handle_val.into())),
            aipo_val_tag_t::AIPO_VAL_FAILURE => Ok(Value::failure(self.read_str()?)),
        }
    }
}

/// Opaque handle to a virtual register machine instance.
#[repr(C)]
pub struct aipo_reg_vm_t {
    _private: [u8; 0],
}
