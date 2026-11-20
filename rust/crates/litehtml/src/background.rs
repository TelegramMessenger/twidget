//! `background.h` and `background.cpp`: a background's layers as painting hands them to the container. A layer is
//! the boxes it is drawn in (`background_layer`: border, clip and origin box, radii, attachment, repeat), and what
//! fills it: the colour, an image's url, or a gradient's geometry (a linear one's end points, a radial one's centre
//! and radii, a conic one's centre, angle and reach) with its colour stops fixed up as CSS Images 4 asks and a
//! repeating gradient's period unrolled across the box (or its average colour where it cannot be).

use crate::Px;
use crate::borders::BorderRadiuses;
use crate::css_length::CssLength;
use crate::css_position::CssSize;
use crate::css_properties::Background;
use crate::css_values::*;
use crate::cxx;
use crate::document::{Document, StyleCx};
use crate::document_container::DocumentContainer;
use crate::element::ElementId;
use crate::gradient::*;
use crate::render_item::RenderId;
use crate::string_id::*;
use crate::types::{PointF, Position, Size};
use crate::web_color::WebColor;

const M_PI: f64 = std::f64::consts::PI;

/// `background_layer`: where a layer is drawn. `border_box` bounds everything drawn, with `border_radius`;
/// `clip_box` clips further; `origin_box` is where one copy of the image or gradient lies.
#[derive(Clone, Copy, Debug)]
pub struct BackgroundLayer {
    pub border_box: Position,
    pub border_radius: BorderRadiuses,
    pub clip_box: Position,
    pub origin_box: Position,
    /// A `background_attachment` value.
    pub attachment: i32,
    /// A `background_repeat` value.
    pub repeat: i32,
    /// The root element's: the container may paint it over the whole window.
    pub is_root: bool,
}

impl Default for BackgroundLayer {
    fn default() -> BackgroundLayer {
        BackgroundLayer {
            border_box: Position::default(),
            border_radius: BorderRadiuses::default(),
            clip_box: Position::default(),
            origin_box: Position::default(),
            attachment: BackgroundAttachment::background_attachment_scroll.index(),
            repeat: BackgroundRepeat::background_repeat_repeat.index(),
            is_root: false,
        }
    }
}

/// `background_layer::color_point`: a stop's offset along the gradient (0 to 1), its colour and the hint after it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ColorPoint {
    pub offset: f32,
    pub color: WebColor,
    pub hint: Option<f32>,
}

impl ColorPoint {
    /// `color_point(offset, color)`.
    pub fn new(offset: f32, color: WebColor) -> ColorPoint {
        ColorPoint { offset, color, hint: None }
    }
}

/// `background_layer::gradient_base`: the stops and the colour space they are mixed in (none unless the gradient's
/// kind sets them: only a conic gradient does).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GradientBase {
    pub color_points: Vec<ColorPoint>,
    pub color_space: ColorSpace,
    pub hue_interpolation: HueInterpolation,
}

/// `background_layer::linear_gradient`: from `start` to `end`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LinearGradient {
    pub base: GradientBase,
    pub start: PointF,
    pub end: PointF,
}

/// `background_layer::radial_gradient`: the centre and the ellipse's radii.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RadialGradient {
    pub base: GradientBase,
    pub position: PointF,
    pub radius: PointF,
}

/// `background_layer::conic_gradient`: the centre, the angle it starts at (degrees, clockwise from the top), and the
/// distance to the farthest corner of the box.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ConicGradient {
    pub base: GradientBase,
    pub position: PointF,
    pub angle: f32,
    pub radius: f32,
}

/// `background::layer_type`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LayerType {
    type_none,
    type_color,
    type_image,
    type_linear_gradient,
    type_radial_gradient,
    type_conic_gradient,
}

/// `background_layer::image`: an image layer's url and the base url it is read against.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ImageLayer {
    pub url: Vec<u8>,
    pub base_url: Vec<u8>,
}

impl Background {
    /// `is_empty()`: no colour (alpha 0) and no image that is not empty.
    pub fn is_empty(&self) -> bool {
        if self.m_color.alpha != 0 {
            return false;
        }
        if self.m_image.is_empty() {
            return true;
        }
        self.m_image.iter().all(|img| img.is_empty())
    }

    /// `get_layers_number()`: the images, and the colour unless it is transparent black.
    pub fn get_layers_number(&self) -> i32 {
        if self.m_color != WebColor::TRANSPARENT {
            return self.m_image.len() as i32 + 1;
        }
        self.m_image.len() as i32
    }

