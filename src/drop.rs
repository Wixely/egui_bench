use crate::theme::{CHASSIS, VALUE, legend_font};
use egui::{Align2, Color32, FontId, Id, ImageSource, Rect, Stroke, Vec2};

pub fn hovering(ctx: &egui::Context) -> Option<Vec<Option<std::path::PathBuf>>> {
    let files: Vec<_> = ctx.input(|i| i.raw.hovered_files.iter().map(|f| f.path.clone()).collect());
    (!files.is_empty()).then_some(files)
}

pub fn overlay(
    ctx: &egui::Context,
    image: Option<ImageSource<'_>>,
    headline: &str,
    note: &str,
    tint: Color32,
) {
    let screen = ctx.content_rect();
    egui::Area::new(Id::unique("egui_bench_drop"))
        .order(egui::Order::Foreground)
        .fixed_pos(screen.min)
        .interactable(false)
        .show(ctx, |ui| {
            ui.set_min_size(screen.size());
            let p = ui.painter();
            p.rect_filled(screen, 0.0, CHASSIS.gamma_multiply(0.94));
            let frame = screen.shrink(24.0);
            let corners =
                [frame.left_top(), frame.right_top(), frame.right_bottom(), frame.left_bottom()];
            for n in 0..4 {
                let edge = [corners[n], corners[(n + 1) % 4]];
                p.extend(egui::Shape::dashed_line(&edge, Stroke::new(1.5, tint), 8.0, 6.0));
            }
            let c = frame.center();
            if let Some(image) = image {
                let at = Rect::from_center_size(c - Vec2::new(0.0, 60.0), Vec2::splat(72.0));
                egui::Image::new(image).paint_at(ui, at);
            }
            let p = ui.painter();
            p.text(
                c + Vec2::new(0.0, 6.0),
                Align2::CENTER_CENTER,
                headline,
                legend_font(24.0),
                VALUE,
            );
            p.text(
                c + Vec2::new(0.0, 36.0),
                Align2::CENTER_CENTER,
                note,
                FontId::proportional(14.0),
                tint,
            );
        });
}
