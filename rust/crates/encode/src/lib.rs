//! The JSON forms of a display list, of a canvas batch and of a composition (`core/src/encode.cpp`'s
//! `to_json`), for painters and compositors outside C++: Android's Canvas in Kotlin, the web's canvas in
//! TypeScript. `display_list.h`, `canvas_ops.h` and `compositor.h` document the keys. The text is bytes, as
//! the C++ `std::string` holds it: a command's text goes in quoted but otherwise as it is, UTF-8 or not.
//!
//! CSS and canvas px are written to hundredths (`std::round(double(v) * 100) / 100`), matrix components,
//! opacities and gradient offsets exactly; numbers as widget-sandbox's `Json::number` writes them, a
//! number that is not finite as `null`.

#![forbid(unsafe_code)]

use twidget_compositor::{Composition, LayerKey};
use twidget_json::{number, quote};
use twidget_wire::{CanvasOp, Cmd, Color, Command, Composite, OpKind, PaintKind, Path, PathOp, Rect, Stop};

struct Writer {
    out: Vec<u8>,
    fresh: bool,
}

impl Writer {
    fn new() -> Writer {
        Writer { out: Vec::new(), fresh: false }
    }

    fn push(&mut self, c: u8) {
        self.out.push(c);
    }

    fn text(&mut self, s: &str) {
        self.out.extend_from_slice(s.as_bytes());
    }

    fn open(&mut self) {
        self.push(b'{');
        self.fresh = true;
    }

    /// CSS and canvas px: hundredths are below what any painter resolves.
    fn num(&mut self, v: f32) {
        self.text(&number((f64::from(v) * 100.0).round() / 100.0));
    }

    /// Matrix components and gradient offsets keep their precision: a rounded rotation drifts.
    fn exact(&mut self, v: f32) {
        self.text(&number(f64::from(v)));
    }

    fn key(&mut self, k: &str) {
        if !self.fresh {
            self.push(b',');
        }
        self.fresh = false;
        self.push(b'"');
        self.text(k);
        self.text("\":");
    }

    fn argb(&mut self, c: &Color) {
        let v = (u32::from(c.a) << 24) | (u32::from(c.r) << 16) | (u32::from(c.g) << 8) | u32::from(c.b);
        self.text(&v.to_string());
    }

    fn color(&mut self, k: &str, c: &Color) {
        self.key(k);
        self.argb(c);
    }

    fn rect(&mut self, k: &str, r: &Rect) {
        self.key(k);
        self.push(b'[');
        self.num(r.x);
        self.push(b',');
        self.num(r.y);
        self.push(b',');
        self.num(r.w);
        self.push(b',');
        self.num(r.h);
        self.push(b']');
    }

    fn floats(&mut self, k: &str, v: &[f32], precise: bool) {
        self.key(k);
        self.push(b'[');
        for (i, &x) in v.iter().enumerate() {
            if i > 0 {
                self.push(b',');
            }
            if precise {
                self.exact(x);
            } else {
                self.num(x);
            }
        }
        self.push(b']');
    }

    fn stops(&mut self, k: &str, list: &[Stop]) {
        self.key(k);
        self.push(b'[');
        for (i, s) in list.iter().enumerate() {
            if i > 0 {
                self.push(b',');
            }
            self.exact(s.offset);
            self.push(b',');
            self.argb(&s.color);
        }
        self.push(b']');
    }

    /// A path as "o" (op codes: 0 move, 1 line, 2 quad, 3 cubic, 4 close) and "v" (their points).
    fn geometry(&mut self, ops: &[PathOp], points: &[f32]) {
        self.key("o");
        self.push(b'[');
        for (i, op) in ops.iter().enumerate() {
            if i > 0 {
                self.push(b',');
            }
            self.text(&op.as_u8().to_string());
        }
        self.push(b']');
        self.floats("v", points, false);
    }

    fn flag(&mut self, k: &str, on: bool) {
        if on {
            self.key(k);
            self.push(b'1');
        }
    }
}

fn svg_path(w: &mut Writer, p: &Path) {
    w.open();
    w.geometry(&p.ops, &p.points);
    w.flag("e", p.evenodd);
    if p.fill && p.gradient != 0 && p.stops.len() >= 2 {
        w.key("g");
        w.text(&p.gradient.to_string());
        w.floats("ga", &[p.x1, p.y1, p.x2, p.y2, p.r, p.fx, p.fy], false);
        w.stops("s", &p.stops);
    } else if p.fill {
        w.color("f", &p.fill_color);
    }
    if p.stroke {
        w.color("sc", &p.stroke_color);
        if p.stroke_gradient != 0 && p.stroke_stops.len() >= 2 {
            w.key("sg");
            w.text(&p.stroke_gradient.to_string());
            w.floats("sga", &p.sg, false);
            w.stops("ss", &p.stroke_stops);
        }
        w.key("sw");
        w.num(p.stroke_width);
        w.key("lc");
        w.text(&p.line_cap.to_string());
        w.key("lj");
        w.text(&p.line_join.to_string());
        w.key("ml");
        w.num(p.miter_limit);
        if !p.dashes.is_empty() {
            w.floats("d", &p.dashes, false);
            w.key("do");
            w.num(p.dash_offset);
        }
    }
    w.push(b'}');
}

