use std::{io::Cursor, time::Duration};

use eframe::egui;

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

pub(crate) fn paint(ui: &egui::Ui, github_user_id: Option<&str>, name: &str, rect: egui::Rect) {
    if !ui.is_rect_visible(rect) {
        return;
    }
    if let Some(texture) = github_user_id.and_then(|id| texture(ui.ctx(), id)) {
        egui::Image::new((texture.id(), rect.size()))
            .corner_radius(255)
            .paint_at(ui, rect);
    } else {
        let initials: String = name
            .trim_start_matches('@')
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_avatar_ids_never_start_a_download() {
        let ctx = egui::Context::default();
        for id in ["", "0", "-1", "octocat", "42/path", "18446744073709551616"] {
            assert!(texture(&ctx, id).is_none());
        }
        assert!(
            ctx.data(|data| data
                .get_temp::<Option<egui::TextureHandle>>(egui::Id::new(("github-avatar", 0_u64))))
                .is_none()
        );
    }
}
