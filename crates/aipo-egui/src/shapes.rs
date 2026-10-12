//! Extraction and conversion of egui drawing primitives (shapes) into agnostic Aipo values.

#![forbid(unsafe_code)]

use std::cell::RefCell;
use std::rc::Rc;

use aipo_vm::{DictMap, Value};
use egui::epaint::{ClippedShape, Color32, ColorMode, Pos2, Rect, Shape};

/// Converts an egui `Color32` into an Aipo List `[r, g, b, a]` (0..255).
#[must_use]
pub fn color_to_value(c: Color32) -> Value {
    Value::List(Rc::new(RefCell::new(
        (vec![
            Value::Int(i64::from(c.r())),
            Value::Int(i64::from(c.g())),
            Value::Int(i64::from(c.b())),
            Value::Int(i64::from(c.a())),
        ])
        .into(),
    )))
}

/// Converts an egui `Rect` into an Aipo List `[min_x, min_y, max_x, max_y]`.
#[must_use]
pub fn rect_to_value(r: Rect) -> Value {
    Value::List(Rc::new(RefCell::new(
        (vec![
            Value::Float(f64::from(r.min.x)),
            Value::Float(f64::from(r.min.y)),
            Value::Float(f64::from(r.max.x)),
            Value::Float(f64::from(r.max.y)),
        ])
        .into(),
    )))
}

/// Converts an egui `Pos2` into an Aipo List `[x, y]`.
#[must_use]
pub fn pos_to_value(p: Pos2) -> Value {
    Value::List(Rc::new(RefCell::new(
        (vec![Value::Float(f64::from(p.x)), Value::Float(f64::from(p.y))]).into(),
    )))
}

fn color_mode_to_color(cm: &ColorMode) -> Color32 {
    match cm {
        ColorMode::Solid(c) => *c,
        ColorMode::UV(_) => Color32::WHITE,
    }
}

fn str_val(s: &str) -> Value {
    Value::String(Rc::new(s.to_string()))
}