/// A display list as an array of `{"k": <Cmd>, ...}` (display_list.h documents the keys). Flags are present
/// only when set.
pub fn list_json(list: &[Command]) -> Vec<u8> {
    let mut w = Writer::new();
    w.out.reserve(list.len() * 64 + 2);
    w.push(b'[');
    for (i, c) in list.iter().enumerate() {
        if i > 0 {
            w.push(b',');
        }
        w.open();
        w.key("k");
        w.text(&c.kind.as_u8().to_string());
        if !matches!(c.kind, Cmd::ClipPop | Cmd::GroupPop | Cmd::FilterPop | Cmd::Text | Cmd::Path) {
            w.rect("r", &c.rect);
        }
        if c.radii.any() {
            let q = &c.radii;
            w.floats("q", &[q.tlx, q.tly, q.trx, q.try_, q.brx, q.bry, q.blx, q.bly], false);
        }
        if c.has_clip {
            w.rect("l", &c.clip);
        }
        match c.kind {
            Cmd::FillRect | Cmd::FillEllipse => w.color("c", &c.color),
            Cmd::StrokeRect | Cmd::StrokeEllipse => {
                w.color("c", &c.color);
                w.key("w");
                w.num(c.width);
            }
            Cmd::Text => {
                w.color("c", &c.color);
                w.text(&format!(",\"f\":{},\"x\":", c.font));
                w.num(c.x);
                w.text(",\"y\":");
                w.num(c.y);
                w.text(",\"t\":");
                w.out.extend_from_slice(&quote(&c.text));
            }
            Cmd::Image => {
                if c.canvas != 0 {
                    w.text(&format!(",\"cv\":{}", c.canvas));
                } else {
                    w.text(&format!(",\"i\":{}", c.image));
                }
            }
            Cmd::Linear | Cmd::Radial | Cmd::Conic => {
                w.floats("a", &[c.ax, c.ay, c.bx, c.by], false);
                w.stops("s", &c.stops);
            }
            Cmd::ClipPush => {
                if let Some(path) = &c.path {
                    w.key("p");
                    w.open();
                    w.geometry(&path.ops, &path.points);
                    w.flag("e", path.evenodd);
                    w.push(b'}');
                }
            }
            Cmd::Path => {
                if let Some(path) = &c.path {
                    w.key("p");
                    svg_path(&mut w, path);
                }
            }
            Cmd::GroupPush => {
                w.floats("m", &c.matrix, true);
                w.key("o");
                w.exact(c.opacity);
                w.flag("h", c.hidden);
            }
            Cmd::Shadow => {
                w.color("c", &c.color);
                w.floats("a", &[c.ax, c.ay, c.bx, c.by], false);
                w.flag("n", c.inset);
            }
            Cmd::FilterPush | Cmd::Backdrop => {
                if let Some(filters) = &c.filter {
                    w.key("fl");
                    w.push(b'[');
                    for (k, f) in filters.iter().enumerate() {
                        if k > 0 {
                            w.push(b',');
                        }
                        w.push(b'[');
                        w.text(&f.kind.as_u8().to_string());
                        w.push(b',');
                        w.exact(f.amount);
                        w.push(b',');
                        w.num(f.x);
                        w.push(b',');
                        w.num(f.y);
                        w.push(b',');
                        w.argb(&f.color);
                        w.push(b']');
                    }
                    w.push(b']');
                }
            }
            Cmd::ClipPop | Cmd::GroupPop | Cmd::FilterPop => {}
        }
        w.push(b'}');
    }
    w.push(b']');
    w.out
}

