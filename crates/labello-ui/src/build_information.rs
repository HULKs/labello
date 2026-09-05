use std::{future::Future, pin::Pin, rc::Rc};

use eframe::egui::{self, RichText};
use labello_client::BuildIdentity;

use crate::{
    app::{AppView, LabelloApp, PendingTransition, SetupSection, UiCommand, UiMessage},
    theme,
};

pub type BuildClipboardWriter = Rc<dyn Fn(String) -> Pin<Box<dyn Future<Output = Result<(), ()>>>>>;

#[derive(Default)]
pub(crate) struct BuildInformationState {
    pub web: BuildIdentity,
    pub server: Option<BuildIdentity>,
    pub checked: bool,
    pub loading: bool,
    pub clipboard: Option<BuildClipboardWriter>,
    pub copying: bool,
    pub copy_feedback: Option<&'static str>,
}

impl LabelloApp {
    /// Set only from metadata compiled into the executing browser artifact.
    pub fn set_web_build_metadata(
        &mut self,
        release_tag: Option<&str>,
        source_commit: Option<&str>,
    ) {
        self.builds.web = BuildIdentity::from_metadata(release_tag, source_commit);
    }

    pub fn set_build_clipboard_writer(&mut self, writer: BuildClipboardWriter) {
        self.builds.clipboard = Some(writer);
    }

    /// The browser adapter calls this notifier when a visible tab regains focus.
    pub fn build_refresh_notifier(&self, ctx: egui::Context) -> Rc<dyn Fn()> {
        let tx = self.runtime.tx.clone();
        Rc::new(move || {
            let _ = tx.send(UiMessage::BuildRefreshRequested);
            ctx.request_repaint();
        })
    }

    pub(crate) fn request_build_information(&mut self) {
        if self.builds.loading {
            return;
        }
        self.builds.server = None;
        self.builds.copy_feedback = None;
        self.builds.checked = true;
        if self.runtime.api.is_none() {
            return;
        }
        self.builds.loading = true;
        let request = self.request_identity(None);
        self.queue_command(UiCommand::BuildInformation { request });
    }

    pub(crate) fn reduce_build_message(&mut self, message: UiMessage) -> Option<UiMessage> {
        match message {
            UiMessage::BuildRefreshRequested => self.request_build_information(),
            UiMessage::BuildInformationLoaded { result, .. } => {
                self.builds.loading = false;
                self.builds.checked = true;
                self.builds.server = result.ok().filter(BuildIdentity::is_valid);
            }
            UiMessage::BuildInformationCopied { succeeded, .. } => {
                self.builds.copying = false;
                self.builds.copy_feedback = Some(if succeeded {
                    "Build information copied."
                } else {
                    "Copy failed. Select the build information below and copy it manually."
                });
            }
            message => return Some(message),
        }
        None
    }

    pub(crate) fn builds_differ(&self) -> bool {
        self.builds
            .server
            .as_ref()
            .is_some_and(|server| self.builds.web.differs_from(server))
    }

    pub(crate) fn build_information_text(&self) -> String {
        let server = if self.builds.loading {
            "Server: loading".into()
        } else {
            self.builds
                .server
                .as_ref()
                .map(|identity| identity_text("Server", identity))
                .unwrap_or_else(|| "Server: unavailable".into())
        };
        format!("{}\n{server}", identity_text("Web app", &self.builds.web))
    }

    pub(crate) fn copy_build_information(&mut self) {
        if self.builds.copying {
            return;
        }
        self.builds.copy_feedback = None;
        let Some(writer) = self.builds.clipboard.clone() else {
            self.builds.copy_feedback =
                Some("Copy unavailable. Select the build information below and copy it manually.");
            return;
        };
        let text = self.build_information_text();
        let request = self.request_identity(None);
        self.runtime.active_requests.insert(request.request_id);
        self.builds.copying = true;
        // Invoke while handling the activation so the browser retains user activation.
        let operation = writer(text);
        self.spawn_message(request.clone(), async move {
            UiMessage::BuildInformationCopied {
                request,
                succeeded: operation.await.is_ok(),
            }
        });
    }

