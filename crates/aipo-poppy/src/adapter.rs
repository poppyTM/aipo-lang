//! VM adapter and native function bindings for the Poppy Game Engine profile.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::{LazyLock, Mutex};

#[cfg(test)]
use aipo_diagnostics::DiagnosticCode;
use aipo_host::{Capability, CapabilitySet, Handle};
use aipo_vm::{DictMap, Value, Vm, VmFault};

use crate::simulation::Simulation;

/// Global service state for the Poppy host profile.
pub struct PoppyService {
    /// Simulation state.
    pub sim: Simulation,
    /// Granted capabilities for this host session.
    pub capabilities: CapabilitySet,
}

impl PoppyService {
    /// Creates a service with a simulation and default Poppy capabilities.
    #[must_use]
    pub fn new(seed: u64, dt: f64) -> Self {
        let mut caps = CapabilitySet::none();
        caps.grant(Capability::parse("poppy").expect("valid capability"));
        Self {
            sim: Simulation::new(seed, dt),
            capabilities: caps,
        }
    }

    /// Creates a service with custom granted capabilities.
    #[must_use]
    pub fn with_capabilities(seed: u64, dt: f64, capabilities: CapabilitySet) -> Self {
        Self {
            sim: Simulation::new(seed, dt),
            capabilities,
        }
    }
}

static POPPY: LazyLock<Mutex<Option<PoppyService>>> = LazyLock::new(|| Mutex::new(None));

/// Serializes Poppy tests across the process.
pub static POPPY_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

/// Installs the global Poppy service.
pub fn install_poppy(service: PoppyService) {
    if let Ok(mut guard) = POPPY.lock() {
        *guard = Some(service);
    }
}

/// Revokes the global Poppy service, denying all Poppy capabilities.
pub fn revoke_poppy() {
    if let Ok(mut guard) = POPPY.lock() {
        *guard = None;
    }
}

/// Helper to access the Poppy service under lock with capability checking.
fn with_poppy<R>(
    capability: &str,
    operation: &str,
    f: impl FnOnce(&mut PoppyService) -> Result<R, VmFault>,
) -> Result<R, VmFault> {
    let mut guard = POPPY.lock().map_err(|_| VmFault::CorruptedBytecode {
        offset: 0,
        reason: "poppy service mutex was poisoned".to_string(),
    })?;

    let Some(ref mut service) = *guard else {
        return Err(VmFault::CapabilityDenied {
            capability: capability.to_string(),
            operation: operation.to_string(),
        });
    };

    let cap = Capability::parse(capability).map_err(|_| VmFault::CorruptedBytecode {
        offset: 0,
        reason: format!("invalid capability '{capability}'"),
    })?;

    if !service.capabilities.allows(&cap) {
        return Err(VmFault::CapabilityDenied {
            capability: capability.to_string(),
            operation: operation.to_string(),
        });
    }

    f(service)
}

fn require_arity(args: &[Value], expected: usize, op: &str) -> Result<(), VmFault> {
    if args.len() == expected {
        Ok(())
    } else {
        Err(VmFault::TypeMismatch {
            expected: format!("{expected} argument(s) for {op}"),
            actual: format!("{} arguments", args.len()),
        })
    }
}

fn expect_string(val: &Value, op: &str) -> Result<String, VmFault> {
    match val {
        Value::String(s) => Ok(s.to_string()),
        other => Err(VmFault::TypeMismatch {
            expected: format!("String for {op}"),
            actual: other.type_name().to_string(),
        }),
    }
}

fn expect_float(val: &Value, op: &str) -> Result<f64, VmFault> {
    match val {
        Value::Float(f) => Ok(*f),
        Value::Int(i) => Ok(*i as f64),
        other => Err(VmFault::TypeMismatch {
            expected: format!("Float for {op}"),
            actual: other.type_name().to_string(),
        }),
    }
}

fn expect_int(val: &Value, op: &str) -> Result<i64, VmFault> {
    match val {
        Value::Int(i) => Ok(*i),
        other => Err(VmFault::TypeMismatch {
            expected: format!("Int for {op}"),
            actual: other.type_name().to_string(),
        }),
    }
}

fn expect_handle(val: &Value, op: &str) -> Result<Handle, VmFault> {
    match val {
        Value::HostHandle(h) => Ok(*h),
        other => Err(VmFault::TypeMismatch {
            expected: format!("Entity HostHandle for {op}"),
            actual: other.type_name().to_string(),
        }),
    }
}

