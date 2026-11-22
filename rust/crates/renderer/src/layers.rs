//! Stage 4.5 of renderer.cpp: the paint policy's heavy boxes counted in the renderer, the compositor layers
//! (`update_composition`: movers, shimmers, scripted, SVG, canvas and still layers, nested ones, the coverage
//! rounds and the budget), scripted layers moved without a build, the paint (the display list the renderer
//! controls, the layers' content kept or drawn again, the list handed out again), the damage between paints, and
//! the layers the hit test and the selection read. Every call into litehtml's document goes through
//! [`Engine`], where and in the order renderer.cpp makes it; the state is [`Layers`], the renderer's members.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use twidget_animations::{Animator, Declared, Motion, Plan, StyleOverrides, StyleValues};
use twidget_compositor::{
    Composition, Layer, LayerKey, LayerTrack, PaintSummary, bake_track, damage_between, drop_inert_clips, paint_digest, resume_track,
    summarize,
};
use twidget_events::Events;
use twidget_json::Json;
use twidget_paint::{BoxPaint, PaintCounter, Verdict};
use twidget_shadow::{Document, NodeKind, node_id};
use twidget_wire::{Affine, Command, DisplayList, Radii, Rect};

use crate::{Backend, BoxStyle, Clip, Core, Engine, css, max_f32, min_f32, min_f64, still};

/// Layers: at most this many per widget, this many device pixels in all, and no track shorter than this (a
/// frame or two is cheaper to paint than to hand over).
pub const MAX_LAYERS: usize = 256;
/// Static elements painted over a wanted layer that may become layers of their own to let it be one.
pub const MAX_OBSCURERS: usize = 3;
pub const MAX_LAYER_PIXELS: f64 = 4096.0 * 4096.0;
pub const MIN_LAYER_SPAN: f64 = 0.1;
/// An element moved by style ops in three batches running becomes a scripted layer: batches at most this many
/// apart, or this close in time, continue a streak.
pub const SCRIPTED_STREAK_BATCHES: u64 = 4;
pub const SCRIPTED_STREAK_GAP_S: f64 = 0.12;
pub const MAX_SCRIPTED_AREA: f64 = 2e6;
pub const LAYER_MARGIN: f32 = 48.0;

/// What a still layer reads of what the document declares.
const LAYER_DECLARED: [&str; 6] = ["transform", "translate", "rotate", "scale", "opacity", "transform-origin"];

/// An element the composition lifts out of the document (renderer.h's `Promotion`).
#[derive(Clone, Debug, PartialEq)]
pub struct Promotion {
    /// Its border box (the C++'s `box`); a shimmer's tile.
    pub rect: Rect,
    /// What the layer can cover over its whole track.
    pub reach: Rect,
    pub raster_scale: f32,
    pub has_clip: bool,
    pub clip: Rect,
    pub clip_radii: Radii,
    pub signature: u64,
    pub track_version: u64,
    pub track: LayerTrack,
    pub order: usize,
    /// A still layer a script moves (apply_scripted), within `reach` and up to `raster_scale`.
    pub scripted: bool,
    /// What the document declared for the layer properties at the last build.
    pub base: Declared,
    /// A shimmer: the element's background tile alone, `rect`, sliding under `clip`; the document paints the rest
    /// of the element.
    pub shimmer: bool,
    pub tile: DisplayList,
    pub tile_version: u64,
    /// An SVG a script keeps changing: a still layer whose content repaints without the document.
    pub svg: bool,
    /// A canvas drawn again and again: a still layer whose drawing repaints only it.
    pub canvas: bool,
    /// The layer this one moves with, and the clip clip_of found before it was dropped or kept, to tell a clip of
    /// its own from the parent's.
    pub parent: i64,
    pub found_clip: bool,
    pub found_clip_rect: Rect,
}

impl Default for Promotion {
    fn default() -> Promotion {
        Promotion {
            rect: Rect::default(),
            reach: Rect::default(),
            raster_scale: 1.0,
            has_clip: false,
            clip: Rect::default(),
            clip_radii: Radii::default(),
            signature: 0,
            track_version: 0,
            track: LayerTrack::default(),
            order: 0,
            scripted: false,
            base: Declared::new(),
            shimmer: false,
            tile: DisplayList::new(),
            tile_version: 0,
            svg: false,
            canvas: false,
            parent: 0,
            found_clip: false,
            found_clip_rect: Rect::default(),
        }
    }
}

/// An element's streak of batches that moved it through the layer properties.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ScriptedHit {
    pub count: i32,
    pub batch: u64,
    pub at: f64,
}

/// A rule-5 animation the ledger counts: its box's key, which of the box's animations it is, its paint.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CountedMotion {
    pub key: i64,
    pub id: Vec<u8>,
    pub paint: BoxPaint,
}

/// Stage 4.5's members of renderer.cpp's `Renderer`, by their C++ names.
#[derive(Clone, Debug)]
pub struct Layers {
    pub compositing: bool,
    pub promoted: BTreeMap<i64, Promotion>,
    pub layer_ids: HashSet<i64>,
    pub composition: Composition,
    pub wake_at: f64,
    pub composition_key: u64,
    pub next_version: u64,
    /// The batches running that changed each SVG; three make it an SVG layer.
    pub svg_streaks: HashMap<i64, i32>,
    pub svg_layers: BTreeSet<i64>,
    /// How often each canvas drew; a canvas id is its node's.
    pub canvas_draws: HashMap<i32, u64>,
    pub canvas_layers: BTreeSet<i64>,
    pub scripted_hits: HashMap<i64, ScriptedHit>,
    pub ops_batch: u64,
    pub scripted_candidates: BTreeSet<i64>,
    /// Since the last paint, only scripted layers moved.
    pub composition_only: bool,
    /// Kept while there are scripted or canvas layers, for reuse_paint.
    pub painted_list: DisplayList,
    pub painted_list_valid: bool,
    pub painted_key: (u64, u32, u64),
    pub painted: PaintSummary,
    pub painted_valid: bool,
    pub painted_size: (u32, u32),
    pub damage: Vec<Rect>,
    pub damage_valid: bool,
    pub canvases_drawn: HashSet<i32>,
    pub painted_layouts: u64,
    /// Counted unless the host's `policy` is compat; `paintReports` (tooling): a `paint` message when the counts
    /// change.
    pub paint_on: bool,
    pub paint_reports: bool,
    pub paint_reported: (i32, i32),
    pub paint_scan: bool,
    pub paint: PaintCounter<i64>,
    pub motion_hash: u64,
    pub motion_stale: bool,
    pub motions: Vec<Motion>,
    pub motion_counted: Vec<CountedMotion>,
}

impl Default for Layers {
    fn default() -> Layers {
        Layers {
            compositing: false,
            promoted: BTreeMap::new(),
            layer_ids: HashSet::new(),
            composition: Composition::default(),
            wake_at: f64::INFINITY,
            composition_key: 0,
            next_version: 1,
            svg_streaks: HashMap::new(),
            svg_layers: BTreeSet::new(),
            canvas_draws: HashMap::new(),
            canvas_layers: BTreeSet::new(),
            scripted_hits: HashMap::new(),
            ops_batch: 0,
            scripted_candidates: BTreeSet::new(),
            composition_only: false,
            painted_list: DisplayList::new(),
            painted_list_valid: false,
            painted_key: (0, 0, 0),
            painted: PaintSummary::default(),
            painted_valid: false,
            painted_size: (0, 0),
            damage: Vec::new(),
            damage_valid: false,
            canvases_drawn: HashSet::new(),
            painted_layouts: 0,
            paint_on: true,
            paint_reports: false,
            paint_reported: (-1, -1),
            paint_scan: true,
            paint: PaintCounter::new(),
            motion_hash: 0,
            motion_stale: true,
            motions: Vec::new(),
            motion_counted: Vec::new(),
        }
    }
}

impl Layers {
    fn version(&mut self) -> u64 {
        let v = self.next_version;
        self.next_version = self.next_version.wrapping_add(1);
        v
    }

    /// The paint summary and the last list go: the next paint is painted whole (`trim`'s part).
    pub(crate) fn trim(&mut self) {
        self.painted = PaintSummary::default();
        self.painted_valid = false;
        self.painted_list = DisplayList::new();
        self.painted_list_valid = false;
    }

    /// A canvas was released (`forget_canvas`).
    pub(crate) fn canvas_released(&mut self, canvas: i32) {
        self.canvas_draws.remove(&canvas);
        self.canvas_layers.remove(&i64::from(canvas));
    }
}

// ── helpers ──

