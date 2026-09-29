use std::collections::BTreeMap;

use eframe::egui::{self, RichText};
use labello_domain::{ClassId, TaskId};

use crate::{
    app::{AppView, LabelloApp, LayoutMode, StatisticsOverlayState},
    theme,
};

mod overview;
pub(crate) use overview::{OverviewState, StatisticsScope};
mod avatar;
mod leaderboard;
mod streak;
pub(crate) use leaderboard::LeaderboardState;
pub use streak::set_reduced_motion;

impl LabelloApp {
    pub(crate) fn statistics_visible(&self) -> bool {
        self.navigation.statistics.open || self.view == AppView::Stats
    }

    pub(crate) fn open_statistics(&mut self) {
        if self.navigation.statistics.open {
            return;
        }
        self.navigation.statistics = StatisticsOverlayState {
            open: true,
            assignment_id: self
                .work
                .assignment
                .as_ref()
                .map(|assignment| assignment.assignment_id.clone()),
            invoker: self
                .runtime
                .repaint_ctx
                .as_ref()
                .and_then(|ctx| ctx.memory(|memory| memory.focused())),
            from_navigation: self.navigation.drawer_open,
            focus_close: true,
            restore_focus: None,
        };
        if self.datasets.metadata.is_none() {
            self.datasets.overview.scope = StatisticsScope::All;
        }
        self.request_stats();
    }