    /// `get_layer(idx, pos, el, ri, layer)`: layer `idx`'s boxes for an element whose content box is `pos` (the
    /// colour is the layer past the images): its border, padding or content box as `background-clip` and
    /// `background-origin` say, its radii from `el`'s style, and its origin box sized by `background-size` (an
    /// image's against the size the container knows of it) and placed by `background-position`.
    #[allow(clippy::too_many_arguments)]
    pub fn get_layer(
        &self,
        doc: &Document,
        cx: &StyleCx,
        idx: i32,
        pos: Position,
        el: ElementId,
        ri: RenderId,
        layer: &mut BackgroundLayer,
    ) -> bool {
        if idx < 0 || idx >= self.get_layers_number() {
            return false;
        }
        let item = doc.render.item(ri);
        let content_box = pos;
        let mut padding_box = pos;
        padding_box.grow(&item.padding);
        let mut border_box = padding_box;
        border_box.grow(&item.borders);

        layer.border_radius = doc.el(el).css.m_css_borders.radius.calc_percents(border_box.width, border_box.height);
        layer.border_box = border_box;
        layer.is_root = doc.el_is_root(el);

        let clip;
        let mut size = CssSize::default();
        let mut position_x = CssLength::default();
        let mut position_y = CssLength::default();

        if idx == self.m_image.len() as i32 {
            if self.m_image.is_empty() {
                clip = self.m_clip.first().copied().unwrap_or(BackgroundBox::background_box_border.index());
            } else {
                // m_clip[(idx - 1) % m_clip.size()]
                clip = if self.m_clip.is_empty() {
                    BackgroundBox::background_box_border.index()
                } else {
                    self.m_clip[(idx as usize - 1) % self.m_clip.len()]
                };
            }
        } else {
            let at = idx as usize;
            layer.attachment = if self.m_attachment.is_empty() {
                BackgroundAttachment::background_attachment_scroll.index()
            } else {
                self.m_attachment[at % self.m_attachment.len()]
            };
            layer.repeat = if self.m_repeat.is_empty() {
                BackgroundRepeat::background_repeat_repeat.index()
            } else {
                self.m_repeat[at % self.m_repeat.len()]
            };
            clip = if self.m_clip.is_empty() { BackgroundBox::background_box_border.index() } else { self.m_clip[at % self.m_clip.len()] };
            let origin = if self.m_origin.is_empty() {
                BackgroundBox::background_box_padding.index()
            } else {
                self.m_origin[at % self.m_origin.len()]
            };
            let auto = BackgroundSize::background_size_auto.index();
            size = if self.m_size.is_empty() {
                CssSize::new(CssLength::predef_value(auto), CssLength::predef_value(auto))
            } else {
                self.m_size[at % self.m_size.len()].clone()
            };
            position_x = if self.m_position_x.is_empty() {
                CssLength::new(0.0, CssUnits::css_units_percentage)
            } else {
                self.m_position_x[at % self.m_position_x.len()].clone()
            };
            position_y = if self.m_position_y.is_empty() {
                CssLength::new(0.0, CssUnits::css_units_percentage)
            } else {
                self.m_position_y[at % self.m_position_y.len()].clone()
            };
            layer.origin_box = if origin == BackgroundBox::background_box_border.index() {
                border_box
            } else if origin == BackgroundBox::background_box_content.index() {
                content_box
            } else {
                padding_box
            };
        }

        layer.clip_box = if clip == BackgroundBox::background_box_padding.index() {
            padding_box
        } else if clip == BackgroundBox::background_box_content.index() {
            content_box
        } else {
            border_box
        };

        let mut bg_size = Size::new(layer.origin_box.width, layer.origin_box.height);

        if self.get_layer_type(idx) == LayerType::type_image {
            if let Some(image_layer) = self.get_image_layer(idx) {
                let img_size =
                    cx.container.get_image_size(crate::gumbo::c_str(&image_layer.url), Some(crate::gumbo::c_str(&image_layer.base_url)));
                if img_size.width != Px(0.0) && img_size.height != Px(0.0) {
                    let mut img_new_sz = img_size;
                    let img_ar_width = img_size.width / img_size.height;
                    let img_ar_height = img_size.height / img_size.width;
                    let ob = layer.origin_box;
                    if size.width.is_predefined() {
                        let predef = size.width.predef();
                        if predef == BackgroundSize::background_size_contain.index() {
                            if (ob.width * img_ar_height) <= ob.height {
                                img_new_sz.width = ob.width;
                                img_new_sz.height = ob.width * img_ar_height;
                            } else {
                                img_new_sz.height = ob.height;
                                img_new_sz.width = ob.height * img_ar_width;
                            }
                        } else if predef == BackgroundSize::background_size_cover.index() {
                            if (ob.width * img_ar_height) >= ob.height {
                                img_new_sz.width = ob.width;
                                img_new_sz.height = ob.width * img_ar_height;
                            } else {
                                img_new_sz.height = ob.height;
                                img_new_sz.width = ob.height * img_ar_width;
                            }
                        } else if predef == BackgroundSize::background_size_auto.index() && !size.height.is_predefined() {
                            img_new_sz.height = size.height.calc_percent(ob.height);
                            img_new_sz.width = img_new_sz.height * img_ar_width;
                        }
                    } else {
                        img_new_sz.width = size.width.calc_percent(ob.width);
                        if size.height.is_predefined() {
                            img_new_sz.height = img_new_sz.width * img_ar_height;
                        } else {
                            img_new_sz.height = size.height.calc_percent(ob.height);
                        }
                    }
                    bg_size = img_new_sz;
                }
            }
        } else {
            if !size.width.is_predefined() {
                bg_size.width = size.width.calc_percent(layer.origin_box.width);
            }
            if !size.height.is_predefined() {
                bg_size.height = size.height.calc_percent(layer.origin_box.height);
            }
        }

        let new_origin_box = Position::new(
            layer.origin_box.x + position_x.calc_percent(layer.origin_box.width - bg_size.width),
            layer.origin_box.y + position_y.calc_percent(layer.origin_box.height - bg_size.height),
            bg_size.width,
            bg_size.height,
        );
        layer.origin_box = new_origin_box;
        true
    }

