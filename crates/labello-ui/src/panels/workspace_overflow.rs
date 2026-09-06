pub(crate) const COMPACT_MORE_LABEL: &str = "…";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum WorkspaceCommand {
    User(labello_domain::UserAction),
    NextMigrationObject,
    RetryActivity,
}

pub(crate) struct WorkspaceAction {
    pub command: WorkspaceCommand,
    pub label: String,
    pub shortcut: String,
    pub enabled: bool,
    pub help: &'static str,
}

impl WorkspaceAction {
    fn button(&self) -> egui::Button<'_> {
        let button = egui::Button::new(self.label.as_str())
            .min_size(egui::Vec2::splat(44.0))
            .wrap_mode(egui::TextWrapMode::Extend);
        if self.shortcut.is_empty() {
            button
        } else {
            button.shortcut_text(theme::button_shortcut(self.shortcut.clone()))
        }
    }
}

#[derive(Clone, Default)]
struct WorkspaceOverflowFocus {
    inline: Vec<(WorkspaceCommand, egui::Id)>,
    menu_commands: Vec<WorkspaceCommand>,
    pending: Option<WorkspaceCommand>,
    trigger: Option<egui::Id>,
}

pub(crate) fn workspace_command_in_open_menu(
    ctx: &egui::Context,
    command: WorkspaceCommand,
) -> bool {
    let owner = egui::Id::new("workspace-secondary-actions");
    egui::Popup::is_id_open(ctx, owner.with("popup"))
        && ctx.data(|data| {
            data.get_temp::<WorkspaceOverflowFocus>(owner)
                .is_some_and(|focus| focus.menu_commands.contains(&command))
        })
}

pub(crate) fn remember_workspace_action_response(
    ui: &egui::Ui,
    command: WorkspaceCommand,
    response: &egui::Response,
) {
    let owner = egui::Id::new("workspace-secondary-actions");
    ui.ctx().data_mut(|data| {
        let mut focus = data
            .get_temp::<WorkspaceOverflowFocus>(owner)
            .unwrap_or_default();
        focus.inline.retain(|(existing, _)| *existing != command);
        focus.inline.push((command, response.id));
        data.insert_temp(owner, focus);
    });
}

// Measure the same atoms, font, frame and target as the rendered button, without
// allocating an interactive widget or estimating text from character counts.
pub(crate) fn workspace_button_size(ui: &egui::Ui, button: &egui::Button<'_>) -> egui::Vec2 {
    use egui::widget_style::{HasClasses, WidgetState};
    let style = ui
        .style()
        .button_style(button.classes(), WidgetState::Inactive);
    let measured = egui::AtomLayout::new(button.atoms().clone())
        .frame(style.frame)
        .fallback_font(style.text_style.font_id)
        .min_size(egui::Vec2::splat(44.0))
        .wrap_mode(egui::TextWrapMode::Extend)
        .measure(ui, egui::Vec2::INFINITY);
    // SizedAtomKind exposes the measured outer size of nested layouts.
    egui::SizedAtomKind::Layout(Box::new(measured)).size()
}

pub(crate) fn workspace_inline_prefix(
    widths: &[f32],
    available: f32,
    gap: f32,
    more: f32,
) -> usize {
    let total = widths.iter().sum::<f32>() + gap * widths.len().saturating_sub(1) as f32;
    if total <= available {
        return widths.len();
    }
    let mut used = more;
    widths
        .iter()
        .take_while(|width| {
            let next = used + gap + **width;
            if next <= available {
                used = next;
                true
            } else {
                false
            }
        })
        .count()
}