/// The bounding box of `r` mapped through `m`.
fn bounds_under(r: &Rect, m: &Affine) -> Rect {
    let xs = [r.x, r.x + r.w, r.x, r.x + r.w];
    let ys = [r.y, r.y, r.y + r.h, r.y + r.h];
    let (mut x0, mut y0, mut x1, mut y1) = (1e30f32, 1e30f32, -1e30f32, -1e30f32);
    for i in 0..4 {
        let x = m.apply_x(xs[i], ys[i]);
        let y = m.apply_y(xs[i], ys[i]);
        x0 = min_f32(x0, x);
        y0 = min_f32(y0, y);
        x1 = max_f32(x1, x);
        y1 = max_f32(y1, y);
    }
    Rect::new(x0, y0, x1 - x0, y1 - y0)
}

/// When a track's key at `t` (0..1) applies. A loop's end is the next iteration's start already, so its last key
/// reads a moment before.
/// What a layer's sample reads of `property`: the last value the animations give it, else its base value.
fn sampled<'a>(values: &'a StyleValues, animator: &'a Animator, id: i64, property: &[u8]) -> &'a [u8] {
    for (k, v) in values.iter().rev() {
        if k == property {
            return v;
        }
    }
    animator.base_bytes(id, property)
}

fn plan_time(plan: &Plan, t: f64) -> f64 {
    let t = if plan.repeat && t >= 1.0 { 1.0 - 1e-6 } else { t };
    plan.start + t * plan.duration
}

fn intersection(a: &Rect, b: &Rect) -> Rect {
    let x0 = max_f32(a.x, b.x);
    let y0 = max_f32(a.y, b.y);
    let x1 = min_f32(a.x + a.w, b.x + b.w);
    let y1 = min_f32(a.y + a.h, b.y + b.h);
    Rect::new(x0, y0, max_f32(0.0, x1 - x0), max_f32(0.0, y1 - y0))
}

fn contains(outer: &Rect, inner: &Rect) -> bool {
    const SLACK: f32 = 0.01;
    inner.x >= outer.x - SLACK
        && inner.y >= outer.y - SLACK
        && inner.x + inner.w <= outer.x + outer.w + SLACK
        && inner.y + inner.h <= outer.y + outer.h + SLACK
}

/// `std::memcmp` of two rectangles: their bits.
fn same_rect(a: &Rect, b: &Rect) -> bool {
    [a.x, a.y, a.w, a.h].map(f32::to_bits) == [b.x, b.y, b.w, b.h].map(f32::to_bits)
}

fn radii_bits(q: &Radii) -> [u32; 8] {
    [q.tlx, q.tly, q.trx, q.try_, q.brx, q.bry, q.blx, q.bly].map(f32::to_bits)
}

/// `std::max({...})` of a clip's radii: the first of the largest.
fn corner_of(q: &Radii) -> f32 {
    let mut largest = q.tlx;
    for v in [q.tly, q.trx, q.try_, q.brx, q.bry, q.blx, q.bly] {
        if largest < v {
            largest = v;
        }
    }
    largest
}

/// What a shimmer layer animates: its background tile's position along x.
fn is_shimmer_property(property: &[u8]) -> bool {
    property == b"background-position" || property == b"background-position-x"
}

/// What a compositor layer animates; an element animating anything else is painted per frame.
pub fn is_layer_property(property: &[u8]) -> bool {
    matches!(property, b"transform" | b"opacity" | b"translate" | b"rotate" | b"scale")
}

/// The bytes a C string holds: up to a NUL.
fn c_str(s: &[u8]) -> &[u8] {
    s.iter().position(|&c| c == 0).map_or(s, |n| &s[..n])
}

/// Where a background-position value puts a tile `tile` wide in `area`, along x; and along y when the value has a
/// second component, which must leave the tile where `tile` has it.
fn shimmer_x(value: &[u8], area: &Rect, tile: &Rect) -> Option<f32> {
    let length = |token: &[u8], room: f32| -> Option<f32> {
        if token == b"left" || token == b"top" {
            return Some(0.0);
        }
        if token == b"center" {
            return Some(room / 2.0);
        }
        if token == b"right" || token == b"bottom" {
            return Some(room);
        }
        let text = c_str(token);
        let (v, n) = twidget_cstd::strtof(text);
        if n == 0 {
            return None;
        }
        let unit = &text[n..];
        if unit == b"%" {
            return Some(room * v / 100.0);
        }
        if unit == b"px" || (unit.is_empty() && v == 0.0) {
            return Some(v);
        }
        None
    };
    let mut tokens: Vec<&[u8]> = Vec::new();
    let mut at = 0;
    while at < value.len() {
        let Some(start) = value[at..].iter().position(|&c| c != b' ').map(|i| at + i) else { break };
        let stop = value[start..].iter().position(|&c| c == b' ').map(|i| start + i);
        tokens.push(&value[start..stop.unwrap_or(value.len())]);
        at = stop.unwrap_or(value.len());
    }
    if tokens.is_empty() || tokens.len() > 2 {
        return None;
    }
    let x = length(tokens[0], area.w - tile.w)?;
    if tokens.len() == 2 {
        let y = length(tokens[1], area.h - tile.h);
        match y {
            Some(y) if (area.y + y - tile.y).abs() <= 0.5 => {}
            _ => return None,
        }
    }
    Some(area.x + x)
}

/// MSVC's `std::hash<double>`: FNV-1a over the bytes, -0 as 0 (the composition key's; another library's hash
/// changes the key's value, not when it is the same).
fn hash_double(v: f64) -> u64 {
    let v = if v == 0.0 { 0.0 } else { v };
    let mut h = 14695981039346656037u64;
    for b in v.to_le_bytes() {
        h = (h ^ u64::from(b)).wrapping_mul(1099511628211);
    }
    h
}

fn is_ancestor(doc: &Document, ancestor: i64, id: i64) -> bool {
    if ancestor == 0 {
        return false;
    }
    let mut p = doc.node(id).and_then(|r| doc.get(r).parent);
    while let Some(r) = p {
        let n = doc.get(r);
        if n.id == ancestor {
            return true;
        }
        p = n.parent;
    }
    false
}

/// What a box adds (paint_policy.h, rules 1 to 4) from what `box_paint_of` reads; `opacity` the element's when a
/// scripted layer holds a newer one than the document.
fn box_paint_of(style: &BoxStyle, pseudo: bool, opacity: Option<f32>) -> BoxPaint {
    // A disabled or pressed <button>'s opacity is its own dimming, the UA layer's or the author's.
    let dimmed = !pseudo && style.dimmed;
    BoxPaint {
        backdrop: style.backdrop,
        heavy: style.backdrop || style.filter || style.blurred || (!dimmed && opacity.unwrap_or(style.opacity) < 1.0),
    }
}

/// Whether `ancestor` keeps what it holds out of layers: it animates something but its own background alone.
fn blocks<E: Engine>(engine: &mut E, animator: &Animator, animated: &HashSet<i64>, local: &mut HashMap<i64, bool>, ancestor: i64) -> bool {
    if !animated.contains(&ancestor) {
        return false;
    }
    let background = match local.get(&ancestor) {
        Some(&v) => v,
        None => {
            local.insert(ancestor, false);
            let properties = animator.animated_properties(ancestor);
            let v = properties.len() == 1 && properties[0] == b"background-color" && engine.background_is_local(ancestor);
            local.insert(ancestor, v);
            v
        }
    };
    !background
}

/// A child goes with its parent, and only when nothing else is composited between them.
fn drop_orphans(doc: &Document, set: &mut BTreeMap<i64, Promotion>) {
    let mut dropped = true;
    while dropped {
        dropped = false;
        let ids: Vec<i64> = set.keys().copied().collect();
        for id in ids {
            let (parent, order) = match set.get(&id) {
                Some(p) => (p.parent, p.order),
                None => continue,
            };
            let mut orphan = false;
            if parent != 0 {
                match set.get(&parent).map(|up| up.order) {
                    None => orphan = true,
                    Some(up) => {
                        for (&other, q) in set.iter() {
                            if orphan {
                                break;
                            }
                            orphan = other != parent && other != id && q.order > up && q.order < order && !is_ancestor(doc, parent, other);
                        }
                    }
                }
            }
            if orphan {
                set.remove(&id);
                dropped = true;
            }
        }
    }
}

fn apply_clip(p: &mut Promotion, clip: &Clip) {
    p.has_clip = clip.has;
    if clip.has {
        p.clip = clip.rect;
        p.clip_radii = clip.radii;
    }
}

/// The track an old promotion keeps for a new one.
type Kept = Option<(LayerTrack, u64)>;

impl<E: Engine, B: Backend> Core<E, B> {
    fn drop_all_layers(&mut self) {
        if self.layers.promoted.is_empty() {
            return;
        }
        self.layers.promoted.clear();
        self.layers.layer_ids.clear();
        self.layers.composition_key = 0;
        self.dirty = true;
    }