    /// `get_image_layer(idx)`: an image layer's url, with the background's base url.
    pub fn get_image_layer(&self, idx: i32) -> Option<ImageLayer> {
        if idx >= 0 && idx < self.m_image.len() as i32 && self.m_image[idx as usize].ty == ImageType::type_url {
            return Some(ImageLayer { url: self.m_image[idx as usize].url.clone(), base_url: self.m_baseurl.clone() });
        }
        None
    }

    /// `get_color_layer(idx)`: the colour, for the layer past the images.
    pub fn get_color_layer(&self, idx: i32) -> Option<WebColor> {
        (idx == self.m_image.len() as i32).then_some(self.m_color)
    }

    fn gradient_of(&self, idx: i32, linear: bool, radial: bool) -> Option<&Gradient> {
        if idx < 0 || idx >= self.m_image.len() as i32 {
            return None;
        }
        let image = &self.m_image[idx as usize];
        if image.ty != ImageType::type_gradient {
            return None;
        }
        let g = &image.m_gradient;
        let ok = if linear {
            g.m_type == _linear_gradient_ || g.m_type == _repeating_linear_gradient_
        } else if radial {
            g.m_type == _radial_gradient_ || g.m_type == _repeating_radial_gradient_
        } else {
            g.m_type == _conic_gradient_ || g.m_type == _repeating_conic_gradient_
        };
        ok.then_some(g)
    }

    /// `get_linear_gradient_layer(idx, layer)`: the end points of a gradient at its angle (or towards its side or
    /// corner) over the origin box, and its stops along that line.
    pub fn get_linear_gradient_layer(&self, idx: i32, layer: &BackgroundLayer) -> Option<LinearGradient> {
        let g = self.gradient_of(idx, true, false)?;
        let mut ret = LinearGradient::default();
        let angle = if g.m_side == 0 {
            g.angle
        } else {
            let mut rise = layer.origin_box.width.value();
            let mut run = layer.origin_box.height.value();
            if g.m_side & gradient_side_left != 0 {
                run *= -1.0;
            }
            if g.m_side & gradient_side_bottom != 0 {
                rise *= -1.0;
            }
            // 90 - atan2(rise, run) * 180 / M_PI: atan2 of floats is a float, times 180 a float, then over M_PI a double.
            (90.0 - f64::from(rise.atan2(run) * 180.0) / M_PI) as f32
        };
        end_points_from_angle(
            angle,
            Size::new(Px(layer.origin_box.width.value()), Px(layer.origin_box.height.value())),
            &mut ret.start,
            &mut ret.end,
        );
        ret.start.x += layer.origin_box.x.value();
        ret.start.y += layer.origin_box.y.value();
        ret.end.x += layer.origin_box.x.value();
        ret.end.y += layer.origin_box.y.value();

        let line_len = distance(&ret.start, &ret.end);
        if !ret.base.prepare_color_points(line_len, g.m_type, &g.m_colors) {
            return None;
        }
        Some(ret)
    }

