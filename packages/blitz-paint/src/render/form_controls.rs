use super::{BlitzDomPainter, ElementCx};
use crate::color::{Color, ToColorColor as _};
use crate::text::stroke_text;
use anyrender::PaintScene;
use blitz_dom::{BaseDocument, Node, local_name, node::SelectMode, util::ToColorColor as _};
use kurbo::{Affine, BezPath, Cap, Circle, Join, Point, Rect, RoundedRect, Stroke, Vec2};
use peniko::Fill;
use style::dom::TElement as _;
use style::values::specified::TextAlignKeyword;

const SELECT_TEXT_PADDING: f64 = 6.0;
const SELECT_CHEVRON_RESERVED_WIDTH: f64 = 18.0;

impl ElementCx<'_> {
    pub(super) fn draw_input(&self, scene: &mut impl PaintScene) {
        if self.node.local_name() != "input" {
            return;
        }
        let Some(checked) = self.element.checkbox_input_checked() else {
            return;
        };

        let type_attr = self.node.attr(local_name!("type"));
        let disabled = self.node.attr(local_name!("disabled")).is_some();

        // TODO this should be coming from css accent-color, but I couldn't find how to retrieve it
        let accent_color = if disabled {
            Color::from_rgba8(209, 209, 209, 255)
        } else {
            self.style.clone_color().as_srgb_color()
        };

        let width = self.frame.border_box.width();
        let height = self.frame.border_box.height();
        let min_dimension = width.min(height);
        let scale = (min_dimension - 4.0).max(0.0) / 16.0;

        let frame = self.frame.border_box.to_rounded_rect(scale * 2.0);

        match type_attr {
            Some("checkbox") => {
                draw_checkbox(scene, checked, frame, self.transform, accent_color, scale);
            }
            Some("radio") => {
                let center = frame.center();
                draw_radio_button(scene, checked, center, self.transform, accent_color, scale);
            }
            _ => {}
        }
    }

    pub(super) fn draw_select(&self, scene: &mut impl PaintScene) {
        if self.node.local_name() != "select" {
            return;
        }
        let Some(select) = self.element.select_data() else {
            return;
        };

        let content_x = self.node.final_layout.content_box_x() as f64 * self.scale;
        let content_y = self.node.final_layout.content_box_y() as f64 * self.scale;
        let content_width = self.node.final_layout.content_box_width() as f64 * self.scale;
        let content_height = self.node.final_layout.content_box_height() as f64 * self.scale;
        let row_height = select.row_height as f64 * self.scale;
        let chevron_inset = 14.0 * self.scale;
        let chevron_color = resolved_select_foreground_color(self.node);

        match select.mode {
            SelectMode::Dropdown => {
                if let Some((_, option)) = select.options.iter().enumerate().find(|(_, option)| {
                    self.context
                        .dom
                        .get_node(option.node_id)
                        .and_then(|node| node.element_data())
                        .and_then(|element| element.option_data())
                        .is_some_and(|data| data.selected)
                }) {
                    draw_option_label(
                        scene,
                        self.context.dom,
                        option,
                        self.transform,
                        option_label_x(
                            self.node,
                            option,
                            content_x,
                            content_width,
                            self.scale,
                            true,
                        ),
                        content_y
                            + ((content_height - option_text_height(option, self.scale)) / 2.0)
                                .max(0.0),
                    );
                }

                draw_select_chevron(
                    scene,
                    self.transform,
                    chevron_color,
                    content_x + content_width - chevron_inset,
                    content_y + content_height / 2.0,
                    self.scale,
                );
            }
            SelectMode::Listbox => {
                for (index, option) in select.options.iter().enumerate() {
                    let row_top = content_y + row_height * index as f64;
                    let row_rect = Rect::new(
                        content_x,
                        row_top,
                        content_x + content_width,
                        row_top + row_height,
                    );
                    let is_selected = self
                        .context
                        .dom
                        .get_node(option.node_id)
                        .and_then(|node| node.element_data())
                        .and_then(|element| element.option_data())
                        .is_some_and(|data| data.selected);
                    let is_active = select.active_index == Some(index);
                    if let Some(fill) = option_fill_color(
                        self.context.dom,
                        self.node,
                        self.node.is_focussed(),
                        is_selected,
                        is_active,
                    ) {
                        scene.fill(Fill::NonZero, self.transform, fill, None, &row_rect);
                    }

                    draw_option_label(
                        scene,
                        self.context.dom,
                        option,
                        self.transform,
                        option_label_x(
                            self.node,
                            option,
                            content_x,
                            content_width,
                            self.scale,
                            false,
                        ),
                        row_top
                            + ((row_height - option_text_height(option, self.scale)) / 2.0)
                                .max(0.0),
                    );
                }
            }
        }
    }
}

