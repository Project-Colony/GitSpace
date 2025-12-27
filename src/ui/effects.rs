use egui::{Color32, Context, Id, Rgba, Stroke};

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t.clamp(0.0, 1.0)
}

pub fn lerp_color(a: Color32, b: Color32, t: f32) -> Color32 {
    let a = Rgba::from(a);
    let b = Rgba::from(b);
    Rgba::from_rgba_premultiplied(
        lerp(a.r(), b.r(), t),
        lerp(a.g(), b.g(), t),
        lerp(a.b(), b.b(), t),
        lerp(a.a(), b.a(), t),
    )
    .into()
}

pub fn animated_scalar(
    ctx: &Context,
    id: Id,
    base: f32,
    hover: f32,
    active: f32,
    hovered: bool,
    active_state: bool,
) -> f32 {
    let hover_t = ctx.animate_bool(id.with("hover"), hovered);
    let active_t = ctx.animate_bool(id.with("active"), active_state);
    let base_to_hover = lerp(base, hover, hover_t);
    lerp(base_to_hover, active, active_t)
}

pub fn animated_color(
    ctx: &Context,
    id: Id,
    base: Color32,
    hover: Color32,
    active: Color32,
    hovered: bool,
    active_state: bool,
) -> Color32 {
    let hover_t = ctx.animate_bool(id.with("hover"), hovered);
    let active_t = ctx.animate_bool(id.with("active"), active_state);
    let base_to_hover = lerp_color(base, hover, hover_t);
    lerp_color(base_to_hover, active, active_t)
}

pub fn animated_stroke(
    ctx: &Context,
    id: Id,
    base: Stroke,
    hover: Stroke,
    active: Stroke,
    hovered: bool,
    active_state: bool,
) -> Stroke {
    let width = animated_scalar(
        ctx,
        id.with("width"),
        base.width,
        hover.width,
        active.width,
        hovered,
        active_state,
    );
    let color = animated_color(
        ctx,
        id.with("color"),
        base.color,
        hover.color,
        active.color,
        hovered,
        active_state,
    );
    Stroke::new(width, color)
}
