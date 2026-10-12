//! Native function bindings and module creation for the `egui` host profile.

#![forbid(unsafe_code)]

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::{LazyLock, Mutex};

use aipo_host::{Capability, CapabilitySet, Handle, HandleTable};
use aipo_vm::{DictMap, Value, Vm, VmFault};

use crate::context::{EguiSession, FrameInputOptions};

/// Global service state for the egui host profile.
pub struct EguiService {
    /// Session table managing live contexts behind generational handles.
    pub sessions: HandleTable<EguiSession>,
    /// Granted capabilities for this host session.
    pub capabilities: CapabilitySet,
    /// Currently active session handle during frame execution.
    pub active_handle: Option<Handle>,
    /// Last session handle that executed a frame pass.
    pub last_active_handle: Option<Handle>,
}

impl EguiService {
    /// Creates a service with default capabilities.
    #[must_use]
    pub fn new() -> Self {
        let mut caps = CapabilitySet::none();
        caps.grant(Capability::parse("egui").expect("valid capability"));
        Self {
            sessions: HandleTable::new(),
            capabilities: caps,
            active_handle: None,
            last_active_handle: None,
        }
    }

    /// Creates a service with explicit granted capabilities.
    #[must_use]
    pub fn with_capabilities(capabilities: CapabilitySet) -> Self {
        Self {
            sessions: HandleTable::new(),
            capabilities,
            active_handle: None,
            last_active_handle: None,
        }
    }
}

impl Default for EguiService {
    fn default() -> Self {
        Self::new()
    }
}

static EGUI: LazyLock<Mutex<Option<EguiService>>> = LazyLock::new(|| Mutex::new(None));

/// Serializes egui host operations across the process in test runs.
pub static EGUI_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

/// Installs the global egui service.
pub fn install_egui(service: EguiService) {
    if let Ok(mut guard) = EGUI.lock() {
        *guard = Some(service);
    }
}

/// Revokes the global egui service, denying all egui operations.
pub fn revoke_egui() {
    if let Ok(mut guard) = EGUI.lock() {
        *guard = None;
    }
}

