use std::{io::Cursor, time::Duration};

use eframe::egui::{self, RichText};
use labello_domain::ContributorStats;

use crate::theme;

fn texture(ctx: &egui::Context, github_id: &str) -> Option<egui::TextureHandle> {
    let github_id = github_id.parse::<u64>().ok().filter(|id| *id > 0)?;
    let key = egui::Id::new(("github-avatar", github_id));
    if let Some(cached) = ctx.data_mut(|data| data.get_temp::<Option<egui::TextureHandle>>(key)) {
        return cached;
    }
    // Cache pending and failed requests too; redraws must not retry downloads.
    ctx.data_mut(|data| data.insert_temp(key, None::<egui::TextureHandle>));
    let request = ehttp::Request::get(format!(
        "https://avatars.githubusercontent.com/u/{github_id}?s=96"
    ))
    .with_timeout(Some(Duration::from_secs(10)));
    #[cfg(target_arch = "wasm32")]
    let request = request.with_credentials(ehttp::Credentials::Omit);
    let ctx = ctx.clone();
    ehttp::fetch(request, move |response| {
        let image = response
            .ok()
            .filter(|r| r.ok && r.bytes.len() <= 256 * 1024)
            .and_then(|response| {
                let mut reader = image::ImageReader::new(Cursor::new(response.bytes))
                    .with_guessed_format()
                    .ok()?;
                let mut limits = image::Limits::default();
                limits.max_image_width = Some(256);
                limits.max_image_height = Some(256);
                limits.max_alloc = Some(1024 * 1024);
                reader.limits(limits);
                let image = reader.decode().ok()?.to_rgba8();
                Some(egui::ColorImage::from_rgba_unmultiplied(
                    [image.width() as usize, image.height() as usize],
                    image.as_raw(),
                ))
            });
        let texture =
            image.map(|image| ctx.load_texture("github-avatar", image, Default::default()));
        ctx.data_mut(|data| data.insert_temp(key, texture));
        ctx.request_repaint();
    });
    None
}

pub(super) fn paint(ui: &egui::Ui, person: &ContributorStats, rect: egui::Rect) {
    if !ui.is_rect_visible(rect) {
        return;
    }
    if let Some(texture) = person
        .github_user_id
        .as_deref()
        .and_then(|id| texture(ui.ctx(), id))
    {
        egui::Image::new((texture.id(), rect.size()))
            .corner_radius(255)
            .paint_at(ui, rect);
    } else {
        let initials: String = person
            .display_name
            .split_whitespace()
            .filter_map(|part| part.chars().next())
            .take(2)
            .flat_map(char::to_uppercase)
            .collect();
        ui.painter()
            .circle_filled(rect.center(), rect.width() / 2.0, theme::INPUT_BG);
        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            initials,
            egui::FontId::proportional(rect.height() * 0.42),
            theme::TEXT_MUTED,
        );
    }
}

pub(super) fn person(
    ui: &mut egui::Ui,
    person: &ContributorStats,
    text: RichText,
    size: egui::Vec2,
    selected: Option<bool>,
) -> egui::Response {
    let avatar_id = ui.id().with("avatar");
    let avatar = egui::Atom::custom(avatar_id, egui::Vec2::splat(24.0));
    let response = if let Some(selected) = selected {
        egui::Button::selectable(selected, (avatar, text, egui::Atom::grow()))
            .min_size(size)
            .truncate()
            .atom_ui(ui)
    } else {
        let label = text.text().to_owned();
        let response = egui::AtomLayout::new((avatar, text))
            .min_size(size)
            .max_width(if size.x > 0.0 {
                size.x
            } else {
                ui.available_width()
            })
            .align2(egui::Align2::LEFT_CENTER)
            .wrap_mode(egui::TextWrapMode::Truncate)
            .show(ui);
        response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, &label));
        response
    };
    if let Some(rect) = response.rect(avatar_id) {
        paint(ui, person, rect);
    }
    response.response.on_hover_text(&person.display_name)
}