    pub(crate) fn statistics_overlay(&mut self, ctx: &egui::Context) {
        if self
            .navigation
            .statistics
            .assignment_id
            .as_ref()
            .is_some_and(|id| {
                self.work
                    .assignment
                    .as_ref()
                    .is_none_or(|assignment| &assignment.assignment_id != id)
            })
        {
            self.navigation.statistics = Default::default();
            return;
        }
        let screen = ctx.content_rect();
        let inset = if LayoutMode::for_width(screen.width()) == LayoutMode::Compact {
            40.0
        } else {
            56.0
        };
        let width = (screen.width() - inset).clamp(120.0, 1050.0);
        let max_height = (screen.height() - inset).max(160.0);
        let id = egui::Id::new("statistics-overlay");
        let resized = ctx.data_mut(|data| {
            let viewport_id = id.with("viewport");
            let previous = data.get_temp::<egui::Rect>(viewport_id);
            data.insert_temp(viewport_id, screen);
            previous.is_some_and(|previous| previous != screen)
        });
        let area = egui::Modal::default_area(id)
            .default_width(width)
            // Remeasure the anchored area and repaint immediately after a viewport change.
            .sizing_pass(resized)
            .constrain_to(screen);
        let mut close = false;
        let response = theme::modal(ctx, id).area(area).show(ctx, |ui| {
            ui.set_width(width);
            ui.set_max_height(max_height);
            ui.spacing_mut().interact_size.y = 44.0;
            let focus_close = std::mem::take(&mut self.navigation.statistics.focus_close);
            let mut close_button = |ui: &mut egui::Ui| {
                let button =
                    ui.add(egui::Button::new("Close statistics").min_size(egui::vec2(120.0, 44.0)));
                if focus_close {
                    button.request_focus();
                }
                close = button.clicked();
            };
            let header = ui.vertical(|ui| {
                if width < 260.0 {
                    ui.heading(crate::glossary::STATISTICS);
                    close_button(ui);
                } else {
                    ui.horizontal(|ui| {
                        ui.heading(crate::glossary::STATISTICS);
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            close_button(ui);
                            if width >= 520.0 {
                                self.statistics_scope_selector(ui);
                            }
                        });
                    });
                }
                if width < 520.0 {
                    self.statistics_scope_selector(ui);
                }
            });
            egui::ScrollArea::vertical()
                .scroll_source(crate::pointer_input::scroll_source(ui.ctx()))
                .id_salt("statistics-overlay-scroll")
                .max_height(
                    (max_height - header.response.rect.height() - ui.spacing().item_spacing.y)
                        .max(80.0),
                )
                .show(ui, |ui| {
                    if let Some(error) = &self.runtime.error {
                        theme::inline_message(ui, theme::Intent::Warning, error);
                    }
                    self.stats_view(ui, LayoutMode::for_width(width));
                    if let Some(focused) = ui
                        .memory(|memory| memory.focused())
                        .and_then(|id| ui.ctx().read_response(id))
                        && focused.gained_focus()
                        && focused.layer_id == ui.layer_id()
                        && ui.min_rect().contains_rect(focused.rect)
                    {
                        focused.scroll_to_me(None);
                    }
                });
        });
        response.response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Window, true, "Dataset statistics")
        });
        ctx.accesskit_node_builder(response.response.id, |node| node.set_modal());
        if close || response.should_close() {
            self.navigation.statistics.open = false;
            self.navigation.statistics.restore_focus = self.navigation.statistics.invoker;
            if self.navigation.statistics.from_navigation
                && LayoutMode::for_width(screen.width()) != LayoutMode::Wide
            {
                self.navigation.drawer_open = true;
            }
            ctx.request_repaint();
        }
    }

    pub(crate) fn stats_view(&mut self, ui: &mut egui::Ui, layout: LayoutMode) {
        ui.spacing_mut().interact_size.y = 44.0;
        ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
        if !self.navigation.statistics.open {
            self.statistics_scope_selector(ui);
        }
        let alternate = self.datasets.overview.scope != StatisticsScope::Workspace;
        let (completed, loading, error) = if alternate {
            (
                self.datasets.overview.completed,
                self.datasets.overview.pending.is_some(),
                self.datasets.overview.error.clone(),
            )
        } else {
            (
                self.datasets.last_stats_completion,
                self.loading.stats,
                self.datasets.stats_error.clone(),
            )
        };
        let has_data = completed.is_some();
        let initial_loading = loading && !has_data;
        if initial_loading {
            theme::card_frame().show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(RichText::new("Loading statistics...").strong());
                });
                ui.label(
                    RichText::new("Fetching statistics for this selection.")
                        .color(theme::TEXT_MUTED),
                );
            });
            return;
        }
        if !has_data {
            let (title, explanation, action) = if let Some(error) = &error {
                (
                    "Statistics unavailable",
                    format!("The first statistics request failed: {error}"),
                    "Retry statistics",
                )
            } else {
                (
                    "Statistics have not loaded",
                    "Load summary and activity history for this selection.".to_string(),
                    "Load statistics",
                )
            };
            if theme::empty_state(ui, title, &explanation, Some(egui::Button::new(action))) {
                self.request_stats();
            }
            return;
        }
        if let Some(error) = &error {
            theme::inline_message(
                ui,
                theme::Intent::Warning,
                format!("Statistics may be stale. Last refresh failed: {error}"),
            );
        }
        let compact = layout == LayoutMode::Compact;
        let rows = &self.datasets.overview.rows;
        let selected = match &self.datasets.overview.scope {
            StatisticsScope::Dataset(id) => rows.iter().find(|row| &row.dataset_id == id),
            _ => None,
        };
        let aggregate = self.datasets.overview.scope == StatisticsScope::All;
        let stats = if aggregate {
            &self.datasets.overview.total
        } else if let Some(row) = selected {
            &row.stats
        } else {
            &self.datasets.stats
        };
        let tasks = selected
            .map(|row| row.tasks.as_slice())
            .unwrap_or(&self.work.tasks);
        let classes = selected
            .map(|row| row.classes.as_slice())
            .unwrap_or(&self.work.classes);
        let task_names = tasks
            .iter()
            .map(|task| (task.task_id.clone(), task.name.clone()))
            .collect::<BTreeMap<_, _>>();
        let class_names = classes
            .iter()
            .map(|class| (class.class_id.clone(), class.name.clone()))
            .collect::<BTreeMap<_, _>>();
        let imbalance = if aggregate {
            None
        } else if let Some(row) = selected {
            row.imbalance.as_ref()
        } else {
            self.datasets
                .metadata
                .as_ref()
                .and_then(|metadata| metadata.imbalance.as_ref())
        };
        let identity = selected
            .map(|row| row.dataset_id.clone())
            .unwrap_or_else(|| self.config.dataset_id.clone());
        ui.add_space(8.0);
        self.datasets.leaderboard.show(
            ui,
            stats,
            (&identity, &self.config.user_id, self.auth_epoch),
            aggregate,
        );
        ui.add_space(theme::SPACE_5);
        self.datasets.leaderboard.show_activity(ui, stats);
        ui.add_space(theme::SPACE_5);
        ui.heading(if aggregate {
            "All accessible datasets"
        } else {
            "Dataset totals"
        });
        let metrics = [
            (crate::glossary::IMAGES, stats.total_images),
            (crate::glossary::COMPLETED, stats.completed_tasks),
            (crate::glossary::PENDING, stats.pending_tasks),
            (crate::glossary::IN_PROGRESS, stats.in_progress_tasks),
            (
                crate::glossary::AWAITING_REVIEW,
                stats.awaiting_review_tasks,
            ),
            (
                crate::glossary::NEEDS_CORRECTION,
                stats.needs_correction_tasks,
            ),
        ];
        let minimum_card_width = if compact { 148.0 } else { 160.0 };
        let column_count = (((ui.available_width() + 10.0) / (minimum_card_width + 10.0)).floor()
            as usize)
            .clamp(1, 4);
        for row in metrics.chunks(column_count) {
            ui.columns(column_count, |columns| {
                for (column, (label, value)) in columns.iter_mut().zip(row) {
                    theme::metric(column, label, value.to_string());
                }
            });
        }
        ui.add_space(12.0);
        if let (Some(imbalance), Some(balance)) = (imbalance, stats.assignment_balance.as_ref()) {
            theme::card_frame().show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.heading("Assignment Balance");
                let policy = format!(
                    "Absolute completion window of {} image{}",
                    imbalance.max_difference,
                    if imbalance.max_difference == 1 { "" } else { "s" }
                );
                ui.label(format!(
                    "{}: {policy}",
                    if imbalance.enforce {
                        "Enforced"
                    } else {
                        "Configured but not enforced"
                    }
                ));
                ui.label(
                    RichText::new(
                        "Annotation balance counts submitted and completed images. Review balance counts completed images. Excluded denominator entries and disabled workflows do not participate.",
                    )
                    .color(theme::TEXT_MUTED),
                );
                ui.label(
                    RichText::new(
                        "The selected workflow is blocked when its count exceeds the least-completed enabled peer by more than the window. A gap equal to the window remains eligible.",
                    )
                    .color(theme::TEXT_MUTED),
                );
                if imbalance.enforce {
                    ui.label(format!(
                        "Currently blocked for annotation: {}",
                        task_set_summary(
                            &balance.annotation_blocked_tasks,
                            &task_names,
                        )
                    ));
                    ui.label(format!(
                        "Currently blocked for review: {}",
                        task_set_summary(&balance.review_blocked_tasks, &task_names)
                    ));
                }
            });
        }
        if aggregate {
            ui.label(format!("{} accessible datasets. Images are counted within each dataset, including copies in other datasets.", rows.len()));
            ui.label("Assignment balance and scoring focus are available when selecting an individual dataset.");
            for row in rows {
                ui.push_id(&row.dataset_id, |ui| {
                    ui.heading(format!("{} ({})", row.name, row.dataset_id));
                    let task_names = row
                        .tasks
                        .iter()
                        .map(|task| (task.task_id.clone(), task.name.clone()))
                        .collect();
                    let class_names = row
                        .classes
                        .iter()
                        .map(|class| (class.class_id.clone(), class.name.clone()))
                        .collect();
                    stats_breakdowns(ui, compact, &row.stats, &task_names, &class_names);
                });
            }
        } else {
            stats_breakdowns(ui, compact, stats, &task_names, &class_names);
        }
        theme::card_frame().show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.heading("Throughput");
            if stats.throughput.is_empty() {
                theme::empty_state(
                    ui,
                    "No recorded activity",
                    "Throughput appears after annotations are created or reviews are recorded.",
                    None,
                );
            } else {
                stats_throughput_chart(ui, &stats.throughput);
            }
        });
    }
}