    /// `get_radial_gradient_layer(idx, layer)`: the centre over the origin box and the radii its extent keyword
    /// or lengths give, and its stops along the horizontal radius.
    pub fn get_radial_gradient_layer(&self, idx: i32, layer: &BackgroundLayer) -> Option<RadialGradient> {
        let g = self.gradient_of(idx, false, true)?;
        let ob = &layer.origin_box;
        let mut ret = RadialGradient { position: gradient_center(g, ob), ..RadialGradient::default() };
        let (left, right, top, bottom) = (ob.left().value(), ob.right().value(), ob.top().value(), ob.bottom().value());
        let p = ret.position;
        if g.radial_extent != RadialExtent::radial_extent_none {
            match g.radial_extent {
                RadialExtent::radial_extent_closest_corner => {
                    if g.radial_shape == radial_shape_circle {
                        let corner1 = distance(&p, &PointF::new(left, top));
                        let corner2 = distance(&p, &PointF::new(right, top));
                        let corner3 = distance(&p, &PointF::new(left, bottom));
                        let corner4 = distance(&p, &PointF::new(right, bottom));
                        let r = cxx::min_list_f32(&[corner1, corner2, corner3, corner4]);
                        ret.radius = PointF::new(r, r);
                    } else {
                        // Aspect ratio is the same as for radial_extent_closest_side
                        let aspect_ration =
                            cxx::min_f32((p.x - left).abs(), (p.x - right).abs()) / cxx::min_f32((p.y - top).abs(), (p.y - bottom).abs());
                        let corner = find_corner(&p, ob, false);
                        ret.radius = calc_ellipse_radius(&corner, aspect_ration);
                    }
                }
                RadialExtent::radial_extent_closest_side => {
                    if g.radial_shape == radial_shape_circle {
                        let r = cxx::min_list_f32(&[(p.x - left).abs(), (p.x - right).abs(), (p.y - top).abs(), (p.y - bottom).abs()]);
                        ret.radius = PointF::new(r, r);
                    } else {
                        ret.radius.x = cxx::min_f32((p.x - left).abs(), (p.x - right).abs());
                        ret.radius.y = cxx::min_f32((p.y - top).abs(), (p.y - bottom).abs());
                    }
                }
                RadialExtent::radial_extent_farthest_corner => {
                    if g.radial_shape == radial_shape_circle {
                        let corner1 = distance(&p, &PointF::new(left, top));
                        let corner2 = distance(&p, &PointF::new(right, top));
                        let corner3 = distance(&p, &PointF::new(left, bottom));
                        let corner4 = distance(&p, &PointF::new(right, bottom));
                        let r = cxx::max_list_f32(&[corner1, corner2, corner3, corner4]);
                        ret.radius = PointF::new(r, r);
                    } else {
                        // Aspect ratio is the same as for radial_extent_farthest_side
                        let aspect_ration =
                            cxx::max_f32((p.x - left).abs(), (p.x - right).abs()) / cxx::max_f32((p.y - top).abs(), (p.y - bottom).abs());
                        let corner = find_corner(&p, ob, true);
                        ret.radius = calc_ellipse_radius(&corner, aspect_ration);
                    }
                }
                RadialExtent::radial_extent_farthest_side => {
                    if g.radial_shape == radial_shape_circle {
                        let r = cxx::max_list_f32(&[(p.x - left).abs(), (p.x - right).abs(), (p.y - top).abs(), (p.y - bottom).abs()]);
                        ret.radius = PointF::new(r, r);
                    } else {
                        ret.radius.x = cxx::max_f32((p.x - left).abs(), (p.x - right).abs());
                        ret.radius.y = cxx::max_f32((p.y - top).abs(), (p.y - bottom).abs());
                    }
                }
                _ => {}
            }
        }
        if !g.radial_radius_x.is_predefined() {
            ret.radius.x = g.radial_radius_x.calc_percent(ob.width).value();
        }
        if !g.radial_radius_y.is_predefined() {
            ret.radius.y = g.radial_radius_y.calc_percent(ob.height).value();
        }
        if ret.base.prepare_color_points(ret.radius.x, g.m_type, &g.m_colors) {
            return Some(ret);
        }
        None
    }

    /// `get_conic_gradient_layer(idx, layer)`: the centre over the origin box, the starting angle, the distance to
    /// the farthest corner, the colour space, and the stops by angle.
    pub fn get_conic_gradient_layer(&self, idx: i32, layer: &BackgroundLayer) -> Option<ConicGradient> {
        let g = self.gradient_of(idx, false, false)?;
        let ob = &layer.origin_box;
        let mut ret = ConicGradient { position: gradient_center(g, ob), ..ConicGradient::default() };
        ret.angle = g.conic_from_angle;
        ret.base.color_space = g.color_space;
        ret.base.hue_interpolation = g.hue_interpolation;
        let (left, right, top, bottom) = (ob.left().value(), ob.right().value(), ob.top().value(), ob.bottom().value());
        let p = ret.position;
        let corner1 = distance(&p, &PointF::new(left, top));
        let corner2 = distance(&p, &PointF::new(right, top));
        let corner3 = distance(&p, &PointF::new(left, bottom));
        let corner4 = distance(&p, &PointF::new(right, bottom));
        ret.radius = cxx::max_list_f32(&[corner1, corner2, corner3, corner4]);
        if ret.base.prepare_color_points(0.0, g.m_type, &g.m_colors) {
            return Some(ret);
        }
        None
    }

    /// `get_layer_type(idx)`.
    pub fn get_layer_type(&self, idx: i32) -> LayerType {
        if idx >= 0 && idx < self.m_image.len() as i32 {
            let image = &self.m_image[idx as usize];
            match image.ty {
                ImageType::type_url => return LayerType::type_image,
                ImageType::type_gradient => {
                    let t = image.m_gradient.m_type;
                    if t == _linear_gradient_ || t == _repeating_linear_gradient_ {
                        return LayerType::type_linear_gradient;
                    }
                    if t == _radial_gradient_ || t == _repeating_radial_gradient_ {
                        return LayerType::type_radial_gradient;
                    }
                    if t == _conic_gradient_ || t == _repeating_conic_gradient_ {
                        return LayerType::type_conic_gradient;
                    }
                }
                ImageType::type_none => {}
            }
        } else if idx == self.m_image.len() as i32 {
            return LayerType::type_color;
        }
        LayerType::type_none
    }