    /// `update_composition`: which elements the compositor animates, still or along a baked track.
    pub(crate) fn update_composition(&mut self, doc: &Document, now: f64) {
        self.layers.wake_at = f64::INFINITY;
        if !self.layers.compositing || still(doc) {
            self.drop_all_layers();
            return;
        }
        struct Planned {
            id: i64,
            plan: Plan,
            signature: u64,
        }
        struct Want {
            id: i64,
            plan: Plan,
            rect: Rect,
            signature: u64,
        }
        let iterations = self.interest.contains(&b"animationiteration"[..]);
        let ids = self.animator.animated_ids(now);
        // What decides the layers: the layout, the animations and where their plans stand. The same as last time,
        // nothing below can come out differently, so a frame stops here.
        let mut planned: Vec<Planned> = Vec::new();
        let mut key = self.container.layouts().wrapping_mul(0x9E3779B97F4A7C15) ^ hash_double(self.width);
        for &id in &ids {
            let Some(plan) = self.animator.plan(id, now, iterations) else { continue };
            self.layers.wake_at = min_f64(self.layers.wake_at, plan.wake);
            let usable = plan.ready && (plan.repeat || plan.duration >= MIN_LAYER_SPAN);
            let signature = self.animator.signature(id);
            key = key.wrapping_mul(1099511628211) ^ (id as u64) ^ (signature << 2) ^ u64::from(plan.repeat) ^ (u64::from(usable) << 1);
            if usable {
                planned.push(Planned { id, plan, signature });
            }
        }
        if key == self.layers.composition_key && self.layers.composition_key != 0 {
            return;
        }
        self.layers.composition_key = key;

        // A layer holds a still picture: nothing inside or around it may animate, but for an ancestor that only
        // repaints its own background. One walk up from each animated element marks the elements with something
        // animated inside.
        let animated: HashSet<i64> = ids.iter().copied().collect();
        // Movers: elements whose usable animations touch only layer properties. One inside another becomes a
        // layer inside the other's (nested), so neither rules the other out.
        let mut movers: HashSet<i64> = HashSet::new();
        let mut mover_elements = self.engine.elements(std::iter::empty());
        for p in &planned {
            let properties = self.animator.animated_properties(p.id);
            let Some(n) = doc.node(p.id).map(|r| doc.get(r)) else { continue };
            if n.svg || n.tag == b"svg" || !properties.iter().all(|name| is_layer_property(name)) {
                continue;
            }
            movers.insert(p.id);
            if self.engine.has_element(p.id) {
                self.engine.insert_element(&mut mover_elements, p.id);
            }
        }
        let mut holds_animated: HashSet<i64> = HashSet::new();
        for &id in &ids {
            if movers.contains(&id) {
                continue;
            }
            let mut p = doc.node(id).and_then(|r| doc.get(r).parent);
            while let Some(r) = p {
                let n = doc.get(r);
                if n.id != 0 && !holds_animated.insert(n.id) {
                    break;
                }
                p = n.parent;
            }
        }
        let mut local_background: HashMap<i64, bool> = HashMap::new();
        let mut shimmers: Vec<usize> = Vec::new();
        let mut wanted: Vec<Want> = Vec::new();
        for (i, p) in planned.iter().enumerate() {
            let id = p.id;
            let properties = self.animator.animated_properties(id);
            if !properties.iter().all(|name| is_layer_property(name)) {
                if !properties.is_empty() && properties.iter().all(|name| is_shimmer_property(name)) {
                    shimmers.push(i);
                }
                continue;
            }
            let Some(n) = doc.node(id).map(|r| doc.get(r)) else { continue };
            if n.svg || n.tag == b"svg" || holds_animated.contains(&id) {
                continue;
            }
            let mut blocked = false;
            let mut a = n.parent;
            while !blocked {
                let Some(r) = a else { break };
                let up = doc.get(r);
                blocked = up.id != 0
                    && !movers.contains(&up.id)
                    && blocks(&mut self.engine, &self.animator, &animated, &mut local_background, up.id);
                a = up.parent;
            }
            if blocked {
                continue;
            }
            if !self.engine.has_element(id) {
                continue;
            }
            let Some(rect) = self.engine.layer_candidate(id, Some(&mover_elements)) else { continue };
            wanted.push(Want { id, plan: p.plan, rect, signature: p.signature });
        }
        self.layers.scripted_candidates.retain(|id| doc.node(*id).is_some());
        self.layers.svg_layers.retain(|id| doc.node(*id).is_some());
        if wanted.is_empty()
            && self.layers.scripted_candidates.is_empty()
            && shimmers.is_empty()
            && self.layers.svg_layers.is_empty()
            && self.layers.canvas_layers.is_empty()
        {
            if !self.layers.promoted.is_empty() {
                self.layers.promoted.clear();
                self.layers.layer_ids.clear();
                self.layers.composition_only = false;
                self.dirty = true;
            }
            return;
        }

        let mut next: BTreeMap<i64, Promotion> = BTreeMap::new();
        for want in &wanted {
            let id = want.id;
            let mut p = Promotion { rect: want.rect, signature: want.signature, ..Promotion::default() };
            let Some(clip) = self.engine.clip_of(id) else { continue };
            apply_clip(&mut p, &clip);
            p.found_clip = p.has_clip;
            p.found_clip_rect = p.clip;
            // A running track stays when nothing about it changed, and while it still covers what the animations
            // will do: the end of a one-shot track, or the same loop.
            let kept: Kept = self.layers.promoted.get(&id).and_then(|old| {
                let same_span = if want.plan.repeat {
                    old.track.repeat && (old.track.duration - want.plan.duration).abs() < 1e-6
                } else {
                    !old.track.repeat && (old.track.start + old.track.duration - (want.plan.start + want.plan.duration)).abs() < 1e-3
                };
                (same_span && old.signature == want.signature && same_rect(&old.rect, &want.rect))
                    .then(|| (old.track.clone(), old.track_version))
            });
            if let Some((track, version)) = kept {
                p.track = track;
                p.track_version = version;
            } else {
                let rect = want.rect;
                let Some(origin) = css::parse_origin(&self.animator.base_value(id, b"transform-origin"), rect.w, rect.h) else { continue };
                let to = Affine::translate(rect.x + origin.0, rect.y + origin.1);
                let from = Affine::translate(-(rect.x + origin.0), -(rect.y + origin.1));
                let plan = want.plan;
                let animator = &self.animator;
                let sample = |t: f64, out: &mut LayerKey| -> bool {
                    let values = animator.peek(id, plan_time(&plan, t));
                    let translate = css::parse_translate(sampled(&values, animator, id, b"translate"), rect.w, rect.h);
                    let rotate = css::parse_rotate(sampled(&values, animator, id, b"rotate"));
                    let scale = css::parse_scale(sampled(&values, animator, id, b"scale"));
                    let transform = css::parse_transform(sampled(&values, animator, id, b"transform"), rect.w, rect.h);
                    let opacity = css::parse_opacity(sampled(&values, animator, id, b"opacity"));
                    let (Some(translate), Some(rotate), Some(scale), Some(transform), Some(opacity)) =
                        (translate, rotate, scale, transform, opacity)
                    else {
                        return false;
                    };
                    out.matrix = to * translate * rotate * scale * transform * from;
                    out.opacity = opacity;
                    true
                };
                p.track.start = plan.start;
                p.track.duration = plan.duration;
                p.track.repeat = plan.repeat;
                if !bake_track(sample, &rect, &mut p.track.keys) {
                    continue;
                }
                p.track_version = self.layers.version();
            }
            // What the layer can cover, and how fine to rasterise it.
            let (mut x0, mut y0, mut x1, mut y1, mut scale) = (1e30f32, 1e30f32, -1e30f32, -1e30f32, 1.0f32);
            for k in &p.track.keys {
                let b = &p.rect;
                let corners = [[b.x, b.y], [b.x + b.w, b.y], [b.x, b.y + b.h], [b.x + b.w, b.y + b.h]];
                for c in corners {
                    let x = k.matrix.apply_x(c[0], c[1]);
                    let y = k.matrix.apply_y(c[0], c[1]);
                    x0 = min_f32(x0, x);
                    y0 = min_f32(y0, y);
                    x1 = max_f32(x1, x);
                    y1 = max_f32(y1, y);
                }
                scale = max_f32(scale, (k.matrix.a * k.matrix.d - k.matrix.b * k.matrix.c).abs().sqrt());
            }
            p.raster_scale = min_f32(scale, 3.0);
            p.reach = Rect::new(x0, y0, x1 - x0, y1 - y0);
            // A clip the whole track stays inside, away from its corners, cuts nothing: the layer goes without.
            if p.has_clip {
                let corner = corner_of(&p.clip_radii);
                if p.reach.x >= p.clip.x + corner
                    && p.reach.y >= p.clip.y + corner
                    && p.reach.x + p.reach.w <= p.clip.x + p.clip.w - corner
                    && p.reach.y + p.reach.h <= p.clip.y + p.clip.h - corner
                {
                    p.has_clip = false;
                }
            }
            if p.has_clip {
                let cx0 = max_f32(p.reach.x, p.clip.x);
                let cy0 = max_f32(p.reach.y, p.clip.y);
                let cx1 = min_f32(p.reach.x + p.reach.w, p.clip.x + p.clip.w);
                let cy1 = min_f32(p.reach.y + p.reach.h, p.clip.y + p.clip.h);
                p.reach = Rect::new(cx0, cy0, max_f32(0.0, cx1 - cx0), max_f32(0.0, cy1 - cy0));
            }
            next.entry(id).or_insert(p);
        }

        // Shimmers (the Swift renderer's WidgetShimmers): a background-position animation of a lone gradient tile
        // is the tile sliding under the element's clip, baked like any other track.
        for &i in &shimmers {
            let id = planned[i].id;
            let plan = planned[i].plan;
            let Some(n) = doc.node(id).map(|r| doc.get(r)) else { continue };
            if !n.children.is_empty() || next.contains_key(&id) || holds_animated.contains(&id) {
                continue;
            }
            let mut blocked = false;
            let mut a = n.parent;
            while !blocked {
                let Some(r) = a else { break };
                let up = doc.get(r);
                blocked = up.id != 0 && blocks(&mut self.engine, &self.animator, &animated, &mut local_background, up.id);
                a = up.parent;
            }
            let el = self.engine.has_element(id);
            let mut p = Promotion::default();
            if blocked || !el {
                continue;
            }
            let Some(tile) = self.engine.background_tile(id) else { continue };
            p.rect = tile.tile;
            let area = tile.area;
            p.clip = tile.clip;
            p.clip_radii = tile.radii;
            p.tile = tile.content;
            // An ancestor's clip must not cut into the tile's: a layer carries one.
            let Some(outer) = self.engine.clip_of(id) else { continue };
            if outer.has {
                let corner = corner_of(&outer.radii);
                let inner =
                    Rect::new(outer.rect.x + corner, outer.rect.y + corner, outer.rect.w - 2.0 * corner, outer.rect.h - 2.0 * corner);
                if !contains(&inner, &p.clip) {
                    continue;
                }
            }
            p.shimmer = true;
            p.has_clip = true;
            p.signature = planned[i].signature;
            p.tile_version = paint_digest(&p.tile);
            let kept: Kept = self.layers.promoted.get(&id).and_then(|old| {
                let same_span = old.shimmer
                    && if plan.repeat {
                        old.track.repeat && (old.track.duration - plan.duration).abs() < 1e-6
                    } else {
                        !old.track.repeat && (old.track.start + old.track.duration - (plan.start + plan.duration)).abs() < 1e-3
                    };
                (same_span && old.signature == p.signature && same_rect(&old.rect, &p.rect)).then(|| (old.track.clone(), old.track_version))
            });
            if let Some((track, version)) = kept {
                p.track = track;
                p.track_version = version;
            } else {
                let base = self.animator.base_value(id, b"background-position");
                let animator = &self.animator;
                let tile_rect = p.rect;
                let sample = |t: f64, out: &mut LayerKey| -> bool {
                    let values = animator.peek(id, plan_time(&plan, t));
                    let mut x: Option<f32> = None;
                    let mut found = false;
                    for (k, v) in values.iter().rev() {
                        if found {
                            break;
                        }
                        if is_shimmer_property(k) {
                            found = true;
                            x = shimmer_x(v, &area, &tile_rect);
                        }
                    }
                    if !found {
                        x = shimmer_x(if base.is_empty() { b"0% 0%" } else { &base }, &area, &tile_rect);
                    }
                    let Some(x) = x else { return false };
                    out.matrix = Affine::translate(x - tile_rect.x, 0.0);
                    out.opacity = 1.0;
                    true
                };
                p.track.start = plan.start;
                p.track.duration = plan.duration;
                p.track.repeat = plan.repeat;
                if !bake_track(sample, &tile_rect, &mut p.track.keys) {
                    continue;
                }
                p.track_version = self.layers.version();
            }
            p.reach = p.clip;
            next.entry(id).or_insert(p);
        }

        // Only a layer nothing paints over can go above the document. Dropping one can uncover another's
        // neighbour or put back what covered another, so this runs until nothing changes.
        self.engine.paint_order();

        // Scripted layers: still pictures apply_scripted moves, each alone in its branch of the tree, with room to
        // move around where it is now and the raster scale its transform asks for.
        {
            let mut others = self.engine.elements(next.keys().copied());
            let mut candidates: Vec<i64> = Vec::new();
            let all: Vec<i64> = self.layers.scripted_candidates.iter().copied().collect();
            for &id in &all {
                let n = doc.node(id).map(|r| doc.get(r));
                if next.contains_key(&id)
                    || animated.contains(&id)
                    || holds_animated.contains(&id)
                    || self.animator.declares_motion(id)
                    || n.is_none_or(|n| n.svg || n.tag == b"svg")
                {
                    continue;
                }
                let Some(n) = n else { continue };
                let mut alone = true;
                for &other in next.keys() {
                    alone = alone && !is_ancestor(doc, other, id) && !is_ancestor(doc, id, other);
                }
                for &other in &all {
                    alone = alone && (other == id || (!is_ancestor(doc, other, id) && !is_ancestor(doc, id, other)));
                }
                let mut a = n.parent;
                while alone {
                    let Some(r) = a else { break };
                    let up = doc.get(r);
                    alone = !(up.id != 0 && blocks(&mut self.engine, &self.animator, &animated, &mut local_background, up.id));
                    a = up.parent;
                }
                let el = self.engine.has_element(id);
                if !alone || !el {
                    continue;
                }
                self.engine.insert_element(&mut others, id);
                candidates.push(id);
            }
            for id in candidates {
                let mut p = Promotion::default();
                let Some(rect) = self.engine.layer_candidate(id, None) else { continue };
                p.rect = rect;
                if f64::from(p.rect.w) * f64::from(p.rect.h) >= MAX_SCRIPTED_AREA {
                    continue;
                }
                let Some(clip) = self.engine.clip_of(id) else { continue };
                apply_clip(&mut p, &clip);
                p.scripted = true;
                p.base = self.engine.declared(id, &LAYER_DECLARED);
                let Some(key) = self.scripted_key(doc, id, &p) else { continue };
                let m = key.matrix;
                let translating = m.a == 1.0 && m.b == 0.0 && m.c == 0.0 && m.d == 1.0;
                let rotated = m.b.abs() > 1e-4 || m.c.abs() > 1e-4;
                let scale = (m.a * m.d - m.b * m.c).abs().sqrt();
                p.raster_scale = if translating { 1.0 } else { min_f32(3.0, max_f32(if rotated { 2.0 } else { 1.0 }, scale * 1.25)) };
                p.signature = 0x5C819u64 ^ (id as u64);
                let kept: Kept = self.layers.promoted.get(&id).and_then(|old| {
                    (old.scripted
                        && old.track.keys.len() == 1
                        && old.track.keys[0].matrix == m
                        && old.track.keys[0].opacity == key.opacity
                        && same_rect(&old.rect, &p.rect))
                    .then(|| (old.track.clone(), old.track_version))
                });
                if let Some((track, version)) = kept {
                    p.track = track;
                    p.track_version = version;
                } else {
                    p.track.start = now;
                    p.track.keys = vec![key];
                    p.track_version = self.layers.version();
                }
                // The widest margin around it that nothing paints over.
                let at = bounds_under(&p.rect, &m);
                for margin in [max_f32(48.0, max_f32(p.rect.w, p.rect.h) / 2.0), 16.0, 2.0] {
                    p.reach = Rect::new(at.x - margin, at.y - margin, at.w + 2.0 * margin, at.h + 2.0 * margin);
                    if p.has_clip {
                        p.reach = intersection(&p.reach, &p.clip);
                    }
                    if !self.engine.covered(id, p.reach, &others) {
                        break;
                    }
                }
                if p.has_clip {
                    let corner = corner_of(&p.clip_radii);
                    let inner = Rect::new(p.clip.x + corner, p.clip.y + corner, p.clip.w - 2.0 * corner, p.clip.h - 2.0 * corner);
                    if contains(&inner, &p.reach) {
                        p.has_clip = false;
                    }
                }
                next.entry(id).or_insert(p);
            }
        }

        // SVG layers (the Swift renderer's WidgetSVGLayers): an SVG a script changes batch after batch repaints
        // alone, and the document around it does not.
        let svgs: Vec<i64> = self.layers.svg_layers.iter().copied().collect();
        for id in svgs {
            let Some(n) = doc.node(id).map(|r| doc.get(r)) else { continue };
            if next.contains_key(&id) || animated.contains(&id) || holds_animated.contains(&id) {
                continue;
            }
            let mut alone = true;
            for &other in next.keys() {
                alone = alone && !is_ancestor(doc, other, id) && !is_ancestor(doc, id, other);
            }
            let mut a = n.parent;
            while alone {
                let Some(r) = a else { break };
                let up = doc.get(r);
                alone = !(up.id != 0 && blocks(&mut self.engine, &self.animator, &animated, &mut local_background, up.id));
                a = up.parent;
            }
            let el = self.engine.has_element(id);
            let mut p = Promotion::default();
            if !alone || !el {
                continue;
            }
            let Some(rect) = self.engine.layer_candidate(id, None) else { continue };
            p.rect = rect;
            let Some(clip) = self.engine.clip_of(id) else { continue };
            apply_clip(&mut p, &clip);
            p.svg = true;
            p.signature = 0x5F6u64 ^ (id as u64);
            if !self.still_track(doc, id, &mut p, now) {
                continue;
            }
            next.entry(id).or_insert(p);
        }

        // Canvas layers: a canvas that keeps drawing repaints only itself.
        let canvases: Vec<i64> = self.layers.canvas_layers.iter().copied().collect();
        for id in canvases {
            let Some(n) = doc.node(id).map(|r| doc.get(r)) else { continue };
            if n.tag != b"canvas" || next.contains_key(&id) || animated.contains(&id) {
                continue;
            }
            let mut alone = true;
            for &other in next.keys() {
                alone = alone && !is_ancestor(doc, other, id) && !is_ancestor(doc, id, other);
            }
            let mut a = n.parent;
            while alone {
                let Some(r) = a else { break };
                let up = doc.get(r);
                alone = !(up.id != 0 && blocks(&mut self.engine, &self.animator, &animated, &mut local_background, up.id));
                a = up.parent;
            }
            let el = self.engine.has_element(id);
            let mut p = Promotion::default();
            if !alone || !el {
                continue;
            }
            let Some(rect) = self.engine.layer_candidate(id, None) else { continue };
            p.rect = rect;
            let Some(clip) = self.engine.clip_of(id) else { continue };
            apply_clip(&mut p, &clip);
            p.canvas = true;
            p.signature = 0xCA9u64 ^ (id as u64);
            if !self.still_track(doc, id, &mut p, now) {
                continue;
            }
            next.entry(id).or_insert(p);
        }

        // A few static elements over a wanted layer (labels on a moving card) become layers of their own, still
        // ones, so the moving one can go under them.
        {
            let mut ignore_all = self.engine.elements(next.keys().copied());
            let mut budget = MAX_LAYERS.saturating_sub(next.len());
            let queue: Vec<i64> = next.keys().copied().collect();
            for id in queue {
                if budget == 0 {
                    break;
                }
                let reach = next[&id].reach;
                if reach.empty() || !self.engine.covered(id, reach, &ignore_all) {
                    continue;
                }
                let Some(found) = self.engine.obscurers(id, reach, &ignore_all, MAX_OBSCURERS) else { continue };
                if found.is_empty() || found.len() > budget {
                    continue;
                }
                let mut stills: Vec<(i64, Promotion)> = Vec::new();
                for other in found {
                    let n = doc.node(other).map(|r| doc.get(r));
                    let el = self.engine.has_element(other);
                    let mut s = Promotion::default();
                    let mut fits = !(next.contains_key(&other)
                        || animated.contains(&other)
                        || n.is_none_or(|n| n.svg && n.tag != b"svg")
                        || is_ancestor(doc, other, id)
                        || is_ancestor(doc, id, other)
                        || !el);
                    if fits {
                        match self.engine.layer_candidate(other, None) {
                            Some(rect) => s.rect = rect,
                            None => fits = false,
                        }
                    }
                    if fits {
                        match self.engine.clip_of(other) {
                            Some(clip) => apply_clip(&mut s, &clip),
                            None => fits = false,
                        }
                    }
                    if !fits {
                        stills.clear();
                        break;
                    }
                    s.signature = 0x57A71Cu64 ^ (other as u64);
                    if !self.still_track(doc, other, &mut s, now) {
                        stills.clear();
                        break;
                    }
                    stills.push((other, s));
                }
                for (other, s) in stills {
                    self.engine.insert_element(&mut ignore_all, other);
                    next.entry(other).or_insert(s);
                    budget -= 1;
                }
            }
        }
        for (&id, p) in next.iter_mut() {
            p.order = self.engine.paint_index(id);
        }

        // Nested layers: a layer inside a mover moves with the mover's layer. Its keys and its own clip are in the
        // parent's content space; a clip it shares with the parent is the parent's. What nothing may cover is where
        // the parent's keys take its reach.
        for (&id, p) in next.iter_mut() {
            p.parent = 0;
            let mut a = doc.node(id).and_then(|r| doc.get(r).parent);
            while p.parent == 0 {
                let Some(r) = a else { break };
                let up = doc.get(r);
                if up.id != 0 && movers.contains(&up.id) {
                    p.parent = up.id;
                }
                a = up.parent;
            }
        }
        {
            let mut by_depth: Vec<(i32, i64)> = Vec::new();
            for (&id, p) in &next {
                let mut depth = 0;
                let mut up = p.parent;
                while up != 0 && depth < 64 {
                    let Some(q) = next.get(&up) else { break };
                    depth += 1;
                    up = q.parent;
                }
                by_depth.push((depth, id));
            }
            by_depth.sort();
            for (_, id) in by_depth {
                let p = &next[&id];
                let (parent_id, has_clip, found_clip_rect, own) = (p.parent, p.has_clip, p.found_clip_rect, p.reach);
                if parent_id == 0 {
                    continue;
                }
                let Some(parent) = next.get(&parent_id) else { continue };
                let drop_clip = has_clip && parent.found_clip && same_rect(&found_clip_rect, &parent.found_clip_rect);
                let mut reach = Rect::default();
                let mut first = true;
                for k in &parent.track.keys {
                    let at = bounds_under(&own, &k.matrix);
                    if first {
                        reach = at;
                    } else {
                        reach = Rect::new(
                            min_f32(reach.x, at.x),
                            min_f32(reach.y, at.y),
                            max_f32(reach.x + reach.w, at.x + at.w) - min_f32(reach.x, at.x),
                            max_f32(reach.y + reach.h, at.y + at.h) - min_f32(reach.y, at.y),
                        );
                    }
                    first = false;
                }
                if parent.has_clip {
                    reach = intersection(&reach, &parent.clip);
                }
                let Some(p) = next.get_mut(&id) else { continue };
                if drop_clip {
                    p.has_clip = false;
                }
                p.reach = reach;
            }
        }
        drop_orphans(doc, &mut next);
        let rounds = next.len() + 1;
        let mut round = 0;
        while round < rounds && !next.is_empty() {
            round += 1;
            let ignore = self.engine.elements(next.keys().copied());
            let mut uncovered: Vec<(i64, f64)> = Vec::new();
            let mut total = 0.0;
            for (&id, p) in &next {
                if p.reach.empty() || self.engine.covered(id, p.reach, &ignore) {
                    continue;
                }
                let s = self.dpr * f64::from(p.raster_scale);
                let cost = f64::from(p.rect.w + 2.0 * LAYER_MARGIN) * f64::from(p.rect.h + 2.0 * LAYER_MARGIN) * s * s;
                uncovered.push((id, cost));
                total += cost;
            }
            // Over the budget, the topmost layers go first and the first that does not fit ends it: a skipped layer
            // would leave the ones under it composited out of order with it.
            let all = uncovered.len() <= MAX_LAYERS && total <= MAX_LAYER_PIXELS;
            if !all {
                uncovered.sort_by(|a, b| (next[&b.0].order, b.0).cmp(&(next[&a.0].order, a.0)));
            }
            let before = next.len();
            let mut admitted: BTreeMap<i64, Promotion> = BTreeMap::new();
            let mut pixels = 0.0;
            for (id, cost) in uncovered {
                if !all && (admitted.len() >= MAX_LAYERS || pixels + cost > MAX_LAYER_PIXELS) {
                    break;
                }
                pixels += cost;
                if let Some(p) = next.remove(&id) {
                    admitted.entry(id).or_insert(p);
                }
            }
            drop_orphans(doc, &mut admitted);
            let stable = admitted.len() == before;
            next = admitted;
            if stable {
                break;
            }
        }

        let mut changed = next.len() != self.layers.promoted.len();
        for (id, p) in &next {
            changed = changed
                || match self.layers.promoted.get(id) {
                    None => true,
                    Some(old) => {
                        old.track_version != p.track_version
                            || old.order != p.order
                            || old.parent != p.parent
                            || !same_rect(&old.rect, &p.rect)
                    }
                };
        }
        self.layers.promoted = next;
        self.layers.layer_ids = self.layers.promoted.keys().copied().collect();
        if changed {
            self.layers.composition_only = false;
            self.dirty = true;
        }
    }