fn task_set_summary(
    task_ids: &std::collections::BTreeSet<TaskId>,
    task_names: &BTreeMap<TaskId, String>,
) -> String {
    if task_ids.is_empty() {
        return "none".to_string();
    }
    task_ids
        .iter()
        .map(|task_id| {
            task_names
                .get(task_id)
                .cloned()
                .unwrap_or_else(|| task_id.to_string())
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn stats_task_grid(
    ui: &mut egui::Ui,
    rows: &BTreeMap<TaskId, labello_domain::TaskStats>,
    task_names: &BTreeMap<TaskId, String>,
) {
    egui::Grid::new("stats-task-grid")
        .num_columns(6)
        .striped(true)
        .spacing([theme::SPACE_3, theme::SPACE_1])
        .show(ui, |ui| {
            stats_name_cell(ui, crate::glossary::WORKFLOW, 180.0, true);
            for heading in [
                crate::glossary::PENDING,
                crate::glossary::IN_PROGRESS,
                crate::glossary::AWAITING_REVIEW,
                crate::glossary::NEEDS_CORRECTION,
                crate::glossary::COMPLETED,
            ] {
                stats_number_cell(ui, heading, 130.0, true);
            }
            ui.end_row();

            for (task_id, stats) in rows {
                stats_name_cell(
                    ui,
                    task_names
                        .get(task_id)
                        .map(String::as_str)
                        .unwrap_or(task_id.as_str()),
                    180.0,
                    false,
                );
                for value in [
                    stats.pending,
                    stats.in_progress,
                    stats.awaiting_review,
                    stats.needs_correction,
                    stats.completed,
                ] {
                    stats_number_cell(ui, value, 130.0, false);
                }
                ui.end_row();
            }
        });
}

fn stats_class_grid(
    ui: &mut egui::Ui,
    rows: &BTreeMap<ClassId, labello_domain::ClassStats>,
    class_names: &BTreeMap<ClassId, String>,
) {
    egui::Grid::new("stats-class-grid")
        .num_columns(3)
        .striped(true)
        .spacing([theme::SPACE_3, theme::SPACE_1])
        .show(ui, |ui| {
            stats_name_cell(ui, crate::glossary::CLASS, 220.0, true);
            stats_number_cell(ui, "Annotations", 130.0, true);
            stats_number_cell(ui, "Completed workflows", 140.0, true);
            ui.end_row();

            for (class_id, stats) in rows {
                stats_name_cell(
                    ui,
                    class_names
                        .get(class_id)
                        .map(String::as_str)
                        .unwrap_or(class_id.as_str()),
                    220.0,
                    false,
                );
                stats_number_cell(ui, stats.annotations, 130.0, false);
                stats_number_cell(ui, stats.completed_tasks, 140.0, false);
                ui.end_row();
            }
        });
}

fn stats_name_cell(ui: &mut egui::Ui, value: &str, width: f32, header: bool) {
    let text = if header {
        RichText::new(value).strong().color(theme::TEXT_MUTED)
    } else {
        RichText::new(value).strong().color(theme::TEXT)
    };
    ui.add_sized(
        [width, 44.0],
        egui::Label::new(text).truncate().halign(egui::Align::Min),
    );
}

fn stats_number_cell(ui: &mut egui::Ui, value: impl ToString, width: f32, header: bool) {
    let text = if header {
        RichText::new(value.to_string())
            .strong()
            .color(theme::TEXT_MUTED)
    } else {
        RichText::new(value.to_string())
            .monospace()
            .color(theme::TEXT)
    };
    ui.add_sized(
        [width, 44.0],
        egui::Label::new(text).truncate().halign(egui::Align::Max),
    );
}

fn stats_throughput_chart(ui: &mut egui::Ui, points: &[labello_domain::ThroughputPoint]) {
    let points = points.iter().rev().take(14).rev().collect::<Vec<_>>();
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new("Annotations").strong().color(theme::ACCENT));
        ui.label(RichText::new("Reviews").strong().color(theme::INFO));
        ui.label(
            RichText::new("Daily annotation and review activity")
                .size(theme::SUPPORTING_SIZE)
                .color(theme::TEXT_MUTED),
        );
    });
    let available_width = ui.available_width();
    egui::ScrollArea::horizontal()
        .scroll_source(crate::pointer_input::scroll_source(ui.ctx()))
        .id_salt("stats-throughput-chart-scroll")
        .show(ui, |ui| {
            let width = available_width.max(42.0 + points.len() as f32 * 48.0);
            let (rect, response) =
                ui.allocate_exact_size(egui::vec2(width, 184.0), egui::Sense::hover());
            response.widget_info(|| {
                egui::WidgetInfo::labeled(egui::WidgetType::Other, true, "Daily throughput chart")
            });

            let maximum = points
                .iter()
                .flat_map(|point| [point.annotations, point.reviews])
                .max()
                .unwrap_or(0)
                .max(1);
            let axis_width = stats_axis_width(maximum);
            let plot = egui::Rect::from_min_max(
                egui::pos2(rect.left() + axis_width, rect.top() + 8.0),
                egui::pos2(rect.right() - 8.0, rect.bottom() - 26.0),
            );
            let painter = ui.painter_at(rect);
            let font = egui::FontId::new(theme::SUPPORTING_SIZE, egui::FontFamily::Monospace);
            let tick_fractions: &[f32] = if maximum == 1 {
                &[0.0, 1.0]
            } else {
                &[0.0, 0.5, 1.0]
            };
            for &fraction in tick_fractions {
                let y = plot.bottom() - plot.height() * fraction;
                painter.line_segment(
                    [egui::pos2(plot.left(), y), egui::pos2(plot.right(), y)],
                    egui::Stroke::new(1.0, theme::BORDER),
                );
                painter.text(
                    egui::pos2(plot.left() - 6.0, y),
                    egui::Align2::RIGHT_CENTER,
                    (maximum as f32 * fraction).round() as usize,
                    font.clone(),
                    theme::TEXT_MUTED,
                );
            }

            let group_width = plot.width() / points.len() as f32;
            let bar_width = (group_width * 0.26).clamp(4.0, 18.0);
            for (index, point) in points.iter().enumerate() {
                let left = plot.left() + index as f32 * group_width;
                let center = left + group_width * 0.5;
                for (value, x, color) in [
                    (point.annotations, center - bar_width - 1.0, theme::ACCENT),
                    (point.reviews, center + 1.0, theme::INFO),
                ] {
                    if value > 0 {
                        let height = plot.height() * value as f32 / maximum as f32;
                        painter.rect_filled(
                            egui::Rect::from_min_max(
                                egui::pos2(x, plot.bottom() - height),
                                egui::pos2(x + bar_width, plot.bottom()),
                            ),
                            egui::CornerRadius::same(2),
                            color,
                        );
                    }
                }
                painter.text(
                    egui::pos2(center, plot.bottom() + 6.0),
                    egui::Align2::CENTER_TOP,
                    point.day.get(5..).unwrap_or(&point.day),
                    font.clone(),
                    theme::TEXT_MUTED,
                );

                let detail = format!(
                    "{}: {} {}, {} {}",
                    point.day,
                    point.annotations,
                    if point.annotations == 1 {
                        "annotation"
                    } else {
                        "annotations"
                    },
                    point.reviews,
                    if point.reviews == 1 {
                        "review"
                    } else {
                        "reviews"
                    }
                );
                let hit = egui::Rect::from_min_max(
                    egui::pos2(left, plot.top()),
                    egui::pos2(left + group_width, rect.bottom()),
                );
                let response = ui
                    .interact(
                        hit,
                        ui.id().with(("throughput-point", index)),
                        egui::Sense::hover(),
                    )
                    .on_hover_text(detail.clone());
                response.widget_info(move || {
                    egui::WidgetInfo::labeled(egui::WidgetType::Label, true, detail.clone())
                });
            }
        });
}

