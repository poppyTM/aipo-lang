//! Egui context wrapper and frame lifecycle management.

#![forbid(unsafe_code)]

use std::cell::RefCell;
use std::rc::Rc;

use aipo_vm::{DictMap, Value, VmFault};
use egui::epaint::{Rect, pos2, vec2};
use egui::{Context, Event, Id, PointerButton, Pos2 as EPos2, RawInput, Ui, UiBuilder};

use crate::shapes::shape_to_value;

/// Parameters for configuring a single egui frame input.
#[derive(Debug, Clone)]
pub struct FrameInputOptions {
    /// Screen width in virtual points.
    pub width: f32,
    /// Screen height in virtual points.
    pub height: f32,
    /// Mouse pointer X position.
    pub mouse_x: f32,
    /// Mouse pointer Y position.
    pub mouse_y: f32,
    /// Whether the primary mouse button is held down.
    pub mouse_down: bool,
    /// Whether the primary mouse button was clicked this frame.
    pub mouse_clicked: bool,
    /// Elapsed time since the previous frame in seconds.
    pub dt: f32,
    /// Horizontal wheel scroll delta.
    pub scroll_x: f32,
    /// Vertical wheel scroll delta.
    pub scroll_y: f32,
}

impl Default for FrameInputOptions {
    fn default() -> Self {
        Self {
            width: 800.0,
            height: 600.0,
            mouse_x: 0.0,
            mouse_y: 0.0,
            mouse_down: false,
            mouse_clicked: false,
            dt: 0.016,
            scroll_x: 0.0,
            scroll_y: 0.0,
        }
    }
}

/// Agnostic egui session wrapping an `egui::Context` and managing frame passes.
pub struct EguiSession {
    /// Inner egui context.
    pub ctx: Context,
    /// Stack of active `Ui` instances for nested containers and windows.
    pub ui_stack: Vec<Ui>,
    /// Whether a frame pass is currently in progress.
    pub is_frame_active: bool,
    /// Cumulative simulation time.
    pub current_time: f64,
    /// Last observed pointer position.
    pub last_pointer_pos: Option<EPos2>,
    /// Last observed primary mouse button state.
    pub last_mouse_down: bool,
    /// Cached output from the most recently completed frame.
    pub last_output: Option<egui::FullOutput>,
    /// Counter for generating unique window IDs.
    pub window_counter: usize,
}

impl EguiSession {
    /// Creates a new egui session.
    #[must_use]
    pub fn new() -> Self {
        Self {
            ctx: Context::default(),
            ui_stack: Vec::new(),
            is_frame_active: false,
            current_time: 0.0,
            last_pointer_pos: None,
            last_mouse_down: false,
            last_output: None,
            window_counter: 0,
        }
    }

    /// Begins a new immediate-mode frame pass with the provided input options.
    pub fn begin_frame(&mut self, options: &FrameInputOptions) -> Result<(), VmFault> {
        if self.is_frame_active {
            return Err(VmFault::CorruptedBytecode {
                offset: 0,
                reason: "cannot call egui.begin_frame while a frame is already active".to_string(),
            });
        }

        self.current_time += f64::from(options.dt);
        let screen_rect = Rect::from_min_size(
            pos2(0.0, 0.0),
            vec2(options.width.max(1.0), options.height.max(1.0)),
        );

        let mut raw_input = RawInput {
            screen_rect: Some(screen_rect),
            time: Some(self.current_time),
            predicted_dt: options.dt.max(0.001),
            ..Default::default()
        };

        let pointer_pos = pos2(options.mouse_x, options.mouse_y);
        raw_input.events.push(Event::PointerMoved(pointer_pos));
        self.last_pointer_pos = Some(pointer_pos);

        if options.mouse_down != self.last_mouse_down {
            raw_input.events.push(Event::PointerButton {
                pos: pointer_pos,
                button: PointerButton::Primary,
                pressed: options.mouse_down,
                modifiers: Default::default(),
            });
            self.last_mouse_down = options.mouse_down;
        }

        if options.scroll_x != 0.0 || options.scroll_y != 0.0 {
            raw_input.events.push(Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: vec2(options.scroll_x, options.scroll_y),
                modifiers: Default::default(),
                phase: egui::TouchPhase::Move,
            });
        }