    /// `draw_layer(hdc, idx, layer, container)`: the colour filled, or (over an origin box that is not empty) the
    /// image drawn or the gradient's geometry drawn when it has stops.
    pub fn draw_layer(&self, hdc: u64, idx: i32, layer: &BackgroundLayer, container: &dyn DocumentContainer) {
        let sized = layer.origin_box.width != Px(0.0) && layer.origin_box.height != Px(0.0);
        match self.get_layer_type(idx) {
            LayerType::type_color => {
                if let Some(color) = self.get_color_layer(idx) {
                    container.draw_solid_fill(hdc, layer, &color);
                }
            }
            LayerType::type_image => {
                if sized && let Some(image_layer) = self.get_image_layer(idx) {
                    container.draw_image(hdc, layer, &image_layer.url, &image_layer.base_url);
                }
            }
            LayerType::type_linear_gradient => {
                if sized && let Some(gradient_layer) = self.get_linear_gradient_layer(idx, layer) {
                    container.draw_linear_gradient(hdc, layer, &gradient_layer);
                }
            }
            LayerType::type_radial_gradient => {
                if sized && let Some(gradient_layer) = self.get_radial_gradient_layer(idx, layer) {
                    container.draw_radial_gradient(hdc, layer, &gradient_layer);
                }
            }
            LayerType::type_conic_gradient => {
                if sized && let Some(gradient_layer) = self.get_conic_gradient_layer(idx, layer) {
                    container.draw_conic_gradient(hdc, layer, &gradient_layer);
                }
            }
            LayerType::type_none => {}
        }
    }
}

/// A radial or conic gradient's centre: the origin box's middle, or its side or length as `m_side` says.
fn gradient_center(g: &Gradient, ob: &Position) -> PointF {
    let mut position = PointF::new(ob.x.value() + ob.width.value() / 2.0, ob.y.value() + ob.height.value() / 2.0);
    if g.m_side & gradient_side_left != 0 {
        position.x = ob.left().value();
    } else if g.m_side & gradient_side_right != 0 {
        position.x = ob.right().value();
    } else if g.m_side & gradient_side_x_center != 0 {
        position.x = ob.left().value() + ob.width.value() / 2.0;
    } else if g.m_side & gradient_side_x_length != 0 {
        position.x = ob.left().value() + g.position_x.calc_percent(ob.width).value();
    }
    if g.m_side & gradient_side_top != 0 {
        position.y = ob.top().value();
    } else if g.m_side & gradient_side_bottom != 0 {
        position.y = ob.bottom().value();
    } else if g.m_side & gradient_side_y_center != 0 {
        position.y = ob.top().value() + ob.height.value() / 2.0;
    } else if g.m_side & gradient_side_y_length != 0 {
        position.y = ob.top().value() + g.position_y.calc_percent(ob.height).value();
    }
    position
}

/// `EndPointsFromAngle(angle_deg, size, first_point, second_point)`: the points a gradient of the angle runs
/// between so that it covers a box of the size (Chromium's css_gradient_value.cc).
fn end_points_from_angle(angle_deg: f32, size: Size, first_point: &mut PointF, second_point: &mut PointF) {
    let mut angle_deg = angle_deg % 360.0;
    if angle_deg < 0.0 {
        angle_deg += 360.0;
    }

    if Px(angle_deg) == Px(0.0) {
        *first_point = PointF::new(0.0, size.height.value());
        *second_point = PointF::new(0.0, 0.0);
        return;
    }
    if Px(angle_deg) == Px(90.0) {
        *first_point = PointF::new(0.0, 0.0);
        *second_point = PointF::new(size.width.value(), 0.0);
        return;
    }
    if Px(angle_deg) == Px(180.0) {
        *first_point = PointF::new(0.0, 0.0);
        *second_point = PointF::new(0.0, size.height.value());
        return;
    }
    if Px(angle_deg) == Px(270.0) {
        *first_point = PointF::new(size.width.value(), 0.0);
        *second_point = PointF::new(0.0, 0.0);
        return;
    }

    // angleDeg is a "bearing angle" (0deg = N, 90deg = E),
    // but tan expects 0deg = E, 90deg = N.
    let slope = ((90.0 - f64::from(angle_deg)) * M_PI / 180.0).tan() as f32;

    // We find the endpoint by computing the intersection of the line formed by
    // the slope, and a line perpendicular to it that intersects the corner.
    let perpendicular_slope = -1.0 / slope;

    // Compute start corner relative to center, in Cartesian space (+y = up).
    let half_height = size.height.value() / 2.0;
    let half_width = size.width.value() / 2.0;
    let end_corner = if angle_deg < 90.0 {
        PointF::new(half_width, half_height)
    } else if angle_deg < 180.0 {
        PointF::new(half_width, -half_height)
    } else if angle_deg < 270.0 {
        PointF::new(-half_width, -half_height)
    } else {
        PointF::new(-half_width, half_height)
    };

    // Compute c (of y = mx + c) using the corner point.
    let c = end_corner.y - perpendicular_slope * end_corner.x;
    let end_x = c / (slope - perpendicular_slope);
    let end_y = perpendicular_slope * end_x + c;

    // We computed the end point, so set the second point, taking into account the
    // moved origin and the fact that we're in drawing space (+y = down).
    *second_point = PointF::new(half_width + end_x, half_height - end_y);
    // Reflect around the center for the start point.
    *first_point = PointF::new(half_width - end_x, half_height + end_y);
}