impl BlitzDomPainter<'_> {
    pub(super) fn draw_open_select_popup(&self, scene: &mut impl PaintScene) {
        let Some(select_id) = self.dom.open_select_popup_id() else {
            return;
        };
        let Some((x, y, width, height)) = self.dom.select_popup_rect(select_id) else {
            return;
        };
        let Some(node) = self.dom.get_node(select_id) else {
            return;
        };
        let Some(select) = node
            .element_data()
            .and_then(|element| element.select_data())
        else {
            return;
        };

        let viewport_scroll = self.dom.viewport_scroll();
        let popup_x = (self.initial_x - viewport_scroll.x + x as f64) * self.scale;
        let popup_y = (self.initial_y - viewport_scroll.y + y as f64) * self.scale;
        let transform = Affine::translate((popup_x, popup_y));
        let popup_rect = Rect::new(
            0.0,
            0.0,
            width as f64 * self.scale,
            height as f64 * self.scale,
        );
        let row_height = select.row_height as f64 * self.scale;
        let popup_background = resolved_popup_background_color(self.dom, node);
        let popup_border = resolved_select_border_color(node);

        scene.fill(
            Fill::NonZero,
            transform,
            popup_background,
            None,
            &popup_rect,
        );
        scene.stroke(
            &Stroke::new(1.0),
            transform,
            popup_border,
            None,
            &popup_rect,
        );

        for (index, option) in select.options.iter().enumerate() {
            let row_top = row_height * index as f64;
            let row_rect = Rect::new(
                0.0,
                row_top,
                width as f64 * self.scale,
                row_top + row_height,
            );
            let is_selected = self
                .dom
                .get_node(option.node_id)
                .and_then(|node| node.element_data())
                .and_then(|element| element.option_data())
                .is_some_and(|data| data.selected);
            let is_active = select.active_index == Some(index);
            if let Some(fill) =
                option_fill_color(self.dom, node, node.is_focussed(), is_selected, is_active)
            {
                scene.fill(Fill::NonZero, transform, fill, None, &row_rect);
            }

            draw_option_label(
                scene,
                self.dom,
                option,
                transform,
                option_label_x(
                    node,
                    option,
                    0.0,
                    width as f64 * self.scale,
                    self.scale,
                    false,
                ),
                row_top + ((row_height - option_text_height(option, self.scale)) / 2.0).max(0.0),
            );
        }
    }
}

fn draw_checkbox(
    scene: &mut impl PaintScene,
    checked: bool,
    frame: RoundedRect,
    transform: Affine,
    accent_color: Color,
    scale: f64,
) {
    if checked {
        scene.fill(Fill::NonZero, transform, accent_color, None, &frame);
        //Tick code derived from masonry
        let mut path = BezPath::new();
        path.move_to((2.0, 9.0));
        path.line_to((6.0, 13.0));
        path.line_to((14.0, 2.0));

        path.apply_affine(Affine::translate(Vec2 { x: 2.0, y: 1.0 }).then_scale(scale));

        let style = Stroke {
            width: 2.0 * scale,
            join: Join::Round,
            miter_limit: 10.0,
            start_cap: Cap::Round,
            end_cap: Cap::Round,
            dash_pattern: Default::default(),
            dash_offset: 0.0,
        };

        scene.stroke(&style, transform, Color::WHITE, None, &path);
    } else {
        scene.fill(Fill::NonZero, transform, Color::WHITE, None, &frame);
        scene.stroke(&Stroke::default(), transform, accent_color, None, &frame);
    }
}

fn draw_radio_button(
    scene: &mut impl PaintScene,
    checked: bool,
    center: Point,
    transform: Affine,
    accent_color: Color,
    scale: f64,
) {
    let outer_ring = Circle::new(center, 8.0 * scale);
    let gap = Circle::new(center, 6.0 * scale);
    let inner_circle = Circle::new(center, 4.0 * scale);
    if checked {
        scene.fill(Fill::NonZero, transform, accent_color, None, &outer_ring);
        scene.fill(Fill::NonZero, transform, Color::WHITE, None, &gap);
        scene.fill(Fill::NonZero, transform, accent_color, None, &inner_circle);
    } else {
        const GRAY: Color = color::palette::css::GRAY;
        scene.fill(Fill::NonZero, transform, GRAY, None, &outer_ring);
        scene.fill(Fill::NonZero, transform, Color::WHITE, None, &gap);
    }
}

fn option_text_height(option: &blitz_dom::node::SelectOption, scale: f64) -> f64 {
    (option.layout.height() as f64 / option.layout.scale() as f64) * scale
}

fn option_text_width(option: &blitz_dom::node::SelectOption, scale: f64) -> f64 {
    (option.layout.full_width() as f64 / option.layout.scale() as f64) * scale
}

fn option_label_x(
    node: &Node,
    option: &blitz_dom::node::SelectOption,
    content_x: f64,
    content_width: f64,
    scale: f64,
    reserve_chevron: bool,
) -> f64 {
    let left_padding = SELECT_TEXT_PADDING * scale;
    let right_padding = left_padding
        + if reserve_chevron {
            SELECT_CHEVRON_RESERVED_WIDTH * scale
        } else {
            0.0
        };
    let available_width = (content_width - left_padding - right_padding).max(0.0);
    let text_width = option_text_width(option, scale).min(available_width);

    match select_text_align(node) {
        TextAlignKeyword::Center | TextAlignKeyword::MozCenter => {
            content_x + left_padding + ((available_width - text_width) / 2.0).max(0.0)
        }
        TextAlignKeyword::Right | TextAlignKeyword::End | TextAlignKeyword::MozRight => {
            content_x + content_width - right_padding - text_width
        }
        _ => content_x + left_padding,
    }
}