/// Native function: `poppy.spawn(tag, x, y)`
pub fn poppy_spawn(args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 3, "poppy.spawn")?;
    let tag = expect_string(&args[0], "poppy.spawn tag")?;
    let x = expect_float(&args[1], "poppy.spawn x")?;
    let y = expect_float(&args[2], "poppy.spawn y")?;

    with_poppy("poppy.ecs", "poppy.spawn", |service| {
        let handle = service
            .sim
            .world_mut()
            .spawn_immediate(&tag, x, y, 0.0, 0.0);
        Ok(Value::HostHandle(handle))
    })
}

/// Native function: `poppy.despawn(entity)`
pub fn poppy_despawn(args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 1, "poppy.despawn")?;
    let handle = expect_handle(&args[0], "poppy.despawn entity")?;

    with_poppy("poppy.ecs", "poppy.despawn", |service| {
        service
            .sim
            .world_mut()
            .queue_despawn(handle)
            .map_err(|_| VmFault::StaleHandle {
                handle: handle.to_string(),
            })?;
        Ok(Value::None)
    })
}

/// Native function: `poppy.query(tag)`
pub fn poppy_query(args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 1, "poppy.query")?;
    let tag = expect_string(&args[0], "poppy.query tag")?;

    with_poppy("poppy.ecs", "poppy.query", |service| {
        let handles = service.sim.world().query(&tag);
        let values: Vec<Value> = handles.into_iter().map(Value::HostHandle).collect();
        Ok(Value::List(Rc::new(RefCell::new((values).into()))))
    })
}

/// Native function: `poppy.get_position(entity)`
pub fn poppy_get_position(args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 1, "poppy.get_position")?;
    let handle = expect_handle(&args[0], "poppy.get_position entity")?;

    with_poppy("poppy.ecs", "poppy.get_position", |service| {
        let record = service
            .sim
            .world()
            .get_entity(handle)
            .map_err(|_| VmFault::StaleHandle {
                handle: handle.to_string(),
            })?;

        let entries = vec![
            (
                Value::String(Rc::new("x".to_string())),
                Value::Float(record.x),
            ),
            (
                Value::String(Rc::new("y".to_string())),
                Value::Float(record.y),
            ),
        ];
        Ok(Value::Dict(Rc::new(RefCell::new(DictMap::from_entries(
            entries,
        )))))
    })
}

/// Native function: `poppy.set_position(entity, x, y)`
pub fn poppy_set_position(args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 3, "poppy.set_position")?;
    let handle = expect_handle(&args[0], "poppy.set_position entity")?;
    let x = expect_float(&args[1], "poppy.set_position x")?;
    let y = expect_float(&args[2], "poppy.set_position y")?;

    with_poppy("poppy.ecs", "poppy.set_position", |service| {
        let record = service
            .sim
            .world_mut()
            .get_entity_mut(handle)
            .map_err(|_| VmFault::StaleHandle {
                handle: handle.to_string(),
            })?;
        record.x = x;
        record.y = y;
        Ok(Value::None)
    })
}

/// Native function: `poppy.get_velocity(entity)`
pub fn poppy_get_velocity(args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 1, "poppy.get_velocity")?;
    let handle = expect_handle(&args[0], "poppy.get_velocity entity")?;

    with_poppy("poppy.ecs", "poppy.get_velocity", |service| {
        let record = service
            .sim
            .world()
            .get_entity(handle)
            .map_err(|_| VmFault::StaleHandle {
                handle: handle.to_string(),
            })?;

        let entries = vec![
            (
                Value::String(Rc::new("vx".to_string())),
                Value::Float(record.vx),
            ),
            (
                Value::String(Rc::new("vy".to_string())),
                Value::Float(record.vy),
            ),
        ];
        Ok(Value::Dict(Rc::new(RefCell::new(DictMap::from_entries(
            entries,
        )))))
    })
}

/// Native function: `poppy.set_velocity(entity, vx, vy)`
pub fn poppy_set_velocity(args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 3, "poppy.set_velocity")?;
    let handle = expect_handle(&args[0], "poppy.set_velocity entity")?;
    let vx = expect_float(&args[1], "poppy.set_velocity vx")?;
    let vy = expect_float(&args[2], "poppy.set_velocity vy")?;

    with_poppy("poppy.ecs", "poppy.set_velocity", |service| {
        let record = service
            .sim
            .world_mut()
            .get_entity_mut(handle)
            .map_err(|_| VmFault::StaleHandle {
                handle: handle.to_string(),
            })?;
        record.vx = vx;
        record.vy = vy;
        Ok(Value::None)
    })
}

/// Native function: `poppy.random_float()`
pub fn poppy_random_float(args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 0, "poppy.random_float")?;

    with_poppy("poppy.random", "poppy.random_float", |service| {
        let f = service.sim.rng_mut().next_float();
        Ok(Value::Float(f))
    })
}