fn stats_axis_width(maximum: usize) -> f32 {
    (maximum.to_string().len() as f32 * 8.0 + 12.0).max(34.0)
}

fn stats_breakdowns(
    ui: &mut egui::Ui,
    compact: bool,
    stats: &labello_domain::DatasetStats,
    task_names: &BTreeMap<TaskId, String>,
    class_names: &BTreeMap<ClassId, String>,
) {
    theme::card_frame().show(ui, |ui| {
        ui.set_min_width(ui.available_width());
        ui.heading(crate::glossary::PER_WORKFLOW);
        let rows = &stats.per_task;
        if rows.is_empty() {
            theme::empty_state(
                ui,
                "No enabled workflows",
                "Enable a labeling workflow to collect workflow statistics.",
                None,
            );
        } else if compact {
            for (task_id, stats) in rows {
                theme::inset_frame().show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    ui.label(
                        RichText::new(
                            task_names
                                .get(task_id)
                                .map(String::as_str)
                                .unwrap_or(task_id.as_str()),
                        )
                        .strong(),
                    );
                    for (label, value) in [
                        (crate::glossary::PENDING, stats.pending),
                        (crate::glossary::IN_PROGRESS, stats.in_progress),
                        (crate::glossary::AWAITING_REVIEW, stats.awaiting_review),
                        (crate::glossary::NEEDS_CORRECTION, stats.needs_correction),
                        (crate::glossary::COMPLETED, stats.completed),
                    ] {
                        ui.label(format!("{label}: {value}"));
                    }
                });
            }
        } else {
            egui::ScrollArea::horizontal()
                .scroll_source(crate::pointer_input::scroll_source(ui.ctx()))
                .id_salt("stats_tasks_horizontal")
                .show(ui, |ui| {
                    stats_task_grid(ui, rows, task_names);
                });
        }
    });
    theme::card_frame().show(ui, |ui| {
        ui.set_min_width(ui.available_width());
        ui.heading("Per Class");
        let rows = &stats.per_class;
        if rows.is_empty() {
            theme::empty_state(
                ui,
                "No classes configured",
                "Add a class to collect class-level statistics.",
                None,
            );
        } else if compact {
            for (class_id, stats) in rows {
                theme::inset_frame().show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    ui.label(
                        RichText::new(
                            class_names
                                .get(class_id)
                                .map(String::as_str)
                                .unwrap_or(class_id.as_str()),
                        )
                        .strong(),
                    );
                    ui.label(format!(
                        "Annotations: {}  Completed workflows: {}",
                        stats.annotations, stats.completed_tasks
                    ));
                });
            }
        } else {
            egui::ScrollArea::horizontal()
                .scroll_source(crate::pointer_input::scroll_source(ui.ctx()))
                .id_salt("stats_classes_horizontal")
                .show(ui, |ui| {
                    stats_class_grid(ui, rows, class_names);
                });
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn statistics_axis_gutter_scales_with_large_values() {
        assert_eq!(stats_axis_width(1), 34.0);
        assert!(stats_axis_width(12_345) >= 52.0);
    }
}