pub(crate) fn workspace_secondary_actions(
    ui: &mut egui::Ui,
    actions: &[WorkspaceAction],
    more_label: &str,
) -> Option<WorkspaceCommand> {
    let owner = egui::Id::new("workspace-secondary-actions");
    let popup_id = owner.with("popup");
    let mut focus = ui.ctx().data_mut(|data| {
        data.get_temp::<WorkspaceOverflowFocus>(owner)
            .unwrap_or_default()
    });
    let more_button = egui::Button::new(more_label)
        .min_size(egui::Vec2::splat(44.0))
        .wrap_mode(egui::TextWrapMode::Extend);
    let more_width = workspace_button_size(ui, &more_button).x;
    let widths: Vec<_> = actions
        .iter()
        .map(|action| workspace_button_size(ui, &action.button()).x)
        .collect();
    let available = ui.available_size_before_wrap().x;
    let total = widths.iter().sum::<f32>()
        + ui.spacing().item_spacing.x * widths.len().saturating_sub(1) as f32;
    if total > available && more_width > available {
        ui.end_row();
    }
    let prefix = workspace_inline_prefix(
        &widths,
        ui.available_size_before_wrap().x,
        ui.spacing().item_spacing.x,
        more_width,
    );
    let focused = ui.ctx().memory(|memory| memory.focused());
    let moved = focus.inline.iter().find_map(|(command, id)| {
        (Some(*id) == focused
            && actions[prefix..]
                .iter()
                .any(|action| action.command == *command))
        .then_some(*command)
    });
    if let Some(command) = moved {
        focus.pending = Some(command);
    }
    let mut clicked = None;
    let mut inline = Vec::new();
    let render = |ui: &mut egui::Ui, action: &WorkspaceAction, menu: bool| {
        ui.scope_builder(
            egui::UiBuilder::new().id(owner.with(action.command)),
            |ui| {
                let button = if menu
                    && workspace_button_size(ui, &action.button()).x > ui.available_width()
                {
                    let text = egui::AtomLayout::new((
                        egui::RichText::new(action.label.as_str()),
                        theme::button_shortcut(action.shortcut.clone()),
                    ))
                    .direction(egui::Direction::TopDown)
                    .gap(4.0)
                    .wrap_mode(egui::TextWrapMode::Wrap);
                    egui::Button::new(egui::Atom::layout(text))
                        .min_size(egui::Vec2::splat(44.0))
                        .wrap_mode(egui::TextWrapMode::Wrap)
                } else {
                    action.button()
                };
                let response = ui
                    .add_enabled(action.enabled, button)
                    .on_hover_text(action.help);
                response.widget_info(|| {
                    egui::WidgetInfo::labeled(
                        egui::WidgetType::Button,
                        action.enabled,
                        format!("{} {}", action.label, action.shortcut).trim(),
                    )
                });
                response
            },
        )
        .inner
    };
    for action in &actions[..prefix] {
        let response = render(ui, action, false);
        if focus.trigger == focused && focus.pending == Some(action.command) {
            response.request_focus();
            focus.pending = None;
        }
        if response.clicked() {
            clicked = Some(action.command);
        }
        inline.push((action.command, response.id));
    }
    if prefix < actions.len() {
        let response = ui
            .scope_builder(egui::UiBuilder::new().id(owner.with("trigger")), |ui| {
                ui.add(more_button)
            })
            .inner;
        if more_label == COMPACT_MORE_LABEL {
            response.clone().on_hover_text("More actions");
            response
                .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "More"));
        }
        focus.trigger = Some(response.id);
        if moved.is_some() {
            response.request_focus();
        }
        egui::Popup::menu(&response).id(popup_id).show(|ui| {
            ui.set_max_width((ui.ctx().content_rect().width() - 32.0).max(44.0));
            egui::ScrollArea::vertical()
                .max_height((ui.ctx().content_rect().height() - 32.0).max(44.0))
                .show(ui, |ui| {
                    for action in &actions[prefix..] {
                        let response = render(ui, action, true);
                        if focus.pending == Some(action.command) && action.enabled {
                            response.request_focus();
                            focus.pending = None;
                        }
                        if response.clicked() {
                            clicked = Some(action.command);
                            ui.close();
                        }
                        if response.has_focus() {
                            response.scroll_to_me(None);
                        }
                    }
                });
        });
    } else {
        egui::Popup::close_id(ui.ctx(), popup_id);
        focus.trigger = None;
    }
    focus.inline = inline;
    focus.menu_commands = actions[prefix..]
        .iter()
        .map(|action| action.command)
        .collect();
    ui.ctx().data_mut(|data| data.insert_temp(owner, focus));
    clicked
}

impl LabelloApp {
    pub(crate) fn workspace_secondary_action(
        &self,
        ctx: &egui::Context,
        action: labello_domain::UserAction,
        label: &str,
        enabled: bool,
        help: &'static str,
    ) -> WorkspaceAction {
        WorkspaceAction {
            command: WorkspaceCommand::User(action),
            label: label.into(),
            shortcut: self.shortcut_text(ctx, action),
            enabled,
            help,
        }
    }

    pub(crate) fn dispatch_workspace_secondary(&mut self, command: Option<WorkspaceCommand>) {
        match command {
            Some(WorkspaceCommand::User(action)) => self.trigger_user_action(action),
            Some(WorkspaceCommand::NextMigrationObject) => self.inspect_migration_object(1),
            Some(WorkspaceCommand::RetryActivity) => self.request_activity(),
            None => {}
        }
    }
}