    pub(crate) fn sample_document(&mut self, now: f64) -> StyleOverrides {
        let promoted = &self.layers.promoted;
        self.animator.sample_filtered(now, |id| {
            let shimmer = promoted.get(&id).map(|p| p.shimmer);
            move |property| match shimmer {
                Some(true) => !is_shimmer_property(property),
                Some(false) => !is_layer_property(property),
                None => true,
            }
        })
    }

    // ── scripted layers ──

    /// `scripted_only`: the batch only moves scripted layers through their layer properties; the elements it moves.
    pub(crate) fn scripted_only(&self, doc: &Document, ops: &[Json]) -> Option<Vec<i64>> {
        if still(doc) || ops.is_empty() || self.layers.promoted.is_empty() {
            return None;
        }
        let mut touched = Vec::new();
        for op in ops {
            if op.get("op").as_str() != b"style" || op.get("important").as_bool(false) {
                return None;
            }
            // Taking a property away goes back to what the document declares, which only a build knows.
            let value = op.get("value").as_str();
            if value.is_empty() || value.len() > 256 || !is_layer_property(&css::lower(op.get("name").as_str())) {
                return None;
            }
            let id = node_id(op.get("id"));
            if !self.layers.promoted.get(&id).is_some_and(|p| p.scripted) {
                return None;
            }
            if !touched.contains(&id) {
                touched.push(id);
            }
        }
        Some(touched)
    }