/// `distance(p1, p2)`: the differences in floats, the rest in doubles, narrowed.
fn distance(p1: &PointF, p2: &PointF) -> f32 {
    let dx = f64::from(p2.x - p1.x);
    let dy = f64::from(p2.y - p1.y);
    (dx * dx + dy * dy).sqrt() as f32
}

/// `calc_ellipse_radius(offset, aspect_ratio)`: the ellipse of the ratio through the offset; flat (0 × 0) for a
/// ratio that is 0, infinite or NaN.
fn calc_ellipse_radius(offset: &PointF, aspect_ratio: f32) -> PointF {
    // If the aspectRatio is 0 or infinite, the ellipse is completely flat.
    // (If it is NaN, the ellipse is 0x0, and should be handled as zero width.)
    if !aspect_ratio.is_finite() || Px(aspect_ratio) == Px(0.0) {
        return PointF::new(0.0, 0.0);
    }
    // x^2/a^2 + y^2/b^2 = 1
    // a/b = aspectRatio, b = a/aspectRatio
    // a = sqrt(x^2 + y^2/(1/aspect_ratio^2))
    let a = (offset.x * offset.x + offset.y * offset.y * aspect_ratio * aspect_ratio).sqrt();
    PointF::new(a, a / aspect_ratio)
}

/// `find_corner(center, box, farthest)`: the corner of the box farthest from (or closest to) the centre, from the
/// centre, the first in left-top, right-top, right-bottom, left-bottom order on a tie.
fn find_corner(center: &PointF, bx: &Position, farthest: bool) -> PointF {
    let (left, right, top, bottom) = (bx.left().value(), bx.right().value(), bx.top().value(), bx.bottom().value());
    // Default is left-top corner
    let mut ret = PointF::new(left, top);
    let mut dist = distance(center, &PointF::new(left, top));

    // Check right-top corner
    let mut next_dist = distance(center, &PointF::new(right, top));
    if (farthest && next_dist > dist) || (!farthest && next_dist < dist) {
        ret = PointF::new(right, top);
        dist = next_dist;
    }

    // Check right-bottom corner
    next_dist = distance(center, &PointF::new(right, bottom));
    if (farthest && next_dist > dist) || (!farthest && next_dist < dist) {
        ret = PointF::new(right, bottom);
        dist = next_dist;
    }

    // Check left-bottom corner
    next_dist = distance(center, &PointF::new(left, bottom));
    if (farthest && next_dist > dist) || (!farthest && next_dist < dist) {
        ret = PointF::new(left, bottom);
    }

    ret.x -= center.x;
    ret.y -= center.y;
    ret
}

/// `v > 0`, false for NaN.
fn positive(v: f64) -> bool {
    v > 0.0
}

/// `max_repeated_color_points`: the most stops a repeating gradient unrolls to; the platforms' painters refuse
/// longer lists (Android: 4096).
const max_repeated_color_points: usize = 4096;

/// `std::lround(v)`: half away from zero, to a `long` (32 bits on Windows: past it the result is unspecified and
/// MSVC gives `LONG_MIN`, as the cast does on x86).
fn lround(v: f64) -> i64 {
    let r = v.round();
    if cfg!(windows) { i64::from(cxx::f64_to_int(r)) } else { r as i64 }
}

/// `average_color(points, evenly)`: the gradient's average colour, premultiplied
/// (https://drafts.csswg.org/css-images-3/#repeating-gradients): over the stops as placed, or, with `evenly`, over
/// the same stops spaced evenly.
fn average_color(points: &[ColorPoint], evenly: bool) -> WebColor {
    let (mut r, mut g, mut b, mut a, mut total) = (0.0f64, 0.0f64, 0.0f64, 0.0f64, 0.0f64);
    let mut i = 0;
    while i + 1 < points.len() {
        let width = if evenly { 1.0 } else { f64::from(points[i + 1].offset) - f64::from(points[i].offset) };
        if width > 0.0 {
            for c in [&points[i].color, &points[i + 1].color] {
                let alpha = f64::from(c.alpha) / 255.0;
                r += f64::from(c.red) * alpha * width / 2.0;
                g += f64::from(c.green) * alpha * width / 2.0;
                b += f64::from(c.blue) * alpha * width / 2.0;
                a += alpha * width / 2.0;
            }
            total += width;
        }
        i += 1;
    }
    if !positive(total) {
        return points[points.len() - 1].color;
    }
    let mut out = WebColor::default();
    out.alpha = lround(a / total * 255.0) as u8;
    if a > 0.0 {
        out.red = 255i64.min(lround(r / a)) as u8;
        out.green = 255i64.min(lround(g / a)) as u8;
        out.blue = 255i64.min(lround(b / a)) as u8;
    }
    out
}