/// Helper to access the Egui service under lock with capability checking.
pub fn with_egui<R>(
    capability: &str,
    operation: &str,
    f: impl FnOnce(&mut EguiService) -> Result<R, VmFault>,
) -> Result<R, VmFault> {
    let mut guard = EGUI.lock().map_err(|_| VmFault::CorruptedBytecode {
        offset: 0,
        reason: "egui service mutex was poisoned".to_string(),
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

fn expect_handle(val: &Value, op: &str) -> Result<Handle, VmFault> {
    match val {
        Value::HostHandle(h) => Ok(*h),
        other => Err(VmFault::TypeMismatch {
            expected: format!("HostHandle for {op}"),
            actual: other.type_name().to_string(),
        }),
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

fn expect_bool(val: &Value, op: &str) -> Result<bool, VmFault> {
    match val {
        Value::Bool(b) => Ok(*b),
        other => Err(VmFault::TypeMismatch {
            expected: format!("Bool for {op}"),
            actual: other.type_name().to_string(),
        }),
    }
}

fn get_active_session_mut(service: &mut EguiService) -> Result<&mut EguiSession, VmFault> {
    let handle = service
        .active_handle
        .ok_or_else(|| VmFault::CorruptedBytecode {
            offset: 0,
            reason: "no active egui session: call egui.begin_frame first".to_string(),
        })?;

    service
        .sessions
        .get_mut(handle)
        .ok_or_else(|| VmFault::StaleHandle {
            handle: handle.to_string(),
        })
}

/// Native function: `egui.create_context()`
pub fn egui_create_context(args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 0, "egui.create_context")?;
    with_egui("egui", "egui.create_context", |service| {
        let handle = service.sessions.insert(EguiSession::new());
        Ok(Value::HostHandle(handle))
    })
}

/// Native function: `egui.destroy_context(handle)`
pub fn egui_destroy_context(args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 1, "egui.destroy_context")?;
    let handle = expect_handle(&args[0], "egui.destroy_context handle")?;

    with_egui("egui", "egui.destroy_context", |service| {
        service
            .sessions
            .remove(handle)
            .ok_or_else(|| VmFault::StaleHandle {
                handle: handle.to_string(),
            })?;
        if service.active_handle == Some(handle) {
            service.active_handle = None;
        }
        Ok(Value::None)
    })
}

/// Native function: `egui.begin_frame(handle, [options])`
pub fn egui_begin_frame(args: &[Value]) -> Result<Value, VmFault> {
    if args.is_empty() || args.len() > 2 {
        return Err(VmFault::TypeMismatch {
            expected: "1 or 2 arguments for egui.begin_frame".to_string(),
            actual: format!("{} arguments", args.len()),
        });
    }

    let handle = expect_handle(&args[0], "egui.begin_frame handle")?;
    let mut options = FrameInputOptions::default();

    if args.len() == 2
        && let Value::Dict(dict_rc) = &args[1]
    {
        let dict = dict_rc.borrow();
        let get_f = |key: &str| -> Option<f32> {
            dict.get(&Value::String(Rc::new(key.to_string())))
                .and_then(|v| match v {
                    Value::Float(f) => Some(*f as f32),
                    Value::Int(i) => Some(*i as f32),
                    _ => None,
                })
        };
        let get_b = |key: &str| -> Option<bool> {
            dict.get(&Value::String(Rc::new(key.to_string())))
                .and_then(|v| match v {
                    Value::Bool(b) => Some(*b),
                    _ => None,
                })
        };

        if let Some(w) = get_f("width") {
            options.width = w;
        }
        if let Some(h) = get_f("height") {
            options.height = h;
        }
        if let Some(mx) = get_f("mouse_x") {
            options.mouse_x = mx;
        }
        if let Some(my) = get_f("mouse_y") {
            options.mouse_y = my;
        }
        if let Some(md) = get_b("mouse_down") {
            options.mouse_down = md;
        }
        if let Some(mc) = get_b("mouse_clicked") {
            options.mouse_clicked = mc;
        }
        if let Some(dt) = get_f("dt") {
            options.dt = dt;
        }
        if let Some(sx) = get_f("scroll_x") {
            options.scroll_x = sx;
        }
        if let Some(sy) = get_f("scroll_y") {
            options.scroll_y = sy;
        }
    }

    with_egui("egui", "egui.begin_frame", |service| {
        let session = service
            .sessions
            .get_mut(handle)
            .ok_or_else(|| VmFault::StaleHandle {
                handle: handle.to_string(),
            })?;
        session.begin_frame(&options)?;
        service.active_handle = Some(handle);
        service.last_active_handle = Some(handle);
        Ok(Value::None)
    })
}

/// Native function: `egui.end_frame([handle])`
pub fn egui_end_frame(args: &[Value]) -> Result<Value, VmFault> {
    if args.len() > 1 {
        return Err(VmFault::TypeMismatch {
            expected: "0 or 1 argument for egui.end_frame".to_string(),
            actual: format!("{} arguments", args.len()),
        });
    }

    with_egui("egui", "egui.end_frame", |service| {
        let handle = if args.len() == 1 {
            expect_handle(&args[0], "egui.end_frame handle")?
        } else {
            service
                .active_handle
                .ok_or_else(|| VmFault::CorruptedBytecode {
                    offset: 0,
                    reason: "no active egui session to end".to_string(),
                })?
        };

        let session = service
            .sessions
            .get_mut(handle)
            .ok_or_else(|| VmFault::StaleHandle {
                handle: handle.to_string(),
            })?;
        let output = session.end_frame()?;
        service.active_handle = None;
        service.last_active_handle = Some(handle);
        Ok(output)
    })
}

/// Native function: `egui.begin_window(title, [x, y, width, height])`
pub fn egui_begin_window(args: &[Value]) -> Result<Value, VmFault> {
    if args.is_empty() || args.len() > 5 {
        return Err(VmFault::TypeMismatch {
            expected: "1 to 5 arguments for egui.begin_window".to_string(),
            actual: format!("{} arguments", args.len()),
        });
    }

    let title = expect_string(&args[0], "egui.begin_window title")?;
    let x = if args.len() > 1 {
        expect_float(&args[1], "egui.begin_window x")? as f32
    } else {
        50.0
    };
    let y = if args.len() > 2 {
        expect_float(&args[2], "egui.begin_window y")? as f32
    } else {
        50.0
    };
    let width = if args.len() > 3 {
        expect_float(&args[3], "egui.begin_window width")? as f32
    } else {
        250.0
    };
    let height = if args.len() > 4 {
        expect_float(&args[4], "egui.begin_window height")? as f32
    } else {
        200.0
    };

    with_egui("egui", "egui.begin_window", |service| {
        let session = get_active_session_mut(service)?;
        session.begin_window(&title, x, y, width, height)?;
        Ok(Value::None)
    })
}

/// Native function: `egui.end_window()`
pub fn egui_end_window(args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 0, "egui.end_window")?;
    with_egui("egui", "egui.end_window", |service| {
        let session = get_active_session_mut(service)?;
        session.end_window()?;
        Ok(Value::None)
    })
}

/// Native function: `egui.label(text)`
pub fn egui_label(args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 1, "egui.label")?;
    let text = expect_string(&args[0], "egui.label text")?;

    with_egui("egui", "egui.label", |service| {
        let session = get_active_session_mut(service)?;
        let ui = session.current_ui_mut()?;
        ui.label(text);
        Ok(Value::None)
    })
}

/// Native function: `egui.heading(text)`
pub fn egui_heading(args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 1, "egui.heading")?;
    let text = expect_string(&args[0], "egui.heading text")?;

    with_egui("egui", "egui.heading", |service| {
        let session = get_active_session_mut(service)?;
        let ui = session.current_ui_mut()?;
        ui.heading(text);
        Ok(Value::None)
    })
}