    /// `note_scripted`: the streaks of elements moved by style ops.
    pub(crate) fn note_scripted(&mut self, ops: &[Json], now: f64) {
        self.layers.ops_batch = self.layers.ops_batch.wrapping_add(1);
        let batch = self.layers.ops_batch;
        let mut moving: Vec<i64> = Vec::new();
        for op in ops {
            if op.get("op").as_str() == b"style" && is_layer_property(&css::lower(op.get("name").as_str())) {
                moving.push(node_id(op.get("id")));
            }
        }
        if moving.is_empty() {
            return;
        }
        let mut changed = false;
        if moving.len() > MAX_LAYERS {
            // A crowd moves together: layers would not fit, and the document paints them in one pass anyway.
            for id in &moving {
                changed = self.layers.scripted_candidates.remove(id) || changed;
            }
        } else {
            for &id in &moving {
                let fresh = !self.layers.scripted_hits.contains_key(&id);
                let hit = self.layers.scripted_hits.entry(id).or_default();
                if !fresh && hit.batch == batch {
                    continue;
                }
                let continues =
                    !fresh && (batch.wrapping_sub(hit.batch) <= SCRIPTED_STREAK_BATCHES || now - hit.at <= SCRIPTED_STREAK_GAP_S);
                *hit = ScriptedHit { count: if continues { hit.count.wrapping_add(1) } else { 1 }, batch, at: now };
                if hit.count >= 3 && self.layers.scripted_candidates.len() < MAX_LAYERS && self.layers.scripted_candidates.insert(id) {
                    changed = true;
                }
            }
        }
        if self.layers.scripted_hits.len() > 4096 {
            self.layers.scripted_hits.retain(|_, h| !(batch.wrapping_sub(h.batch) >= 8 && now - h.at >= 1.0));
        }
        if changed {
            self.layers.composition_key = 0;
        }
    }

