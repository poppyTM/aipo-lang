//! Host surface consumption for the CLI: `--ahs=<file>`.
//!
//! The compiler consumes the host description as data, without host-specific code.
//! Loading a surface neither registers runtime implementations nor grants capabilities.
//! Direct calls are checked against described signatures; runtime dispatch and contracts
//! on non-literal values remain the host adapter's responsibility.

use std::collections::HashMap;
use std::path::Path;

use aipo_host::HostSchema;
use aipo_sema::{HostFunction, HostParameter, PreludeSurface};

/// Loads and validates an AHS file as an explicit semantic surface.
///
/// The returned surface belongs to the caller; it never changes other CLI invocations
/// or embedding compilations.
///
/// # Errors
/// Returns a message naming the file when reading, parsing or schema validation fails.
pub fn load(path: &Path) -> Result<PreludeSurface, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|error| format!("cannot read host surface '{}': {error}", path.display()))?;
    from_json(&text).map_err(|fault| format!("invalid host surface '{}': {fault}", path.display()))
}

/// Converts a validated description to the semantic surface shared by CLI profiles.
pub(crate) fn from_json(text: &str) -> Result<PreludeSurface, aipo_host::HostFault> {
    let schema = HostSchema::from_json(text)?;
    let mut surface = PreludeSurface::new();
    for module in &schema.modules {
        let functions: HashMap<String, HostFunction> = module
            .functions
            .iter()
            .map(|function| {
                (
                    function.name.clone(),
                    HostFunction {
                        params: function
                            .params
                            .iter()
                            .map(|param| HostParameter {
                                name: param.name.clone(),
                                ty: param.ty.name.clone(),
                                nullable: param.ty.nullable,
                                optional: param.is_optional,
                            })
                            .collect(),
                        // An AHS names every parameter it accepts, so its arity is exact.
                        variadic: false,
                    },
                )
            })
            .collect();
        surface.add_host_module(&module.name, functions);
    }
    Ok(surface)
}