/// `repeat_color_points(color_points, authored)`: the period's copies across [0, 1], placed in doubles and
/// narrowed once, a seam's stop reusing the float already there; the average colour instead when an offset is not
/// finite, the period is not positive, the list is past the budget, or the copies would be (`authored` is the
/// stops before `color_points_transparent_fix`).
fn repeat_color_points(color_points: &mut Vec<ColorPoint>, authored: &[ColorPoint]) {
    let first0 = f64::from(color_points[0].offset);
    let last0 = f64::from(color_points[color_points.len() - 1].offset);
    let period = last0 - first0;

    let draw_average = |color_points: &mut Vec<ColorPoint>, evenly: bool| {
        let color = average_color(authored, evenly);
        color_points.clear();
        color_points.push(ColorPoint::new(0.0, color));
        color_points.push(ColorPoint::new(1.0, color));
    };

    let nonfinite = color_points.iter().any(|cp| !cp.offset.is_finite());
    if nonfinite || !positive(period) || color_points.len() > max_repeated_color_points {
        draw_average(color_points, nonfinite || !positive(period));
        return;
    }

    if first0 > 1.0 || last0 < 0.0 {
        let k = (first0 / period).ceil();
        let slid = |offset: f64| {
            if k >= 1.0 { first0 + (offset - last0) - (k - 1.0) * period } else { last0 + (offset - first0) - (k + 1.0) * period }
        };
        let sfirst = slid(first0);
        let slast = slid(last0);
        let flt_max = f64::from(f32::MAX);
        if k != 0.0 && sfirst.is_finite() && slast.is_finite() && sfirst.abs() <= flt_max && slast.abs() <= flt_max {
            for cp in color_points.iter_mut() {
                cp.offset = slid(f64::from(cp.offset)) as f32;
            }
        }
    }

    let limit = max_repeated_color_points;
    let sfirst = f64::from(color_points[0].offset);
    let slast = f64::from(color_points[color_points.len() - 1].offset);
    let size = color_points.len() as f64;
    let certain = |gap: f64| if gap > 0.0 { cxx::max_f64(0.0, (gap / period).floor() - 1.0) * size + 1.0 } else { 0.0 };
    if size + certain(1.0 - slast) + certain(sfirst) > limit as f64 {
        draw_average(color_points, false);
        return;
    }

    let old_points = color_points.clone();
    let mut back_off = slast;
    if back_off < 1.0 {
        let mut iter = 0usize;
        let mut copy: i64 = 1;
        let mut seam = color_points[color_points.len() - 1].offset;
        while back_off < 1.0 && color_points.len() < limit {
            let at = if old_points[iter].offset == old_points[0].offset {
                seam
            } else {
                (f64::from(old_points[iter].offset) + copy as f64 * period) as f32
            };
            back_off = f64::from(at);
            color_points.push(ColorPoint::new(at, old_points[iter].color));
            iter += 1;
            if iter == old_points.len() {
                iter = 0;
                copy += 1;
                seam = at;
            }
        }
    }
    let mut front_off = sfirst;
    if front_off > 0.0 {
        // Built backwards and put in front once.
        let mut before: Vec<ColorPoint> = Vec::new();
        let mut iter = old_points.len() - 1;
        let mut copy: i64 = 1;
        let mut seam = color_points[0].offset;
        while front_off > 0.0 && color_points.len() + before.len() < limit {
            let at = if old_points[iter].offset == old_points[old_points.len() - 1].offset {
                seam
            } else {
                (f64::from(old_points[iter].offset) - copy as f64 * period) as f32
            };
            front_off = f64::from(at);
            before.push(ColorPoint::new(at, old_points[iter].color));
            if iter == 0 {
                iter = old_points.len() - 1;
                copy += 1;
                seam = at;
            } else {
                iter -= 1;
            }
        }
        before.reverse();
        before.extend_from_slice(color_points);
        *color_points = before;
    }

    if back_off < 1.0 || front_off > 0.0 {
        draw_average(color_points, false);
    }
}

/// `normalize_length(length, line_len)`: a % as a fraction, else the length over the line's (as it is when that
/// is 0).
fn normalize_length(length: &CssLength, line_len: f32) -> f32 {
    if length.units() == CssUnits::css_units_percentage {
        return length.val() / 100.0;
    }
    if Px(line_len) != Px(0.0) {
        return length.val() / line_len;
    }
    length.val()
}