    pub(crate) fn about_section(&mut self, ui: &mut egui::Ui) {
        ui.heading("About Labello");
        ui.label("Image annotation for bounding boxes and skeleton keypoints.");
        ui.add_space(theme::SPACE_3);
        identity_row(ui, "Web app", &self.builds.web);
        ui.add_space(theme::SPACE_2);
        if self.builds.loading {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Server: loading");
            });
        } else if let Some(server) = &self.builds.server {
            identity_row(ui, "Server", server);
        } else {
            ui.label("Server: unavailable");
        }
        if self.builds_differ() {
            ui.label(RichText::new("Web app and server builds differ.").color(theme::WARNING));
        }
        ui.add_space(theme::SPACE_2);
        ui.horizontal_wrapped(|ui| {
            let label = if self.builds.server.is_some() {
                "Refresh server identity"
            } else {
                "Retry server identity"
            };
            if focus_action(theme::quiet_button(
                ui,
                !self.builds.loading,
                egui::Button::new(label).min_size(egui::vec2(44.0, 44.0)),
            )) {
                self.request_build_information();
            }
            if focus_action(theme::quiet_button(
                ui,
                !self.builds.copying,
                egui::Button::new("Copy build information").min_size(egui::vec2(44.0, 44.0)),
            )) {
                self.copy_build_information();
            }
        });
        if let Some(feedback) = self.builds.copy_feedback {
            let response = ui.label(feedback);
            ui.ctx().accesskit_node_builder(response.id, |node| {
                node.set_live(egui::accesskit::Live::Polite)
            });
        }
        ui.add_space(theme::SPACE_2);
        let label = ui.label("Build information for manual copying");
        let text = self.build_information_text();
        let mut selectable = text.as_str();
        ui.add(
            egui::TextEdit::multiline(&mut selectable)
                .font(egui::TextStyle::Monospace)
                .desired_rows(5)
                .desired_width(f32::INFINITY),
        )
        .labelled_by(label.id);
    }

    pub(crate) fn open_about(&mut self) {
        self.open_view(AppView::Setup);
        if self.work.pending_transition == Some(PendingTransition::View(AppView::Setup)) {
            self.work.pending_transition = Some(PendingTransition::About);
        } else if self.view == AppView::Setup {
            self.setup.section = SetupSection::About;
            self.request_build_information();
        }
    }

    pub(crate) fn build_warning_bar(&mut self, ui: &mut egui::Ui) {
        let mismatch = self.builds_differ();
        egui::Panel::bottom("workspace_build_status")
            .exact_size(if mismatch { 44.0 } else { 0.0 })
            .frame(egui::Frame::NONE)
            .show(ui, |ui| {
                if !mismatch {
                    return;
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let response = theme::quiet_button(
                        ui,
                        true,
                        egui::Button::new(
                            RichText::new("⚠ Web app and server builds differ")
                                .color(theme::WARNING)
                                .size(12.0),
                        )
                        .min_size(egui::vec2(44.0, 44.0)),
                    )
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .on_hover_text("Open Setup > About to inspect build information.");
                    response.widget_info(|| {
                        egui::WidgetInfo::labeled(
                            egui::WidgetType::Button,
                            true,
                            "Web app and server builds differ; open About",
                        )
                    });
                    if focus_action(response) {
                        self.open_about();
                    }
                });
            });
    }
}

fn identity_text(name: &str, identity: &BuildIdentity) -> String {
    format!(
        "{name}: {}; source commit: {}",
        identity
            .release_tag
            .as_deref()
            .unwrap_or("development (release tag unavailable)"),
        identity.source_commit.as_deref().unwrap_or("unavailable")
    )
}

fn identity_row(ui: &mut egui::Ui, name: &str, identity: &BuildIdentity) {
    ui.label(RichText::new(name).strong());
    let tag = identity
        .release_tag
        .as_deref()
        .unwrap_or("development (release tag unavailable)");
    let commit = identity
        .source_commit
        .as_deref()
        .map(|value| &value[..12])
        .unwrap_or("unavailable");
    let full = identity_text(name, identity);
    let response = ui
        .label(RichText::new(format!("{tag} · {commit}")).monospace())
        .on_hover_text(&full);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, full.clone()));
}

fn focus_action(response: egui::Response) -> bool {
    if response.gained_focus() {
        response.scroll_to_me(Some(egui::Align::Center));
    }
    response.clicked()
}