/// Native function: `poppy.random_int(min, max)`
pub fn poppy_random_int(args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 2, "poppy.random_int")?;
    let min = expect_int(&args[0], "poppy.random_int min")?;
    let max = expect_int(&args[1], "poppy.random_int max")?;

    with_poppy("poppy.random", "poppy.random_int", |service| {
        let n = service.sim.rng_mut().random_int(min, max);
        Ok(Value::Int(n))
    })
}

/// Native function: `poppy.step()`
pub fn poppy_step(args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 0, "poppy.step")?;

    with_poppy("poppy.ecs", "poppy.step", |service| {
        let digest = service.sim.step();
        // Safe conversion to i64 for script observation
        let safe_int = (digest & 0x001F_FFFF_FFFF_FFFF) as i64;
        Ok(Value::Int(safe_int))
    })
}

/// Native function: `poppy.digest()`
pub fn poppy_digest(args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 0, "poppy.digest")?;

    with_poppy("poppy.ecs", "poppy.digest", |service| {
        let tick = service.sim.tick();
        let rng_state = service.sim.rng_mut().state();
        let digest = service.sim.world().digest(tick, rng_state);
        let safe_int = (digest & 0x001F_FFFF_FFFF_FFFF) as i64;
        Ok(Value::Int(safe_int))
    })
}

/// Constructs the `poppy` module dictionary.
#[must_use]
pub fn create_module() -> Value {
    let entries = vec![
        (
            Value::String(Rc::new("spawn".to_string())),
            Value::native("poppy.spawn", 3, poppy_spawn),
        ),
        (
            Value::String(Rc::new("despawn".to_string())),
            Value::native("poppy.despawn", 1, poppy_despawn),
        ),
        (
            Value::String(Rc::new("query".to_string())),
            Value::native("poppy.query", 1, poppy_query),
        ),
        (
            Value::String(Rc::new("get_position".to_string())),
            Value::native("poppy.get_position", 1, poppy_get_position),
        ),
        (
            Value::String(Rc::new("set_position".to_string())),
            Value::native("poppy.set_position", 3, poppy_set_position),
        ),
        (
            Value::String(Rc::new("get_velocity".to_string())),
            Value::native("poppy.get_velocity", 1, poppy_get_velocity),
        ),
        (
            Value::String(Rc::new("set_velocity".to_string())),
            Value::native("poppy.set_velocity", 3, poppy_set_velocity),
        ),
        (
            Value::String(Rc::new("random_float".to_string())),
            Value::native("poppy.random_float", 0, poppy_random_float),
        ),
        (
            Value::String(Rc::new("random_int".to_string())),
            Value::native("poppy.random_int", 2, poppy_random_int),
        ),
        (
            Value::String(Rc::new("step".to_string())),
            Value::native("poppy.step", 0, poppy_step),
        ),
        (
            Value::String(Rc::new("digest".to_string())),
            Value::native("poppy.digest", 0, poppy_digest),
        ),
    ];

    Value::Dict(Rc::new(RefCell::new(DictMap::from_entries(entries))))
}

/// Registers the `poppy` module in the VM's global environment.
pub fn register_poppy(vm: &mut Vm) {
    vm.define_global("poppy", create_module());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_adapter_capability_denial() {
        let _lock = POPPY_LOCK.lock().unwrap();
        revoke_poppy();

        let err = poppy_spawn(&[
            Value::String(Rc::new("player".to_string())),
            Value::Float(0.0),
            Value::Float(0.0),
        ])
        .expect_err("should be denied");

        assert_eq!(
            err.diagnostic_code(),
            DiagnosticCode::AIPO_RT_CAPABILITY_DENIED
        );
    }

    #[test]
    fn test_adapter_operations_when_granted() {
        let _lock = POPPY_LOCK.lock().unwrap();
        install_poppy(PoppyService::new(42, 1.0 / 60.0));

        let entity = poppy_spawn(&[
            Value::String(Rc::new("enemy".to_string())),
            Value::Float(10.0),
            Value::Float(20.0),
        ])
        .expect("spawn succeeded");

        let pos = poppy_get_position(std::slice::from_ref(&entity)).expect("get pos");
        if let Value::Dict(d) = pos {
            assert_eq!(
                d.borrow().get(&Value::String(Rc::new("x".to_string()))),
                Some(&Value::Float(10.0))
            );
        } else {
            panic!("expected Dict");
        }

        poppy_despawn(std::slice::from_ref(&entity)).expect("despawn succeeded");

        // Safe point (step) flushes despawn
        poppy_step(&[]).expect("step");

        // After safe point, handle is stale
        let err = poppy_get_position(&[entity]).expect_err("should be stale");
        assert_eq!(err.diagnostic_code(), DiagnosticCode::AIPO_RT_STALE_HANDLE);
    }
}