    /// `note_svg`: counts the batches running that change each SVG; three make it an SVG layer.
    pub(crate) fn note_svg(&mut self, doc: &Document, ops: &[Json]) {
        let mut touched: BTreeSet<i64> = BTreeSet::new();
        for op in ops {
            let kind = op.get("op").as_str();
            // An insertion changes its parent; a removed node is gone, and so is the way to its SVG.
            let mut n = doc.node(node_id(if kind == b"ins" { op.get("parent") } else { op.get("id") }));
            let mut root = 0;
            while let Some(r) = n {
                let node = doc.get(r);
                if node.kind == NodeKind::Element && node.tag == b"svg" {
                    root = node.id;
                }
                n = node.parent;
            }
            if root != 0 {
                touched.insert(root);
            }
        }
        let mut streaks: HashMap<i64, i32> = HashMap::new();
        let mut added = false;
        for &id in &touched {
            let count = self.layers.svg_streaks.get(&id).map_or(1, |c| c.wrapping_add(1));
            streaks.insert(id, count);
            if count >= 3 && self.layers.svg_layers.len() < MAX_LAYERS && self.layers.svg_layers.insert(id) {
                added = true;
            }
        }
        self.layers.svg_streaks = streaks;
        if added {
            self.layers.composition_key = 0;
        }
    }

    /// `scripted_key`: the layer's matrix and opacity as the element's style has them now.
    fn scripted_key(&self, doc: &Document, id: i64, p: &Promotion) -> Option<LayerKey> {
        let n = doc.get(doc.node(id)?);
        let value = |property: &[u8]| -> Vec<u8> {
            for (name, v) in &n.styles {
                if name == property {
                    return v.clone();
                }
            }
            if let Some(v) = p.base.get(property) {
                return v.clone();
            }
            self.animator.base_value(id, property)
        };
        let r = p.rect;
        let origin = css::parse_origin(&value(b"transform-origin"), r.w, r.h);
        let translate = css::parse_translate(&value(b"translate"), r.w, r.h);
        let rotate = css::parse_rotate(&value(b"rotate"));
        let scale = css::parse_scale(&value(b"scale"));
        let transform = css::parse_transform(&value(b"transform"), r.w, r.h);
        let opacity = css::parse_opacity(&value(b"opacity"));
        let (Some(origin), Some(translate), Some(rotate), Some(scale), Some(transform), Some(opacity)) =
            (origin, translate, rotate, scale, transform, opacity)
        else {
            return None;
        };
        let to = Affine::translate(r.x + origin.0, r.y + origin.1);
        let from = Affine::translate(-(r.x + origin.0), -(r.y + origin.1));
        Some(LayerKey { t: 0.0, matrix: to * translate * rotate * scale * transform * from, opacity })
    }

    /// `still_track`: a still layer's track and reach: one key holding the element's own transform and opacity,
    /// which its content leaves out. The running track stays when the key is the same.
    fn still_track(&mut self, doc: &Document, id: i64, p: &mut Promotion, now: f64) -> bool {
        p.base = self.engine.declared(id, &LAYER_DECLARED);
        let Some(key) = self.scripted_key(doc, id, p) else { return false };
        let kept: Kept = self.layers.promoted.get(&id).and_then(|old| {
            (old.signature == p.signature
                && same_rect(&old.rect, &p.rect)
                && old.track.keys.len() == 1
                && old.track.keys[0].matrix == key.matrix
                && old.track.keys[0].opacity == key.opacity)
                .then(|| (old.track.clone(), old.track_version))
        });
        if let Some((track, version)) = kept {
            p.track = track;
            p.track_version = version;
        } else {
            p.track = LayerTrack { start: now, keys: vec![key], ..LayerTrack::default() };
            p.track_version = self.layers.version();
        }
        p.reach = bounds_under(&p.rect, &key.matrix);
        if p.has_clip {
            p.reach = intersection(&p.reach, &p.clip);
        }
        true
    }

    /// `apply_scripted`: the layers moved without a build; false when one moved past what its layer allows. The clock is
    /// read once every layer may move, as the C++ reads it.
    pub(crate) fn apply_scripted(&mut self, doc: &Document, ids: &[i64]) -> bool {
        let mut moves: Vec<(i64, LayerKey)> = Vec::new();
        for &id in ids {
            let Some(p) = self.layers.promoted.get(&id) else { return false };
            let Some(key) = self.scripted_key(doc, id, p) else { return false };
            let m = key.matrix;
            if (m.a * m.d - m.b * m.c).abs().sqrt() > p.raster_scale + 0.001 {
                return false;
            }
            // Where the layer shows must stay where the plan found nothing painting over it.
            let mut at = bounds_under(&p.rect, &m);
            if p.has_clip {
                at = intersection(&at, &p.clip);
            }
            if !at.empty() && !contains(&p.reach, &at) {
                return false;
            }
            moves.push((id, key));
        }
        let clean = !self.dirty || self.layers.composition_only;
        let mut synced = true;
        let mut changed = false;
        let now = self.now_s();
        for (id, key) in moves {
            let version = self.layers.next_version;
            let Some(p) = self.layers.promoted.get_mut(&id) else { continue };
            if p.track.keys.len() == 1 && p.track.keys[0].matrix == key.matrix && p.track.keys[0].opacity == key.opacity {
                continue;
            }
            p.track.start = now;
            p.track.duration = 0.0;
            p.track.repeat = false;
            p.track.keys = vec![key];
            p.track_version = version;
            let (track, track_version) = (p.track.clone(), p.track_version);
            self.layers.next_version = version.wrapping_add(1);
            changed = true;
            match self.layers.composition.layers.iter_mut().find(|l| l.id == id) {
                None => {
                    synced = false;
                    continue;
                }
                Some(layer) => {
                    layer.track = track;
                    layer.track_version = track_version;
                }
            }
        }
        if changed {
            self.layers.composition.version = self.layers.version();
            self.layers.composition_only = clean && synced;
            self.dirty = true;
        }
        true
    }