/// Native function: `egui.separator()`
pub fn egui_separator(args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 0, "egui.separator")?;
    with_egui("egui", "egui.separator", |service| {
        let session = get_active_session_mut(service)?;
        let ui = session.current_ui_mut()?;
        ui.separator();
        Ok(Value::None)
    })
}

/// Native function: `egui.button(text) -> Bool`
pub fn egui_button(args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 1, "egui.button")?;
    let text = expect_string(&args[0], "egui.button text")?;

    with_egui("egui", "egui.button", |service| {
        let session = get_active_session_mut(service)?;
        let ui = session.current_ui_mut()?;
        let clicked = ui.button(text).clicked();
        Ok(Value::Bool(clicked))
    })
}

/// Native function: `egui.checkbox(text, checked) -> Bool`
pub fn egui_checkbox(args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 2, "egui.checkbox")?;
    let text = expect_string(&args[0], "egui.checkbox text")?;
    let checked = expect_bool(&args[1], "egui.checkbox checked")?;

    with_egui("egui", "egui.checkbox", |service| {
        let session = get_active_session_mut(service)?;
        let ui = session.current_ui_mut()?;
        let mut val = checked;
        let _ = ui.checkbox(&mut val, text);
        Ok(Value::Bool(val))
    })
}

/// Native function: `egui.slider(text, value, min, max) -> Float`
pub fn egui_slider(args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 4, "egui.slider")?;
    let text = expect_string(&args[0], "egui.slider text")?;
    let val = expect_float(&args[1], "egui.slider value")?;
    let min = expect_float(&args[2], "egui.slider min")?;
    let max = expect_float(&args[3], "egui.slider max")?;

    with_egui("egui", "egui.slider", |service| {
        let session = get_active_session_mut(service)?;
        let ui = session.current_ui_mut()?;
        let mut current = val;
        let _ = ui.add(egui::Slider::new(&mut current, min..=max).text(text));
        Ok(Value::Float(current))
    })
}

/// Native function: `egui.text_edit(label, text) -> String`
pub fn egui_text_edit(args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 2, "egui.text_edit")?;
    let _label = expect_string(&args[0], "egui.text_edit label")?;
    let current_text = expect_string(&args[1], "egui.text_edit text")?;

    with_egui("egui", "egui.text_edit", |service| {
        let session = get_active_session_mut(service)?;
        let ui = session.current_ui_mut()?;
        let mut text = current_text;
        let _ = ui.text_edit_singleline(&mut text);
        Ok(Value::String(Rc::new(text)))
    })
}

