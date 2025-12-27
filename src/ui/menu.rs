use eframe::egui::style::WidgetVisuals;
use eframe::egui::{
    self, AboveOrBelow, Color32, CursorIcon, Id, Pos2, Rect, Response, Rounding, Sense, TextStyle,
    Ui, Vec2, WidgetText,
};

use crate::ui::animation::{AnimationEffects, AnimationIntent, motion_settings};
use crate::ui::theme::Theme;

fn lerp_color(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let lerp_channel = |start: u8, end: u8| -> u8 {
        let start = f32::from(start);
        let end = f32::from(end);
        (start + (end - start) * t).round().clamp(0.0, 255.0) as u8
    };

    Color32::from_rgba_unmultiplied(
        lerp_channel(a.r(), b.r()),
        lerp_channel(a.g(), b.g()),
        lerp_channel(a.b(), b.b()),
        lerp_channel(a.a(), b.a()),
    )
}

fn with_alpha(color: Color32, alpha: f32) -> Color32 {
    let alpha = (alpha.clamp(0.0, 1.0) * 255.0).round() as u8;
    Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), alpha)
}

fn menu_animation_progress(ui: &Ui, id: Id, intent: AnimationIntent, target: bool) -> f32 {
    let motion = motion_settings(ui.ctx());
    let timing = motion.timing(intent);
    let duration = timing.duration.as_secs_f32();
    ui.ctx().animate_bool_with_time(id, target, duration)
}

pub fn with_menu_popup_motion<R>(
    ui: &mut Ui,
    id_source: impl std::hash::Hash,
    add_contents: impl FnOnce(&mut Ui) -> R,
) -> R {
    let id = ui.make_persistent_id(id_source);
    let progress = menu_animation_progress(ui, id, AnimationIntent::OpenClose, true);
    if progress < 1.0 {
        ui.ctx().request_repaint();
    }

    let fade = AnimationEffects::fade_in();
    let slide = AnimationEffects::slide_up(8.0);
    let scale = AnimationEffects::scale_in();
    let opacity = egui::lerp(fade.from_opacity..=fade.to_opacity, progress);
    ui.set_opacity(opacity);

    let inner = add_contents(ui);
    let rect = ui.min_rect();
    let offset = Vec2::new(
        egui::lerp(slide.from_offset[0]..=slide.to_offset[0], progress),
        egui::lerp(slide.from_offset[1]..=slide.to_offset[1], progress),
    );
    let scale_factor = egui::lerp(scale.from_scale..=scale.to_scale, progress);
    let anchor = rect.center().to_vec2() * (1.0 - scale_factor);
    ui.ctx().transform_layer_shapes(
        ui.layer_id(),
        egui::emath::TSTransform::new(offset + anchor, scale_factor),
    );
    inner
}

pub fn menu_item(
    ui: &mut Ui,
    theme: &Theme,
    id_source: impl std::hash::Hash,
    label: impl Into<WidgetText>,
    selected: bool,
) -> Response {
    let size = Vec2::new(ui.available_width(), ui.spacing().interact_size.y.max(28.0));
    menu_item_sized(ui, theme, id_source, label, selected, size, Sense::click())
}

pub fn menu_item_sized(
    ui: &mut Ui,
    theme: &Theme,
    id_source: impl std::hash::Hash,
    label: impl Into<WidgetText>,
    selected: bool,
    size: Vec2,
    sense: Sense,
) -> Response {
    let id = ui.make_persistent_id(id_source);
    let (rect, response) = ui.allocate_exact_size(size, sense);
    let response = response.on_hover_cursor(CursorIcon::PointingHand);
    let hover_t = menu_animation_progress(
        ui,
        id.with("hover"),
        AnimationIntent::Hover,
        response.hovered(),
    );
    let focus_t = menu_animation_progress(
        ui,
        id.with("focus"),
        AnimationIntent::Focus,
        response.has_focus(),
    );
    let selected_t =
        menu_animation_progress(ui, id.with("selected"), AnimationIntent::Focus, selected);
    let active_t = hover_t.max(focus_t).max(selected_t);

    let base_background = theme.palette.surface_highlight;
    let background_alpha = 0.1 * hover_t + 0.18 * selected_t + 0.08 * focus_t;
    if background_alpha > 0.0 {
        let fill = with_alpha(base_background, background_alpha);
        ui.painter().rect_filled(rect, Rounding::same(6.0), fill);
    }

    let glow = AnimationEffects::subtle_glow();
    let glow_alpha = glow.intensity * active_t;
    if glow_alpha > 0.01 {
        let glow_color = with_alpha(theme.palette.accent, glow_alpha);
        let glow_rect = rect.expand(glow.radius * active_t);
        ui.painter()
            .rect_stroke(glow_rect, Rounding::same(8.0), (1.0, glow_color));
    }

    let icon_scale = egui::lerp(0.6..=1.0, active_t);
    let icon_radius = 3.5 * icon_scale;
    let icon_center = Pos2::new(rect.left() + 10.0, rect.center().y);
    let icon_color = with_alpha(theme.palette.accent, 0.35 + 0.55 * active_t);
    ui.painter()
        .circle_filled(icon_center, icon_radius, icon_color);

    let text = label.into();
    let galley = text.into_galley(ui, Some(false), f32::INFINITY, TextStyle::Button);
    let base_text = if selected {
        theme.palette.text_primary
    } else {
        theme.palette.text_secondary
    };
    let text_color = lerp_color(
        base_text,
        theme.palette.accent,
        active_t * 0.5 + selected_t * 0.5,
    );
    let text_pos = Pos2::new(
        rect.left() + icon_radius * 2.0 + 16.0,
        rect.center().y - galley.size().y * 0.5,
    );
    ui.painter()
        .galley_with_override_text_color(text_pos, galley, text_color);

    response
}

pub fn combo_icon(
    theme: Theme,
    id: Id,
) -> impl FnOnce(&Ui, Rect, &WidgetVisuals, bool, AboveOrBelow) + 'static {
    move |ui, rect, visuals, is_open, above_or_below| {
        let hover = ui.rect_contains_pointer(rect);
        let hover_t = menu_animation_progress(ui, id.with("hover"), AnimationIntent::Hover, hover);
        let open_t =
            menu_animation_progress(ui, id.with("open"), AnimationIntent::OpenClose, is_open);
        let active_t = hover_t.max(open_t);
        let icon_color = lerp_color(visuals.fg_stroke.color, theme.palette.accent, active_t);
        let scale = egui::lerp(0.75..=1.0, active_t);
        let icon_rect = Rect::from_center_size(rect.center(), rect.size() * scale);
        let points = match above_or_below {
            AboveOrBelow::Above => vec![
                icon_rect.left_bottom(),
                icon_rect.right_bottom(),
                icon_rect.center_top(),
            ],
            AboveOrBelow::Below => vec![
                icon_rect.left_top(),
                icon_rect.right_top(),
                icon_rect.center_bottom(),
            ],
        };
        ui.painter().add(egui::Shape::convex_polygon(
            points,
            icon_color,
            egui::Stroke::new(1.0, icon_color),
        ));
    }
}