    // ── canvases ──

    /// What `run_canvas` does after the backend ran a canvas's ops.
    pub(crate) fn canvas_drew(&mut self, canvas: i32) {
        self.layers.canvases_drawn.insert(canvas);
        let draws = {
            let d = self.layers.canvas_draws.entry(canvas).or_insert(0);
            *d = d.wrapping_add(1);
            *d
        };
        // Drawn a third time, a canvas looks animated: the Swift renderer presents such a canvas in a layer.
        if draws >= 3 && self.layers.canvas_layers.len() < MAX_LAYERS && self.layers.canvas_layers.insert(i64::from(canvas)) {
            self.layers.composition_key = 0;
        }
        let clean = !self.dirty || self.layers.composition_only;
        self.dirty = true;
        let id = i64::from(canvas);
        let is_layer = self.layers.promoted.get(&id).is_some_and(|p| p.canvas);
        let at = self.layers.composition.layers.iter().position(|l| l.id == id);
        let (true, Some(at)) = (is_layer, at) else {
            self.layers.composition_only = false;
            return;
        };
        // Only the layer shows what the canvas drew.
        let version = self.content_version(&self.layers.composition.layers[at].content);
        self.layers.composition.layers[at].content_version = version;
        self.layers.composition.version = self.layers.version();
        self.layers.composition_only = clean;
    }

    /// The content version of a layer `content`: its commands, and what the canvases in it drew.
    fn content_version(&self, content: &[Command]) -> u64 {
        let mut version = paint_digest(content);
        for c in content {
            if c.canvas != 0 {
                let draws = self.layers.canvas_draws.get(&c.canvas).copied().unwrap_or(0);
                version = (version ^ (i64::from(c.canvas) as u64) ^ (draws << 20)).wrapping_mul(1099511628211);
            }
        }
        version
    }

    // ── the paint ──

    fn px_size(&self) -> (u32, u32) {
        ((self.width * self.dpr).ceil() as i64 as u32, (f64::from(self.vp_height) * self.dpr).ceil() as i64 as u32)
    }

    /// `Container::paint_key`: equal keys draw equal lists.
    fn paint_key(&mut self) -> (u64, u32, u64) {
        let (doc, generation) = self.engine.paint_key();
        (doc, generation, self.container.layouts())
    }

    /// `paint()` after its build: the list handed out again, or the document and the layers painted and the damage
    /// against the last paint.
    pub(crate) fn paint_frame(&mut self, doc: &Document, events: &mut Events) -> DisplayList {
        if self.reuse_paint() {
            self.dirty = false;
            self.layers.composition_only = false;
            self.layers.damage.clear();
            self.layers.damage_valid = true;
            self.layers.canvases_drawn.clear();
            self.stats.reused += 1;
            return self.layers.painted_list.clone();
        }
        self.layers.composition_only = false;
        // The paint finds the runs again, in the document or in a layer.
        events.paint_begins(self);
        let mut list = self.paint_document(doc, events);
        drop_inert_clips(&mut list);
        events.paint_selection(self, doc.theme_variables(), &mut list, 0, true);
        // What changed against the last paint, for a platform to repaint only that.
        let summary = summarize(&list);
        let size = self.px_size();
        let l = &mut self.layers;
        l.damage_valid = l.painted_valid && l.painted_size == size && {
            let drawn = &l.canvases_drawn;
            let mut changed = |canvas: i32| drawn.contains(&canvas);
            damage_between(&l.painted, &summary, Some(&mut changed), &mut l.damage)
        };
        l.painted = summary;
        l.painted_valid = true;
        l.painted_size = size;
        l.canvases_drawn.clear();
        l.painted_list_valid = l.promoted.values().any(|p| p.scripted || p.canvas);
        if self.layers.painted_list_valid {
            self.layers.painted_list = list.clone();
            self.layers.painted_key = self.paint_key();
        } else {
            self.layers.painted_list = DisplayList::new();
        }
        list
    }

    /// `reuse_paint`: nothing but layer tracks changed since the last paint: its list stands.
    fn reuse_paint(&mut self) -> bool {
        let l = &self.layers;
        if !l.composition_only || !l.painted_list_valid || !l.painted_valid || !self.touched.is_empty() || self.touched_all {
            return false;
        }
        for &canvas in &l.canvases_drawn {
            if !l.promoted.get(&i64::from(canvas)).is_some_and(|p| p.canvas) {
                return false;
            }
        }
        if self.paint_key() != self.layers.painted_key || self.layers.painted_size != self.px_size() {
            return false;
        }
        let l = &self.layers;
        if l.promoted.len() != l.composition.layers.len() {
            return false;
        }
        for i in 0..l.composition.layers.len() {
            let Some(p) = l.promoted.get(&l.composition.layers[i].id) else { return false };
            if p.track_version != l.composition.layers[i].track_version {
                return false;
            }
            if i > 0 && l.promoted.get(&l.composition.layers[i - 1].id).is_some_and(|q| p.order <= q.order) {
                return false;
            }
        }
        true
    }

    /// `paint_document`: the document without its layers, and each layer's content kept or drawn again.
    fn paint_document(&mut self, doc: &Document, events: &mut Events) -> DisplayList {
        self.dirty = false;
        if !self.layers.promoted.is_empty() || !self.layers.composition.layers.is_empty() {
            let mut order: Vec<(usize, i64)> = Vec::new();
            let mut skip: Vec<i64> = Vec::new();
            let mut backgrounds: Vec<i64> = Vec::new();
            let promoted: Vec<(i64, usize, bool)> = self.layers.promoted.iter().map(|(&id, p)| (id, p.order, p.shimmer)).collect();
            for (id, rank, shimmer) in promoted {
                if self.engine.has_element(id) {
                    order.push((rank, id));
                    if shimmer {
                        backgrounds.push(id);
                    } else {
                        skip.push(id);
                    }
                }
            }
            let list = self.engine.draw_skipping(&skip, &backgrounds);
            order.sort();
            let mut old = std::mem::take(&mut self.layers.composition);
            let mut next = Composition::default();
            let mut changed = order.len() != old.layers.len();
            // A layer whose element and descendants nothing touched since the last paint keeps its content.
            let all = self.touched_all || self.container.layouts() != self.layers.painted_layouts;
            let mut before: HashMap<i64, usize> = HashMap::new();
            if !all {
                for (i, layer) in old.layers.iter().enumerate() {
                    before.entry(layer.id).or_insert(i);
                }
                for &id in &self.touched {
                    if before.is_empty() {
                        break;
                    }
                    before.remove(&id);
                    let mut parent = doc.node(id).and_then(|r| doc.get(r).parent);
                    while let Some(r) = parent {
                        let node = doc.get(r);
                        if node.id != 0 {
                            before.remove(&node.id);
                        }
                        parent = node.parent;
                    }
                }
            }
            for (_, id) in order {
                let p = self.layers.promoted[&id].clone();
                let mut layer = Layer { id, ..Layer::default() };
                let drawn = &self.layers.canvases_drawn;
                let redrawn = |list: &[Command]| list.iter().any(|c| c.canvas != 0 && drawn.contains(&c.canvas));
                let keep = before.get(&id).copied().filter(|&i| !redrawn(&old.layers[i].content));
                if p.shimmer {
                    layer.content = p.tile;
                    layer.bounds = p.rect;
                    layer.content_version = p.tile_version;
                } else if let Some(i) = keep {
                    // A selection change touches every layer: this one's tint is still the selection's.
                    let o = &mut old.layers[i];
                    layer.content = std::mem::take(&mut o.content);
                    layer.bounds = o.bounds;
                    layer.content_version = o.content_version;
                    events.paint_selection(self, doc.theme_variables(), &mut layer.content, id, false);
                } else {
                    let (content, bounds) = self.engine.draw_layer(id, &skip);
                    layer.content = content;
                    layer.bounds = bounds;
                    events.paint_selection(self, doc.theme_variables(), &mut layer.content, id, true);
                    layer.content_version = self.content_version(&layer.content);
                }
                layer.raster_scale = p.raster_scale;
                layer.has_clip = p.has_clip;
                layer.clip = p.clip;
                layer.clip_radii = p.clip_radii;
                layer.track_version = p.track_version;
                layer.track = p.track;
                layer.parent = p.parent;
                let i = next.layers.len();
                if !changed {
                    let o = &old.layers[i];
                    changed = o.id != id
                        || o.content_version != layer.content_version
                        || o.track_version != layer.track_version
                        || o.parent != layer.parent
                        || !same_rect(&o.bounds, &layer.bounds)
                        || o.has_clip != layer.has_clip
                        || !same_rect(&o.clip, &layer.clip)
                        || radii_bits(&o.clip_radii) != radii_bits(&layer.clip_radii);
                }
                next.layers.push(layer);
            }
            next.version = if changed { self.layers.version() } else { old.version };
            self.layers.composition = next;
            self.touched.clear();
            self.touched_all = false;
            self.layers.painted_layouts = self.container.layouts();
            return list;
        }
        self.touched.clear();
        self.touched_all = true;
        self.engine.draw()
    }