impl GradientBase {
    /// `color_points_transparent_fix()`: a transparent stop takes its neighbour's colour (an interior one is split
    /// into two, the previous colour's and the next's, at its offset), its alpha kept 0.
    pub fn color_points_transparent_fix(&mut self) {
        let mut i = 0usize;
        while i < self.color_points.len() {
            if self.color_points[i].color.alpha == 0 {
                if i == 0 {
                    if i + 1 < self.color_points.len() {
                        self.color_points[i].color = self.color_points[i + 1].color;
                        self.color_points[i].color.alpha = 0;
                    }
                } else if i + 1 == self.color_points.len() {
                    self.color_points[i].color = self.color_points[i - 1].color;
                    self.color_points[i].color.alpha = 0;
                } else {
                    self.color_points[i].color = self.color_points[i + 1].color;
                    self.color_points[i].color.alpha = 0;
                    let mut cpt = ColorPoint::new(self.color_points[i].offset, self.color_points[i - 1].color);
                    cpt.color.alpha = 0;
                    self.color_points.insert(i, cpt);
                    i += 1;
                }
            }
            i += 1;
        }
    }

    /// `prepare_color_points(line_len, grad_type, colors)`: the stops at their offsets (a length over the line's,
    /// an angle over 360), hints on the stop before them, fixed up (the first at 0, the last at 1, each at least
    /// the largest before it, the ones without a position spread evenly), a plain gradient's ends extended to 0 and
    /// 1, transparent stops given their neighbours' colours, a repeating gradient's period unrolled. False without
    /// stops.
    pub fn prepare_color_points(&mut self, line_len: f32, g_type: crate::StringId, colors: &[ColorStop]) -> bool {
        let mut repeating = false;
        if g_type == _linear_gradient_ || g_type == _radial_gradient_ || g_type == _conic_gradient_ {
            repeating = false;
        } else if g_type == _repeating_linear_gradient_ || g_type == _repeating_radial_gradient_ || g_type == _repeating_conic_gradient_ {
            repeating = true;
        }
        // Which positions the stylesheet gave: an explicit 0 is a position, not a missing one.
        let mut given: Vec<bool> = Vec::new();
        let mut has_transparent = false;
        for item in colors {
            if item.is_color_hint {
                if let Some(last) = self.color_points.last_mut() {
                    last.hint = Some(match &item.length {
                        Some(length) => normalize_length(length, line_len),
                        None => item.angle.unwrap_or(0.0) / 360.0,
                    });
                }
                continue;
            }
            if item.color.alpha == 0 {
                has_transparent = true;
            }
            if let Some(length) = &item.length {
                self.color_points.push(ColorPoint::new(normalize_length(length, line_len), item.color));
            } else if let Some(angle) = item.angle {
                self.color_points.push(ColorPoint::new(angle / 360.0, item.color));
            } else {
                self.color_points.push(ColorPoint::new(0.0, item.color));
            }
            given.push(item.length.is_some() || item.angle.is_some());
        }
        if self.color_points.is_empty() {
            return false;
        }

        // https://drafts.csswg.org/css-images-4/#color-stop-fixup: the first stop defaults to 0 and the last to
        // 100%, a position before a larger one moves up to it, and the stops between two positions share the gap.
        let n = self.color_points.len();
        if !given[0] {
            self.color_points[0].offset = 0.0;
            given[0] = true;
        }
        if !given[n - 1] {
            self.color_points[n - 1].offset = 1.0;
            given[n - 1] = true;
        }
        let mut largest = self.color_points[0].offset;
        for i in 0..n {
            if given[i] {
                self.color_points[i].offset = cxx::max_f32(self.color_points[i].offset, largest);
                largest = self.color_points[i].offset;
            }
        }
        let mut i = 1;
        while i < n {
            if given[i] {
                i += 1;
                continue;
            }
            let mut j = i;
            while !given[j] {
                j += 1;
            }
            let from = self.color_points[i - 1].offset;
            let to = self.color_points[j].offset;
            for k in i..j {
                self.color_points[k].offset = from + (to - from) * (k - i + 1) as f32 / (j - i + 1) as f32;
            }
            i = j + 1;
        }
        if !repeating {
            if self.color_points[0].offset > 0.0 {
                let color = self.color_points[0].color;
                self.color_points.insert(0, ColorPoint::new(0.0, color));
            }
            if self.color_points[self.color_points.len() - 1].offset < 1.0 {
                let color = self.color_points[self.color_points.len() - 1].color;
                self.color_points.push(ColorPoint::new(1.0, color));
            }
        }

        if repeating {
            // The authored stops for the average: color_points_transparent_fix() doubles an interior transparent stop.
            let authored = self.color_points.clone();
            if has_transparent {
                self.color_points_transparent_fix();
            }
            repeat_color_points(&mut self.color_points, &authored);
        } else if has_transparent {
            self.color_points_transparent_fix();
        }
        true
    }
}
