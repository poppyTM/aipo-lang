//! Unicode data generated at build time by `build.rs`.
//!
//! The tables are `pub(crate)` constants in the generated file; this module only
//! gives them a home so the rest of the crate can refer to `unicode_tables::…`
//! instead of an `include!` at every use site.

include!(concat!(env!("OUT_DIR"), "/unicode_tables.rs"));