fn option_fill_color(
    dom: &BaseDocument,
    node: &Node,
    focused: bool,
    selected: bool,
    active: bool,
) -> Option<Color> {
    if !selected && !active {
        return None;
    }

    let background = resolved_popup_background_color(dom, node);
    let highlight = resolved_option_highlight_color(node, background);
    let emphasis = match (focused, selected, active) {
        (true, true, true) => 0.48,
        (true, true, false) => 0.42,
        (_, true, true) => 0.38,
        (_, true, false) => 0.32,
        (_, false, true) => 0.24,
        _ => 0.0,
    };

    Some(blend_colors(background, highlight, emphasis))
}

fn draw_option_label(
    scene: &mut impl PaintScene,
    dom: &blitz_dom::BaseDocument,
    option: &blitz_dom::node::SelectOption,
    transform: Affine,
    x: f64,
    y: f64,
) {
    let transform = Affine::translate((x, y)) * transform;
    stroke_text(scene, option.layout.lines(), dom, transform);
}

fn draw_select_chevron(
    scene: &mut impl PaintScene,
    transform: Affine,
    color: Color,
    x: f64,
    y: f64,
    scale: f64,
) {
    let width = 4.0 * scale;
    let height = 2.0 * scale;
    let mut path = BezPath::new();
    path.move_to((x - width, y - height));
    path.line_to((x, y + height));
    path.line_to((x + width, y - height));

    scene.stroke(
        &Stroke::new((1.5 * scale).max(1.0)),
        transform,
        color,
        None,
        &path,
    );
}

fn select_text_align(node: &Node) -> TextAlignKeyword {
    node.primary_styles()
        .map(|style| style.clone_text_align())
        .unwrap_or(TextAlignKeyword::Start)
}

fn resolved_select_foreground_color(node: &Node) -> Color {
    node.primary_styles()
        .map(|style| style.get_inherited_text().color.as_color_color())
        .unwrap_or_else(|| Color::from_rgba8(80, 80, 80, 255))
}

fn resolved_select_border_color(node: &Node) -> Color {
    node.primary_styles()
        .map(|style| {
            let current_color = style.clone_color();
            style
                .get_border()
                .border_top_color
                .resolve_to_absolute(&current_color)
                .as_srgb_color()
        })
        .filter(|color| *color != Color::TRANSPARENT)
        .unwrap_or_else(|| Color::from_rgba8(120, 120, 120, 255))
}

fn resolved_option_highlight_color(node: &Node, background: Color) -> Color {
    let border = resolved_select_border_color(node);
    if color_distance(border, background) >= 0.18 {
        border
    } else {
        resolved_select_foreground_color(node)
    }
}

fn resolved_popup_background_color(dom: &BaseDocument, node: &Node) -> Color {
    node.primary_styles()
        .map(|style| {
            let current_color = style.clone_color();
            style
                .get_background()
                .background_color
                .resolve_to_absolute(&current_color)
                .as_srgb_color()
        })
        .filter(|color| *color != Color::TRANSPARENT)
        .or_else(|| document_background_color(dom))
        .unwrap_or(Color::WHITE)
}

fn blend_colors(background: Color, foreground: Color, amount: f32) -> Color {
    let [bg_r, bg_g, bg_b, bg_a] = background.components;
    let [fg_r, fg_g, fg_b, fg_a] = foreground.components;
    let amount = (amount * fg_a).clamp(0.0, 1.0);

    Color::new([
        bg_r + (fg_r - bg_r) * amount,
        bg_g + (fg_g - bg_g) * amount,
        bg_b + (fg_b - bg_b) * amount,
        bg_a + (1.0 - bg_a) * amount,
    ])
}

fn color_distance(a: Color, b: Color) -> f32 {
    let [a_r, a_g, a_b, _] = a.components;
    let [b_r, b_g, b_b, _] = b.components;
    ((a_r - b_r).abs() + (a_g - b_g).abs() + (a_b - b_b).abs()) / 3.0
}

fn document_background_color(dom: &BaseDocument) -> Option<Color> {
    let root_element = dom.root_element();
    let html_styles = root_element.primary_styles()?;
    let html_background = {
        let current_color = html_styles.clone_color();
        html_styles
            .get_background()
            .background_color
            .resolve_to_absolute(&current_color)
            .as_srgb_color()
    };
    if html_background != Color::TRANSPARENT {
        return Some(html_background);
    }

    root_element
        .children
        .iter()
        .find_map(|id| {
            dom.get_node(*id)
                .filter(|node| node.data.is_element_with_tag_name(&local_name!("body")))
        })
        .and_then(|body| {
            let style = body.primary_styles()?;
            let current_color = style.clone_color();
            let color = style
                .get_background()
                .background_color
                .resolve_to_absolute(&current_color)
                .as_srgb_color();
            (color != Color::TRANSPARENT).then_some(color)
        })
}
