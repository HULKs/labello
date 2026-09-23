use eframe::egui::{self, RichText};
use labello_domain::ContributorStats;

pub(super) fn paint(ui: &egui::Ui, person: &ContributorStats, rect: egui::Rect) {
    crate::avatar::paint(
        ui,
        person.github_user_id.as_deref(),
        &person.display_name,
        rect,
    );
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