/// Native function: `egui.progress_bar(fraction)`
pub fn egui_progress_bar(args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 1, "egui.progress_bar")?;
    let fraction = expect_float(&args[0], "egui.progress_bar fraction")?;

    with_egui("egui", "egui.progress_bar", |service| {
        let session = get_active_session_mut(service)?;
        let ui = session.current_ui_mut()?;
        ui.add(egui::ProgressBar::new(fraction as f32));
        Ok(Value::None)
    })
}

/// Native function: `egui.wants_pointer_input(handle) -> Bool`
pub fn egui_wants_pointer_input(args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 1, "egui.wants_pointer_input")?;
    let handle = expect_handle(&args[0], "egui.wants_pointer_input handle")?;

    with_egui("egui", "egui.wants_pointer_input", |service| {
        let session = service
            .sessions
            .get(handle)
            .ok_or_else(|| VmFault::StaleHandle {
                handle: handle.to_string(),
            })?;
        Ok(Value::Bool(session.ctx.egui_wants_pointer_input()))
    })
}

/// Native function: `egui.wants_keyboard_input(handle) -> Bool`
pub fn egui_wants_keyboard_input(args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 1, "egui.wants_keyboard_input")?;
    let handle = expect_handle(&args[0], "egui.wants_keyboard_input handle")?;

    with_egui("egui", "egui.wants_keyboard_input", |service| {
        let session = service
            .sessions
            .get(handle)
            .ok_or_else(|| VmFault::StaleHandle {
                handle: handle.to_string(),
            })?;
        Ok(Value::Bool(session.ctx.egui_wants_keyboard_input()))
    })
}

/// Constructs the `egui` module dictionary with all native operations.
#[must_use]
pub fn create_module() -> Value {
    let entries = vec![
        (
            Value::String(Rc::new("create_context".to_string())),
            Value::native("egui.create_context", 0, egui_create_context),
        ),
        (
            Value::String(Rc::new("destroy_context".to_string())),
            Value::native("egui.destroy_context", 1, egui_destroy_context),
        ),
        (
            Value::String(Rc::new("begin_frame".to_string())),
            Value::native("egui.begin_frame", 2, egui_begin_frame),
        ),
        (
            Value::String(Rc::new("end_frame".to_string())),
            Value::native("egui.end_frame", 1, egui_end_frame),
        ),
        (
            Value::String(Rc::new("begin_window".to_string())),
            Value::native("egui.begin_window", 5, egui_begin_window),
        ),
        (
            Value::String(Rc::new("end_window".to_string())),
            Value::native("egui.end_window", 0, egui_end_window),
        ),
        (
            Value::String(Rc::new("label".to_string())),
            Value::native("egui.label", 1, egui_label),
        ),
        (
            Value::String(Rc::new("heading".to_string())),
            Value::native("egui.heading", 1, egui_heading),
        ),
        (
            Value::String(Rc::new("separator".to_string())),
            Value::native("egui.separator", 0, egui_separator),
        ),
        (
            Value::String(Rc::new("button".to_string())),
            Value::native("egui.button", 1, egui_button),
        ),
        (
            Value::String(Rc::new("checkbox".to_string())),
            Value::native("egui.checkbox", 2, egui_checkbox),
        ),
        (
            Value::String(Rc::new("slider".to_string())),
            Value::native("egui.slider", 4, egui_slider),
        ),
        (
            Value::String(Rc::new("text_edit".to_string())),
            Value::native("egui.text_edit", 2, egui_text_edit),
        ),
        (
            Value::String(Rc::new("progress_bar".to_string())),
            Value::native("egui.progress_bar", 1, egui_progress_bar),
        ),
        (
            Value::String(Rc::new("wants_pointer_input".to_string())),
            Value::native("egui.wants_pointer_input", 1, egui_wants_pointer_input),
        ),
        (
            Value::String(Rc::new("wants_keyboard_input".to_string())),
            Value::native("egui.wants_keyboard_input", 1, egui_wants_keyboard_input),
        ),
    ];

    Value::Dict(Rc::new(RefCell::new(DictMap::from_entries(entries))))
}

/// Registers the egui module in a VM instance with default capabilities.
pub fn register_egui(vm: &mut Vm) {
    vm.define_global("egui", create_module());
}