        self.ctx.begin_pass(raw_input);
        self.is_frame_active = true;
        self.window_counter = 0;
        self.ui_stack.clear();

        // Push root screen UI
        let root_id = Id::new("__root_screen__");
        let root_builder = UiBuilder::default().max_rect(screen_rect);
        let root_ui = Ui::new(self.ctx.clone(), root_id, root_builder);
        self.ui_stack.push(root_ui);

        Ok(())
    }

    /// Ends the current frame pass and packages the output dictionary.
    pub fn end_frame(&mut self) -> Result<Value, VmFault> {
        if !self.is_frame_active {
            return Err(VmFault::CorruptedBytecode {
                offset: 0,
                reason: "cannot call egui.end_frame without an active frame pass".to_string(),
            });
        }

        self.ui_stack.clear();
        let mut full_output = self.ctx.end_pass();
        self.is_frame_active = false;
        // Headless adapter: no GPU uploader consumes texture deltas, so acknowledge
        // and drop them explicitly (epaint panics on an unhandled drop).
        full_output.textures_delta.clear();

        let shapes_list: Vec<Value> = full_output.shapes.iter().map(shape_to_value).collect();
        let cursor_name = format!("{:?}", full_output.platform_output.cursor_icon);

        let entries = vec![
            (
                Value::String(Rc::new("wants_pointer_input".to_string())),
                Value::Bool(self.ctx.egui_wants_pointer_input()),
            ),
            (
                Value::String(Rc::new("wants_keyboard_input".to_string())),
                Value::Bool(self.ctx.egui_wants_keyboard_input()),
            ),
            (
                Value::String(Rc::new("cursor".to_string())),
                Value::String(Rc::new(cursor_name)),
            ),
            (
                Value::String(Rc::new("shapes".to_string())),
                Value::List(Rc::new(RefCell::new((shapes_list).into()))),
            ),
        ];

        self.last_output = Some(full_output);
        Ok(Value::Dict(Rc::new(RefCell::new(DictMap::from_entries(
            entries,
        )))))
    }

    /// Borrows the currently active top-level `Ui`.
    pub fn current_ui_mut(&mut self) -> Result<&mut Ui, VmFault> {
        self.ui_stack
            .last_mut()
            .ok_or_else(|| VmFault::CorruptedBytecode {
                offset: 0,
                reason: "no active egui UI container in current frame".to_string(),
            })
    }

    /// Begins a window container with a frame background, title and title bar.
    pub fn begin_window(
        &mut self,
        title: &str,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
    ) -> Result<(), VmFault> {
        if !self.is_frame_active {
            return Err(VmFault::CorruptedBytecode {
                offset: 0,
                reason: "cannot begin window outside an active frame".to_string(),
            });
        }

        self.window_counter += 1;
        let window_id = Id::new(title).with(self.window_counter);
        let rect = Rect::from_min_size(pos2(x, y), vec2(width.max(100.0), height.max(50.0)));

        let builder = UiBuilder::default().max_rect(rect);
        let mut window_ui = Ui::new(self.ctx.clone(), window_id, builder);

        // Window styling & header
        let frame = egui::Frame::window(&self.ctx.style_of(self.ctx.theme()));
        window_ui.painter().add(frame.paint(rect));
        window_ui.heading(title);
        window_ui.separator();

        self.ui_stack.push(window_ui);
        Ok(())
    }

    /// Ends the current window container.
    pub fn end_window(&mut self) -> Result<(), VmFault> {
        if self.ui_stack.len() <= 1 {
            return Err(VmFault::CorruptedBytecode {
                offset: 0,
                reason: "cannot end window: no window currently open on ui stack".to_string(),
            });
        }
        self.ui_stack.pop();
        Ok(())
    }
}

impl Default for EguiSession {
    fn default() -> Self {
        Self::new()
    }
}