/// A canvas batch as an array of `{"k": <kind>, ...}` (canvas_ops.h documents the keys): a path as "o" and
/// "v", the paint as "pk" with "c" or "pa" and "s", the line style, text as "f", "t" and "tp", an image as
/// "i" or "cv" and "ir", putImageData's pixels as base64 in "px", the clips as "cl".
pub fn ops_json(ops: &[CanvasOp]) -> Vec<u8> {
    let mut w = Writer::new();
    w.out.reserve(ops.len() * 96 + 2);
    w.push(b'[');
    for (i, op) in ops.iter().enumerate() {
        if i > 0 {
            w.push(b',');
        }
        w.open();
        w.key("k");
        w.text(&op.kind.as_u8().to_string());
        match op.kind {
            OpKind::Reset => {}
            OpKind::PutImage => {
                // Pixels travel as base64 of straight RGBA, row by row.
                w.key("x");
                w.num(op.dx);
                w.key("y");
                w.num(op.dy);
                w.text(&format!(",\"w\":{},\"h\":{}", op.w, op.h));
                if let Some(rgba) = &op.rgba {
                    w.text(",\"px\":");
                    w.out.extend_from_slice(&quote(twidget_base64::encode(rgba)));
                }
            }
            _ => 'other: {
                if matches!(op.kind, OpKind::Fill | OpKind::Stroke | OpKind::Clear) {
                    w.geometry(&op.path.ops, &op.path.points);
                }
                if op.kind == OpKind::Clear {
                    break 'other;
                }
                w.flag("e", op.evenodd);
                w.floats("m", &op.matrix, true);
                if op.alpha != 1.0 {
                    w.key("a");
                    w.exact(op.alpha);
                }
                if op.composite != Composite::SourceOver {
                    w.key("cm");
                    w.text(&op.composite.as_u8().to_string());
                }
                if op.has_shadow() {
                    w.color("shc", &op.shadow_color);
                    w.floats("sh", &[op.shadow_x, op.shadow_y, op.shadow_blur], false);
                }
                if matches!(op.kind, OpKind::Fill | OpKind::Stroke | OpKind::Text) {
                    // The paint: "pk" 0 solid ("c"), 1 linear, 2 radial, 3 conic ("pa" in user space, "s" stops).
                    w.key("pk");
                    w.text(&op.paint.kind.as_u8().to_string());
                    if op.paint.kind == PaintKind::Solid {
                        w.color("c", &op.paint.color);
                    } else {
                        w.floats("pa", &op.paint.args, true);
                        w.stops("s", &op.paint.stops);
                    }
                }
                if op.kind == OpKind::Stroke || (op.kind == OpKind::Text && op.stroke_text) {
                    w.key("lw");
                    w.exact(op.line_width);
                    w.key("lc");
                    w.text(&op.line_cap.to_string());
                    w.key("lj");
                    w.text(&op.line_join.to_string());
                    w.key("ml");
                    w.num(op.miter_limit);
                    if !op.dashes.is_empty() {
                        w.floats("d", &op.dashes, false);
                        w.key("do");
                        w.num(op.dash_offset);
                    }
                }
                if op.kind == OpKind::Text {
                    w.text(&format!(",\"f\":{},\"t\":", op.font));
                    w.out.extend_from_slice(&quote(&op.text));
                    w.floats("tp", &[op.x, op.y, op.scale_x, op.text_dx], true);
                    w.flag("st", op.stroke_text);
                }
                if op.kind == OpKind::Image {
                    if op.canvas != 0 {
                        w.text(&format!(",\"cv\":{}", op.canvas));
                    } else {
                        w.text(&format!(",\"i\":{}", op.image));
                    }
                    w.floats("ir", &[op.sx, op.sy, op.sw, op.sh, op.dx, op.dy, op.dw, op.dh], true);
                    w.flag("ns", !op.smoothing);
                }
            }
        }
        if let Some(clips) = &op.clips
            && !clips.is_empty()
            && op.kind != OpKind::Reset
            && op.kind != OpKind::PutImage
        {
            w.text(",\"cl\":[");
            for (i, clip) in clips.iter().enumerate() {
                if i > 0 {
                    w.push(b',');
                }
                w.open();
                w.geometry(&clip.path.ops, &clip.path.points);
                w.flag("e", clip.evenodd);
                w.push(b'}');
            }
            w.push(b']');
        }
        w.push(b'}');
    }
    w.push(b']');
    w.out
}

/// A composition for compositors outside C++, without the layers' content (`list_json` of `Layer::content`
/// gives that): `{"v": version, "l": [{"id", "b" bounds, "cv" content_version, "rs" raster_scale, "l" clip and
/// "q" its radii when clipped, "tv" track_version, "ts" start, "td" duration, "tr" 1 when it repeats, "k" keys
/// flat as t, a, b, c, d, e, f, opacity, "p" parent when nested}]}`.
pub fn composition_json(composition: &Composition) -> Vec<u8> {
    let mut w = Writer::new();
    w.open();
    w.key("v");
    w.text(&composition.version.to_string());
    w.key("l");
    w.push(b'[');
    for (i, layer) in composition.layers.iter().enumerate() {
        if i > 0 {
            w.push(b',');
        }
        w.open();
        w.key("id");
        w.text(&layer.id.to_string());
        w.rect("b", &layer.bounds);
        w.key("cv");
        w.text(&layer.content_version.to_string());
        w.key("rs");
        w.exact(layer.raster_scale);
        if layer.has_clip {
            w.rect("l", &layer.clip);
            if layer.clip_radii.any() {
                let q = &layer.clip_radii;
                w.floats("q", &[q.tlx, q.tly, q.trx, q.try_, q.brx, q.bry, q.blx, q.bly], false);
            }
        }
        w.key("tv");
        w.text(&layer.track_version.to_string());
        w.key("ts");
        w.text(&number(layer.track.start));
        w.key("td");
        w.text(&number(layer.track.duration));
        w.flag("tr", layer.track.repeat);
        let keys: Vec<f32> = layer
            .track
            .keys
            .iter()
            .flat_map(|k: &LayerKey| [k.t, k.matrix.a, k.matrix.b, k.matrix.c, k.matrix.d, k.matrix.e, k.matrix.f, k.opacity])
            .collect();
        w.floats("k", &keys, true);
        if layer.parent != 0 {
            w.key("p");
            w.text(&layer.parent.to_string());
        }
        w.push(b'}');
    }
    w.text("]}");
    w.out
}