/// Converts a `ClippedShape` into a structured dictionary for external rendering backends.
#[must_use]
pub fn shape_to_value(clipped: &ClippedShape) -> Value {
    let mut entries = Vec::with_capacity(8);

    entries.push((str_val("clip_rect"), rect_to_value(clipped.clip_rect)));

    match &clipped.shape {
        Shape::Noop => {
            entries.push((str_val("type"), str_val("noop")));
        }
        Shape::Vec(shapes) => {
            entries.push((str_val("type"), str_val("group")));
            let list: Vec<Value> = shapes
                .iter()
                .map(|s| {
                    shape_to_value(&ClippedShape {
                        clip_rect: clipped.clip_rect,
                        shape: s.clone(),
                    })
                })
                .collect();
            entries.push((
                str_val("shapes"),
                Value::List(Rc::new(RefCell::new((list).into()))),
            ));
        }
        Shape::Circle(circle) => {
            entries.push((str_val("type"), str_val("circle")));
            entries.push((str_val("center"), pos_to_value(circle.center)));
            entries.push((str_val("radius"), Value::Float(f64::from(circle.radius))));
            entries.push((str_val("fill"), color_to_value(circle.fill)));
            entries.push((str_val("stroke_color"), color_to_value(circle.stroke.color)));
            entries.push((
                str_val("stroke_width"),
                Value::Float(f64::from(circle.stroke.width)),
            ));
        }
        Shape::LineSegment { points, stroke } => {
            entries.push((str_val("type"), str_val("line")));
            let pts = Value::List(Rc::new(RefCell::new(
                (vec![pos_to_value(points[0]), pos_to_value(points[1])]).into(),
            )));
            entries.push((str_val("points"), pts));
            entries.push((str_val("color"), color_to_value(stroke.color)));
            entries.push((str_val("width"), Value::Float(f64::from(stroke.width))));
        }
        Shape::Path(path) => {
            entries.push((str_val("type"), str_val("path")));
            let pts: Vec<Value> = path.points.iter().copied().map(pos_to_value).collect();
            entries.push((
                str_val("points"),
                Value::List(Rc::new(RefCell::new((pts).into()))),
            ));
            entries.push((str_val("closed"), Value::Bool(path.closed)));
            entries.push((str_val("fill"), color_to_value(path.fill)));
            let stroke_color = match path.stroke.color {
                ColorMode::Solid(c) => c,
                ColorMode::UV(_) => Color32::WHITE,
            };
            entries.push((str_val("stroke_color"), color_to_value(stroke_color)));
            entries.push((
                str_val("stroke_width"),
                Value::Float(f64::from(path.stroke.width)),
            ));
        }
        Shape::Rect(rect_shape) => {
            entries.push((str_val("type"), str_val("rect")));
            entries.push((str_val("rect"), rect_to_value(rect_shape.rect)));
            entries.push((str_val("fill"), color_to_value(rect_shape.fill)));
            entries.push((
                str_val("stroke_color"),
                color_to_value(rect_shape.stroke.color),
            ));
            entries.push((
                str_val("stroke_width"),
                Value::Float(f64::from(rect_shape.stroke.width)),
            ));
            let cr = rect_shape.corner_radius;
            entries.push((
                str_val("corner_radius"),
                Value::List(Rc::new(RefCell::new(
                    (vec![
                        Value::Float(f64::from(cr.nw)),
                        Value::Float(f64::from(cr.ne)),
                        Value::Float(f64::from(cr.se)),
                        Value::Float(f64::from(cr.sw)),
                    ])
                    .into(),
                ))),
            ));
        }
        Shape::Text(text_shape) => {
            entries.push((str_val("type"), str_val("text")));
            entries.push((str_val("pos"), pos_to_value(text_shape.pos)));
            entries.push((
                str_val("text"),
                Value::String(Rc::new(text_shape.galley.text().to_string())),
            ));
            let size = text_shape.galley.size();
            entries.push((
                str_val("size"),
                Value::List(Rc::new(RefCell::new(
                    (vec![
                        Value::Float(f64::from(size.x)),
                        Value::Float(f64::from(size.y)),
                    ])
                    .into(),
                ))),
            ));
            entries.push((str_val("color"), color_to_value(text_shape.fallback_color)));
        }
        Shape::Ellipse(ellipse) => {
            entries.push((str_val("type"), str_val("ellipse")));
            entries.push((str_val("center"), pos_to_value(ellipse.center)));
            let rad = Value::List(Rc::new(RefCell::new(
                (vec![
                    Value::Float(f64::from(ellipse.radius.x)),
                    Value::Float(f64::from(ellipse.radius.y)),
                ])
                .into(),
            )));
            entries.push((str_val("radius"), rad));
            entries.push((str_val("fill"), color_to_value(ellipse.fill)));
            entries.push((
                str_val("stroke_color"),
                color_to_value(ellipse.stroke.color),
            ));
            entries.push((
                str_val("stroke_width"),
                Value::Float(f64::from(ellipse.stroke.width)),
            ));
        }
        Shape::QuadraticBezier(bez) => {
            entries.push((str_val("type"), str_val("bezier")));
            let pts = Value::List(Rc::new(RefCell::new(
                (vec![
                    pos_to_value(bez.points[0]),
                    pos_to_value(bez.points[1]),
                    pos_to_value(bez.points[2]),
                ])
                .into(),
            )));
            entries.push((str_val("points"), pts));
            entries.push((str_val("fill"), color_to_value(bez.fill)));
            entries.push((
                str_val("stroke_color"),
                color_to_value(color_mode_to_color(&bez.stroke.color)),
            ));
            entries.push((
                str_val("stroke_width"),
                Value::Float(f64::from(bez.stroke.width)),
            ));
        }
        Shape::CubicBezier(bez) => {
            entries.push((str_val("type"), str_val("cubic_bezier")));
            let pts = Value::List(Rc::new(RefCell::new(
                (vec![
                    pos_to_value(bez.points[0]),
                    pos_to_value(bez.points[1]),
                    pos_to_value(bez.points[2]),
                    pos_to_value(bez.points[3]),
                ])
                .into(),
            )));
            entries.push((str_val("points"), pts));
            entries.push((str_val("fill"), color_to_value(bez.fill)));
            entries.push((
                str_val("stroke_color"),
                color_to_value(color_mode_to_color(&bez.stroke.color)),
            ));
            entries.push((
                str_val("stroke_width"),
                Value::Float(f64::from(bez.stroke.width)),
            ));
        }
        Shape::Mesh(_) => {
            entries.push((str_val("type"), str_val("mesh")));
        }
        Shape::Callback(_) => {
            entries.push((str_val("type"), str_val("callback")));
        }
    }

    Value::Dict(Rc::new(RefCell::new(DictMap::from_entries(entries))))
}