    // ── what the input reads ──

    /// `layer_shown`: where promoted layer `id`'s track shows it now, over its parents', and with what opacity.
    pub(crate) fn layer_shown(&self, id: i64, now: f64) -> Option<(Affine, f32)> {
        let mut shown = Affine::IDENTITY;
        let mut opacity = 1.0f32;
        // Its key after each parent's, the opacity theirs too.
        let first = self.layers.promoted.get(&id);
        let mut q = first;
        while let Some(p) = q {
            let (keys, _) = resume_track(&p.track, now - p.track.start);
            if keys.is_empty() || self.layers.promoted.contains_key(&p.parent) != (p.parent != 0) {
                return None;
            }
            shown = keys[0].matrix * shown;
            opacity *= keys[0].opacity;
            q = if p.parent != 0 { self.layers.promoted.get(&p.parent) } else { None };
        }
        first.map(|_| (shown, opacity))
    }

    /// `widget_id_at`: a layer shows where its track has it now, not where layout put it: the topmost layer under
    /// the point takes it, mapped back into the document.
    pub(crate) fn widget_id_at(&mut self, x: f64, y: f64, mut placement: Option<&mut Rect>) -> i64 {
        if !self.layers.promoted.is_empty() {
            let mut order: Vec<(usize, i64)> = self.layers.promoted.iter().map(|(&id, p)| (p.order, id)).collect();
            order.sort_by(|a, b| b.cmp(a));
            let now = self.now_s();
            for (_, id) in order {
                let p = &self.layers.promoted[&id];
                // A shimmer's element stays in the document, which takes the point.
                if p.shimmer {
                    continue;
                }
                let (clip, has_clip, rect) = (p.clip, p.has_clip, p.rect);
                if has_clip
                    && !(x >= f64::from(clip.x)
                        && y >= f64::from(clip.y)
                        && x < f64::from(clip.x + clip.w)
                        && y < f64::from(clip.y + clip.h))
                {
                    continue;
                }
                let Some((shown, opacity)) = self.layer_shown(id, now) else { continue };
                if opacity <= 0.0 {
                    continue;
                }
                let Some(inverse) = shown.invert() else { continue };
                let mx = inverse.apply_x(x as f32, y as f32);
                let my = inverse.apply_y(x as f32, y as f32);
                if !(mx >= rect.x && my >= rect.y && mx < rect.x + rect.w && my < rect.y + rect.h) {
                    continue;
                }
                let hit = self.engine.hit_within(id, mx, my, placement.as_deref_mut());
                if hit != 0 {
                    return hit;
                }
            }
        }
        self.engine.hit(x, y, placement)
    }

    // ── heavy boxes ──

    fn layer_opacity(&self, id: i64) -> Option<f32> {
        let p = self.layers.promoted.get(&id)?;
        if !p.scripted || p.track.keys.len() != 1 {
            return None;
        }
        Some(p.track.keys[0].opacity)
    }

    /// `scan_boxes`: every box the document builds, from its computed values.
    fn scan_boxes(&mut self) {
        self.layers.paint.begin_scan();
        // The widget's elements carry their id; the renderer's own (the stopped badge) build no box of the widget's.
        for (key, pseudo, style) in self.engine.boxes() {
            let opacity = if pseudo { None } else { self.layer_opacity(key / 4) };
            self.layers.paint.set_box(&key, Some(box_paint_of(&style, pseudo, opacity)));
        }
        if self.layers.paint.end_scan() {
            self.layers.motion_stale = true;
        }
    }

    /// `recount_boxes`: the boxes of scripted layers, whose opacity the document has not taken yet.
    pub(crate) fn recount_boxes(&mut self, doc: &mut Document, ids: &[i64]) {
        if !self.layers.paint_on || doc.paint_stopped {
            return;
        }
        for &id in ids {
            // Only a box the last count found: one under display: none stays out.
            let key = id.wrapping_mul(4);
            if self.engine.has_element(id) && self.layers.paint.has_box(&key) {
                let style = self.engine.box_style(id);
                let paint = box_paint_of(&style, false, self.layer_opacity(id));
                self.layers.paint.set_box(&key, Some(paint));
            }
        }
        self.count_paint(doc);
    }

    /// `sync_motion`: rule 5, the running animations into the ledger; true when one that was counted ended.
    fn sync_motion(&mut self, now: f64) -> bool {
        let hash = self.animator.heavy_motion(now, None);
        if !self.layers.motion_stale && hash == self.layers.motion_hash {
            return false;
        }
        self.layers.motion_stale = false;
        self.layers.motion_hash = hash;
        let mut motions = std::mem::take(&mut self.layers.motions);
        self.animator.heavy_motion(now, Some(&mut motions));
        let mut running: Vec<CountedMotion> = Vec::with_capacity(motions.len());
        for m in &motions {
            let mut id = vec![m.kind, b':'];
            if m.kind == b'w' {
                id.extend_from_slice(m.aid.to_string().as_bytes());
            } else {
                id.extend_from_slice(&m.name);
            }
            running.push(CountedMotion { key: m.id.wrapping_mul(4), id, paint: m.paint });
        }
        self.layers.motions = motions;
        let mut ended = false;
        for c in &self.layers.motion_counted {
            if running.iter().any(|r| r.key == c.key && r.id == c.id) {
                continue;
            }
            self.layers.paint.end_animation(&c.key, &c.id);
            ended = true;
        }
        for r in &running {
            if !self.layers.paint.animating(&r.key, &r.id) {
                self.layers.paint.start_animation(&r.key, &r.id, r.paint);
            }
        }
        self.layers.motion_counted = running;
        ended
    }

    /// `count_paint`: heavy boxes are counted before a frame is drawn, after a build, a restyle by the pointer, a
    /// batch that moved scripted layers, and an animation of one starting or ending. Past a limit, `limit` goes to
    /// the host.
    pub(crate) fn count_paint(&mut self, doc: &mut Document) {
        if !self.layers.paint_on || still(doc) {
            return;
        }
        let now = self.now_s();
        // An animation that ended leaves its box's own values, last read while it applied: every box is read again.
        if self.sync_motion(now) {
            self.layers.paint_scan = true;
        }
        if self.layers.paint_scan {
            self.layers.paint_scan = false;
            self.scan_boxes();
            // A box that went took its animations; those that still run come back with it.
            self.sync_motion(now);
        }
        let counts = (self.layers.paint.heavy(), self.layers.paint.backdrop());
        if self.layers.paint_reports && self.layers.paint_reported != counts {
            self.layers.paint_reported = counts;
            let mut m = Json::object();
            m.set("type", "paint").set("heavy", counts.0).set("backdrop", counts.1);
            self.pending_events.push(m);
        }
        if let Some(verdict) = self.layers.paint.check() {
            self.stop_paint(doc, verdict);
        }
    }

    /// `stop_paint`: reported before the frame is drawn; the host stops the widget and freezes the renderer with its
    /// own label. The input's coalesced moves go with the renderer's next `follow`.
    fn stop_paint(&mut self, doc: &mut Document, verdict: Verdict) {
        doc.paint_stopped = true;
        let mut m = Json::object();
        m.set("type", "limit").set("code", verdict.code.code()).set("count", verdict.count);
        self.pending_events.push(m);
        // From this frame on: no blurred shadows, filters, backdrop-filter or transparency, animations stopped where
        // they are and the layers back in the document, so the stopped widget costs nothing to scroll.
        self.engine.set_no_effects();
        let now = self.now_s();
        self.update_composition(doc, now);
        doc.stale = false;
        self.dirty = true;
        self.touched_all = true;
        self.layers.composition_only = false;
        self.layers.painted_valid = false;
    }

    /// The `policy` message: counted unless compat, and `paint` reports.
    pub(crate) fn policy(&mut self, doc: &mut Document, on: bool, reports: bool) {
        self.layers.paint_reports = reports;
        self.layers.paint_reported = (-1, -1);
        if on && !self.layers.paint_on {
            self.layers.paint_scan = true;
            self.layers.motion_stale = true;
        }
        self.layers.paint_on = on;
        self.count_paint(doc);
    }

    /// `damage`: where the last paint differs from the one before; false when everything must be painted.
    pub(crate) fn damage(&self, out: &mut Vec<Rect>) -> bool {
        if !self.layers.damage_valid {
            return false;
        }
        out.clone_from(&self.layers.damage);
        true
    }
}
