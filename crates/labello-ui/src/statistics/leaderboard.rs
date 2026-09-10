use std::{
    cmp::Ordering,
    collections::{BTreeMap, BTreeSet},
};

use chrono::{Datelike, Days, Months, NaiveDate};
use eframe::egui::{self, RichText};
use labello_domain::{ContributorDay, ContributorStats, DatasetId, DatasetStats, UserId};

use super::avatar;
use crate::theme;

const PERIODS: [&str; 6] = [
    "Last day",
    "Last week",
    "Last month",
    "Last 3 months",
    "Last year",
    "Overall",
];
const METRICS: [&str; 4] = ["Labeled", "Reviewed", "Acceptance", "Score"];
const METRIC_WIDTHS: [f32; 4] = [100.0, 110.0, 160.0, 90.0];
const PODIUM_TITLES: [&str; 4] = [
    "Most labeled",
    "Most reviewed",
    "Highest acceptance",
    "Highest score",
];
const PODIUM_BLUES: [egui::Color32; 3] = [
    egui::Color32::from_rgb(59, 130, 246),
    theme::INFO,
    egui::Color32::from_rgb(147, 197, 253),
];
const COLORS: [egui::Color32; 5] = [
    theme::ACCENT,
    theme::INFO,
    theme::WARNING,
    theme::PRELABEL,
    theme::DANGER,
];

pub(crate) struct LeaderboardState {
    identity: Option<(DatasetId, UserId, u64)>,
    period: usize,
    history: bool,
    metric: usize,
    sort_by_name: bool,
    sort_descending: bool,
    ranking_metric: usize,
    selected: Option<Vec<UserId>>,
    people_filter: String,
    activity_user: Option<UserId>,
    activity_day: Option<NaiveDate>,
}

impl Default for LeaderboardState {
    fn default() -> Self {
        Self {
            identity: None,
            period: 5,
            history: false,
            metric: 3,
            sort_by_name: false,
            sort_descending: true,
            ranking_metric: 3,
            selected: None,
            people_filter: String::new(),
            activity_user: None,
            activity_day: None,
        }
    }
}

fn period_start(period: usize, today: NaiveDate) -> NaiveDate {
    match period {
        0 => today,
        1 => today - Days::new(6),
        2 => today - Months::new(1) + Days::new(1),
        3 => today - Months::new(3) + Days::new(1),
        4 => today - Months::new(12) + Days::new(1),
        _ => NaiveDate::MIN,
    }
}

fn add(total: &mut ContributorDay, day: &ContributorDay) {
    total.score.add(&day.score);
    total.labeled += day.labeled;
    total.reviewed += day.reviewed;
    total.accepted += day.accepted;
    total.rejected += day.rejected;
}

fn value(day: &ContributorDay, metric: usize) -> Option<f64> {
    match metric {
        3 => Some(labello_domain::displayed_score(day.score.total()) as f64),
        0 => Some(day.labeled as f64),
        1 => Some(day.reviewed as f64),
        _ => (day.accepted + day.rejected > 0)
            .then(|| 100.0 * day.accepted as f64 / (day.accepted + day.rejected) as f64),
    }
}

fn display(day: &ContributorDay, metric: usize) -> String {
    match metric {
        3 => labello_domain::displayed_score(day.score.total()).to_string(),
        0 => day.labeled.to_string(),
        1 => day.reviewed.to_string(),
        _ => value(day, metric).map_or_else(
            || "—".into(),
            |rate| {
                format!(
                    "{rate:.1}% · {}/{}",
                    day.accepted,
                    day.accepted + day.rejected
                )
            },
        ),
    }
}

fn compare(a: &ContributorDay, b: &ContributorDay, metric: usize) -> Ordering {
    match metric {
        3 => a.score.total().cmp(&b.score.total()),
        0 => a.labeled.cmp(&b.labeled),
        1 => a.reviewed.cmp(&b.reviewed),
        _ => {
            let a_total = a.accepted + a.rejected;
            let b_total = b.accepted + b.rejected;
            (a_total > 0).cmp(&(b_total > 0)).then_with(|| {
                ((a.accepted as u128) * (b_total as u128))
                    .cmp(&((b.accepted as u128) * (a_total as u128)))
            })
        }
    }
}

struct Row<'a> {
    id: &'a UserId,
    name: &'a str,
    person: &'a ContributorStats,
    total: ContributorDay,
}

fn sorted<'a>(rows: &'a [Row<'a>], metric: usize) -> Vec<(usize, &'a Row<'a>)> {
    let mut rows: Vec<_> = rows.iter().collect();
    rows.sort_by(|a, b| {
        compare(&b.total, &a.total, metric)
            .then_with(|| {
                if metric == 2 {
                    (b.total.accepted + b.total.rejected)
                        .cmp(&(a.total.accepted + a.total.rejected))
                } else {
                    Ordering::Equal
                }
            })
            .then_with(|| a.id.cmp(b.id))
    });
    let mut rank = 1;
    rows.iter()
        .enumerate()
        .map(|(index, row)| {
            if index > 0 && compare(&rows[index - 1].total, &row.total, metric) != Ordering::Equal {
                rank = index + 1;
            }
            (rank, *row)
        })
        .collect()
}

impl LeaderboardState {
    fn sort_header(&mut self, ui: &mut egui::Ui, metric: Option<usize>, label: &str, width: f32) {
        let mut active = self.sort_by_name == metric.is_none()
            && metric.is_none_or(|metric| self.ranking_metric == metric);
        let arrow = if !active {
            "↕"
        } else if self.sort_descending {
            "▼"
        } else {
            "▲"
        };
        let button = egui::Button::new(format!("{label} {arrow}")).frame(false);
        let response = if width > 0.0 {
            ui.add_sized([width, 44.0], button.truncate())
        } else {
            ui.add(button.min_size(egui::vec2(44.0, 44.0)))
        };
        if response.clicked() {
            if active {
                self.sort_descending = !self.sort_descending;
            } else {
                self.sort_descending = metric.is_some();
            }
            self.sort_by_name = metric.is_none();
            if let Some(metric) = metric {
                self.ranking_metric = metric;
            }
            active = true;
        }
        response.widget_info(|| {
            let mut info = egui::WidgetInfo::labeled(
                egui::WidgetType::Button,
                true,
                format!("Sort by {label}"),
            );
            info.current_text_value = Some(
                if !active {
                    "Not sorted"
                } else if self.sort_descending {
                    "Descending"
                } else {
                    "Ascending"
                }
                .into(),
            );
            info
        });
    }

    fn table_rows<'a>(&self, rows: &'a [Row<'a>]) -> Vec<(usize, &'a Row<'a>)> {
        let mut ranked = sorted(rows, self.ranking_metric);
        if self.sort_by_name {
            ranked.sort_by(|(_, a), (_, b)| {
                let order = a.name.to_lowercase().cmp(&b.name.to_lowercase());
                if self.sort_descending {
                    order.reverse()
                } else {
                    order
                }
            });
        } else if !self.sort_descending {
            // Preserve tie order and keep unrated entries last when reversing scores.
            ranked.sort_by(|(a_rank, a), (b_rank, b)| {
                value(&b.total, self.ranking_metric)
                    .is_some()
                    .cmp(&value(&a.total, self.ranking_metric).is_some())
                    .then_with(|| b_rank.cmp(a_rank))
            });
        }
        ranked
    }

    fn compact_sort(&mut self, ui: &mut egui::Ui, metrics: &[usize]) {
        let previous = (!self.sort_by_name).then_some(self.ranking_metric);
        let mut selected = previous;
        let label = previous.map_or("Person", |metric| METRICS[metric]);
        let width = if ui.available_width() < 180.0 {
            ui.available_width()
        } else {
            ui.available_width() - 44.0 - ui.spacing().item_spacing.x
        };
        ui.horizontal_wrapped(|ui| {
            egui::ComboBox::from_id_salt("ranking-sort")
                .selected_text(format!("Sort: {label}"))
                .width(width)
                .truncate()
                .show_ui(ui, |ui| {
                    ui.set_min_width(width);
                    for &metric in metrics {
                        ui.selectable_value(&mut selected, Some(metric), METRICS[metric]);
                    }
                    ui.selectable_value(&mut selected, None, "Person");
                })
                .response
                .widget_info(|| {
                    let mut info = egui::WidgetInfo::labeled(
                        egui::WidgetType::ComboBox,
                        true,
                        "Sort rankings",
                    );
                    info.current_text_value = Some(label.into());
                    info
                });
            if selected != previous {
                self.sort_by_name = selected.is_none();
                self.sort_descending = selected.is_some();
                if let Some(metric) = selected {
                    self.ranking_metric = metric;
                }
            }
            let response = ui.add_sized(
                [44.0, 44.0],
                egui::Button::new(if self.sort_descending { "↓" } else { "↑" }),
            );
            if response.clicked() {
                self.sort_descending = !self.sort_descending;
            }
            response
                .on_hover_text("Reverse ranking order")
                .widget_info(|| {
                    let mut info = egui::WidgetInfo::labeled(
                        egui::WidgetType::Button,
                        true,
                        "Reverse ranking order",
                    );
                    info.current_text_value = Some(
                        if self.sort_descending {
                            "Descending"
                        } else {
                            "Ascending"
                        }
                        .into(),
                    );
                    info
                });
        });
    }

    pub(crate) fn show_activity(&mut self, ui: &mut egui::Ui, stats: &DatasetStats) {
        if let Some(contributors) = &stats.contributors {
            let today = labello_domain::now().date_naive();
            activity_chart(
                ui,
                contributors,
                &mut self.activity_user,
                &mut self.activity_day,
                &mut self.period,
                today,
            );
            ui.add_space(theme::SPACE_3);
        }
    }

    pub(crate) fn show(
        &mut self,
        ui: &mut egui::Ui,
        stats: &DatasetStats,
        identity: (&DatasetId, &UserId, u64),
    ) {
        if self.identity.as_ref().is_none_or(|(dataset, user, epoch)| {
            dataset != identity.0 || user != identity.1 || *epoch != identity.2
        }) {
            *self = Self {
                identity: Some((identity.0.clone(), identity.1.clone(), identity.2)),
                ..Default::default()
            };
        }
        ui.heading("Contributor leaderboard");
        let Some(contributors) = &stats.contributors else {
            ui.label("Contributor statistics are unavailable from this server.");
            return;
        };
        let today = labello_domain::now().date_naive();
        let scoring = stats.scoring_version == Some(1);
        if !scoring {
            ui.label("Contribution scores are unavailable from this server.");
            if self.metric == 3 {
                self.metric = 0;
            }
            if self.ranking_metric == 3 {
                self.ranking_metric = 0;
            }
        }
        let metrics: &[usize] = if scoring { &[3, 0, 1, 2] } else { &[0, 1, 2] };
        let mut daily_status = None;
        if scoring {
            let labels = contributors
                .get(identity.1)
                .and_then(|person| {
                    person
                        .history
                        .iter()
                        .find(|day| day.day == today.to_string())
                })
                .map_or(0, |day| day.score.labels);
            let next = if labels >= 500 {
                "Daily maximum reached".into()
            } else {
                format!("Next tier at {} labels", (labels / 100 + 1) * 100)
            };
            daily_status = Some(format!(
                "{labels} labels today · ×{:.2} · {next}",
                labello_domain::daily_multiplier(labels) as f64 / 100.0
            ));
        }
        ui.horizontal_wrapped(|ui| {
            ui.selectable_value(&mut self.history, false, "Leaderboard");
            ui.selectable_value(&mut self.history, true, "History graph");
        });
        period_selector(ui, &mut self.period, "Period");
        ui.horizontal_wrapped(|ui| {
            ui.small(if self.period == 5 {
                "(All recorded activity · UTC)".into()
            } else {
                format!("({} – {today} · UTC)", period_start(self.period, today))
            });
        });
        let start = period_start(self.period, today);
        let rows: Vec<_> = contributors
            .iter()
            .filter_map(|(id, contributor)| {
                let mut total = ContributorDay::default();
                for day in &contributor.history {
                    if let Ok(date) = day.day.parse::<NaiveDate>()
                        && date >= start
                        && date <= today
                    {
                        add(&mut total, day);
                    }
                }
                (total.labeled + total.reviewed + total.accepted + total.rejected > 0
                    || total.score != Default::default())
                .then_some(Row {
                    id,
                    name: &contributor.display_name,
                    person: contributor,
                    total,
                })
            })
            .collect();
        if contributors.is_empty() {
            ui.label("No contributor activity yet.");
            return;
        }
        if self.history {
            let selected = self.selected.get_or_insert_with(|| {
                sorted(&rows, self.metric)
                    .iter()
                    .take(3)
                    .map(|(_, row)| row.id.clone())
                    .collect()
            });
            selected.retain(|id| contributors.contains_key(id));
            theme::card_frame().show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.heading("History");
                if let Some(status) = &daily_status {
                    ui.label(status);
                }
                let menu_width = ui.available_width().min(240.0);
                ui.horizontal_wrapped(|ui| {
                    for &metric in metrics {
                        ui.selectable_value(&mut self.metric, metric, METRICS[metric]);
                    }
                });
                ui.horizontal_wrapped(|ui| {
                    egui::ComboBox::from_id_salt("history-people")
                        .selected_text(if selected.is_empty() {
                            "Select people".to_string()
                        } else {
                            format!("People · {} selected", selected.len())
                        })
                        .width(190.0_f32.min(menu_width))
                        .height(440.0)
                        .truncate()
                        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                        .show_ui(ui, |ui| {
                            ui.set_width(menu_width);
                            ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
                            ui.spacing_mut().interact_size.y = 44.0;
                            ui.spacing_mut().button_padding.y = 4.0;
                            ui.spacing_mut().item_spacing.y = 4.0;
                            ui.add(
                                theme::singleline_text_edit(&mut self.people_filter)
                                    .id(ui.id().with("history-search"))
                                    .hint_text("Search people")
                                    .desired_width(f32::INFINITY),
                            )
                            .widget_info(|| {
                                let mut info = egui::WidgetInfo::labeled(
                                    egui::WidgetType::TextEdit,
                                    true,
                                    "Search people",
                                );
                                info.current_text_value = Some(self.people_filter.clone());
                                info
                            });
                            let all_selected = selected.len() == contributors.len();
                            if ui.selectable_label(all_selected, "Select all").clicked() {
                                if all_selected {
                                    selected.clear();
                                } else {
                                    for id in contributors.keys() {
                                        if !selected.contains(id) {
                                            selected.push(id.clone());
                                        }
                                    }
                                }
                            }
                            ui.separator();
                            let filter = self.people_filter.trim().to_lowercase();
                            let mut matches = 0;
                            for (id, person) in contributors {
                                if !person.display_name.to_lowercase().contains(&filter) {
                                    continue;
                                }
                                matches += 1;
                                // Filtering moves rows; their widget IDs must not depend on position.
                                ui.scope_builder(
                                    egui::UiBuilder::new().id(ui.id().with(("history-person", id))),
                                    |ui| {
                                        let checked = selected.contains(id);
                                        if avatar::person(
                                            ui,
                                            person,
                                            RichText::new(&person.display_name),
                                            egui::vec2(ui.available_width(), 44.0),
                                            Some(checked),
                                        )
                                        .clicked()
                                        {
                                            if checked {
                                                selected.retain(|selected_id| selected_id != id);
                                            } else {
                                                selected.push(id.clone());
                                            }
                                        }
                                    },
                                );
                            }
                            if matches == 0 {
                                ui.label("No matching people");
                            }
                        })
                        .response
                        .widget_info(|| {
                            let mut info = egui::WidgetInfo::labeled(
                                egui::WidgetType::ComboBox,
                                true,
                                "Compare people",
                            );
                            info.current_text_value = Some(format!("{} selected", selected.len()));
                            info
                        });
                });
                ui.add_space(theme::SPACE_2);
                ui.vertical(|ui| {
                    for (index, id) in selected.iter().enumerate() {
                        avatar::person(
                            ui,
                            &contributors[id],
                            RichText::new(format!(
                                "{} · {}",
                                index + 1,
                                contributors[id].display_name
                            ))
                            .color(COLORS[index % COLORS.len()]),
                            egui::vec2(0.0, 28.0),
                            None,
                        );
                    }
                });
                history_chart(ui, contributors, selected, start, today, self.metric);
            });
        } else if rows.is_empty() {
            ui.label("No contributor activity in this period.");
        } else {
            if scoring {
                podium(ui, &rows, 3);
            }
            ui.add_space(theme::SPACE_3);
            theme::card_frame().show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.heading("Rankings");
                if let Some(status) = &daily_status {
                    ui.label(status);
                }
                if ui.available_width() >= 720.0 {
                    let name_width = ui.available_width()
                        - 76.0
                        - metrics
                            .iter()
                            .map(|&metric| METRIC_WIDTHS[metric])
                            .sum::<f32>()
                        - (metrics.len() + 1) as f32 * ui.spacing().item_spacing.x;
                    egui::Grid::new("contributor-table")
                        .num_columns(metrics.len() + 2)
                        .striped(true)
                        .show(ui, |ui| {
                            super::stats_number_cell(ui, "Rank", 76.0, true);
                            self.sort_header(ui, None, "Person", name_width);
                            for &metric in metrics {
                                self.sort_header(
                                    ui,
                                    Some(metric),
                                    METRICS[metric],
                                    METRIC_WIDTHS[metric],
                                );
                            }
                            ui.end_row();
                            for (rank, row) in self.table_rows(&rows) {
                                let rank = if value(&row.total, self.ranking_metric).is_some() {
                                    rank.to_string()
                                } else {
                                    "—".into()
                                };
                                super::stats_number_cell(ui, rank, 76.0, false);
                                avatar::person(
                                    ui,
                                    row.person,
                                    RichText::new(row.name),
                                    egui::vec2(name_width, 44.0),
                                    None,
                                );
                                for &metric in metrics {
                                    super::stats_number_cell(
                                        ui,
                                        display(&row.total, metric),
                                        METRIC_WIDTHS[metric],
                                        false,
                                    );
                                }
                                ui.end_row();
                            }
                        });
                } else {
                    self.compact_sort(ui, metrics);
                    for (rank, row) in self.table_rows(&rows) {
                        let rank = if value(&row.total, self.ranking_metric).is_some() {
                            format!("#{rank}")
                        } else {
                            "—".into()
                        };
                        ui.separator();
                        avatar::person(
                            ui,
                            row.person,
                            RichText::new(format!("{rank}  {}", row.name)).strong(),
                            egui::vec2(0.0, 28.0),
                            None,
                        )
                        .on_hover_text(row.id.as_str());
                        ui.scope(|ui| {
                            ui.spacing_mut().interact_size.y = 20.0;
                            ui.horizontal_wrapped(|ui| {
                                for &metric in metrics {
                                    let label = RichText::new(format!(
                                        "{}: {}",
                                        METRICS[metric],
                                        display(&row.total, metric),
                                    ));
                                    ui.label(if metric == 3 {
                                        label.strong()
                                    } else {
                                        label.color(theme::TEXT_MUTED)
                                    });
                                }
                            });
                        });
                    }
                }
            });
            ui.collapsing("Other highlights", |ui| {
                if ui.available_width() >= 850.0 {
                    ui.columns(3, |columns| {
                        for (metric, column) in columns.iter_mut().enumerate() {
                            podium(column, &rows, metric);
                        }
                    });
                } else {
                    for metric in 0..3 {
                        podium(ui, &rows, metric);
                        if metric < 2 {
                            ui.add_space(theme::SPACE_3);
                        }
                    }
                }
            });
        }
        ui.collapsing(if ui.available_width() < 230.0 { "Score rules" } else { "How scores are counted" }, |ui| {
            if scoring {
                ui.label("Points: one keypoint 10, each additional keypoint +5; bounding box 20. Submission earns points once per label. Manual +10%, focus workflow +25%; bonuses add together. Every 100 labels today increases subsequent label points by 10%, up to ×1.50 after 500. Days use UTC.");
                ui.label("Reviewing a label earns 30% of its base value once per reviewer. Rejection deducts 50% once per label; an accepted geometry correction earns its author 20% once. Correction never refunds the rejection. Review and correction points receive no bonuses or daily-tier progress.");
                ui.label("Score = 10 × square root of total points, rounded down. Rankings use exact points. Periods include deductions made during that period, so period scores can be negative. Historical work earns points; focus bonuses begin when scoring is activated.");
            }
            ui.label("Labeled: distinct image–task submissions per person, including empty results. Resubmissions count once. Imported and automatic work earn no labeling credit.");
            ui.label("Reviewed: review decisions made. Acceptance: approvals received / all reviews received on your work. Corrections count as rejections. Unattributable reviews do not affect acceptance.");
            ui.label("Acceptance shows accepted / reviewed counts. No reviews means no rating. Equal scores share rank; acceptance ties list larger samples first.");
        });
    }
}

fn period_selector(ui: &mut egui::Ui, period: &mut usize, label: &str) {
    let combo = if ui.available_width() < 260.0 {
        ui.label(format!("{label}:"));
        egui::ComboBox::from_id_salt(label)
    } else {
        egui::ComboBox::from_label(label)
    };
    combo
        .width(ui.available_width().min(140.0))
        .truncate()
        .selected_text(PERIODS[*period])
        .show_ui(ui, |ui| {
            for (index, label) in PERIODS.iter().enumerate() {
                ui.selectable_value(period, index, *label);
            }
        })
        .response
        .on_hover_text("UTC calendar days, including today. Last day starts at 00:00 UTC.")
        .widget_info(|| {
            let mut info = egui::WidgetInfo::labeled(egui::WidgetType::ComboBox, true, label);
            info.current_text_value = Some(PERIODS[*period].into());
            info
        });
}

fn activity_chart(
    ui: &mut egui::Ui,
    contributors: &BTreeMap<UserId, ContributorStats>,
    activity_user: &mut Option<UserId>,
    activity_day: &mut Option<NaiveDate>,
    period: &mut usize,
    today: NaiveDate,
) {
    const BLUES: [egui::Color32; 5] = [
        theme::INPUT_BG,
        egui::Color32::from_rgb(48, 114, 238),
        PODIUM_BLUES[0],
        PODIUM_BLUES[1],
        PODIUM_BLUES[2],
    ];
    if activity_user
        .as_ref()
        .is_some_and(|id| !contributors.contains_key(id))
    {
        *activity_user = None;
    }
    theme::card_frame().show(ui, |ui| {
        ui.set_min_width(ui.available_width());
        ui.heading("Daily activity");
        period_selector(ui, period, "Activity period");
        let start = period_start(*period, today);
        let menu_width = ui.available_width().min(300.0);
        let (daily, start, maximum) = ui
            .horizontal_wrapped(|ui| {
                egui::ComboBox::from_id_salt("activity-person")
                    .selected_text(
                        activity_user
                            .as_ref()
                            .map_or("All people", |id| contributors[id].display_name.as_str()),
                    )
                    .width(210.0_f32.min(menu_width))
                    .truncate()
                    .show_ui(ui, |ui| {
                        ui.set_width(menu_width);
                        ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
                        ui.selectable_value(activity_user, None, "All people");
                        for (id, person) in contributors {
                            ui.push_id(id, |ui| {
                                if avatar::person(
                                    ui,
                                    person,
                                    RichText::new(&person.display_name),
                                    egui::vec2(ui.available_width(), 44.0),
                                    Some(activity_user.as_ref() == Some(id)),
                                )
                                .clicked()
                                {
                                    *activity_user = Some(id.clone());
                                }
                            });
                        }
                    })
                    .response
                    .widget_info(|| {
                        let mut info = egui::WidgetInfo::labeled(
                            egui::WidgetType::ComboBox,
                            true,
                            "Activity for",
                        );
                        info.current_text_value = Some(
                            activity_user
                                .as_ref()
                                .map_or("All people", |id| contributors[id].display_name.as_str())
                                .to_string(),
                        );
                        info
                    });
                let mut daily = BTreeMap::<NaiveDate, (usize, usize)>::new();
                for (id, person) in contributors {
                    if activity_user
                        .as_ref()
                        .is_some_and(|selected| selected != id)
                    {
                        continue;
                    }
                    for day in &person.history {
                        if let Ok(date) = day.day.parse::<NaiveDate>()
                            && date <= today
                        {
                            let counts = daily.entry(date).or_default();
                            counts.0 += day.labeled;
                            counts.1 += day.reviewed;
                        }
                    }
                }
                let start = if start == NaiveDate::MIN {
                    daily
                        .keys()
                        .next()
                        .copied()
                        .unwrap_or(today)
                        .with_ordinal(1)
                        .unwrap()
                } else {
                    start
                };
                let (total, maximum) = daily
                    .range(start..=today)
                    .map(|(_, (labeled, reviewed))| labeled + reviewed)
                    .fold((0, 0), |(total, maximum), count| {
                        (total + count, maximum.max(count))
                    });
                ui.small(format!("{total} activities"))
                    .on_hover_text("Labeled tasks + reviews in the selected period.");
                (daily, start, maximum)
            })
            .inner;
        for year in start.year()..=today.year() {
            let first = start.max(NaiveDate::from_ymd_opt(year, 1, 1).unwrap());
            let last = today.min(NaiveDate::from_ymd_opt(year, 12, 31).unwrap());
            egui::ScrollArea::horizontal()
                .id_salt(("activity-calendar", year))
                .show(ui, |ui| {
                    let offset = first.weekday().num_days_from_monday() as usize;
                    let day_count = (last - first).num_days() as usize + 1;
                    let weeks = (offset + day_count).div_ceil(7);
                    let (rect, response) = ui.allocate_exact_size(
                        egui::vec2(50.0 + weeks as f32 * 14.0, 128.0),
                        egui::Sense::hover(),
                    );
                    response.widget_info(|| {
                        egui::WidgetInfo::labeled(
                            egui::WidgetType::Label,
                            true,
                            format!("{year} daily activity calendar"),
                        )
                    });
                    let painter = ui.painter_at(rect);
                    let font = egui::FontId::proportional(theme::SUPPORTING_SIZE);
                    painter.text(
                        rect.min,
                        egui::Align2::LEFT_TOP,
                        year.to_string(),
                        font.clone(),
                        theme::TEXT_MUTED,
                    );
                    for (row, label) in [(0, "Mon"), (2, "Wed"), (4, "Fri")] {
                        painter.text(
                            rect.min + egui::vec2(0.0, 29.0 + row as f32 * 14.0),
                            egui::Align2::LEFT_CENTER,
                            label,
                            font.clone(),
                            theme::TEXT_MUTED,
                        );
                    }
                    let mut last_month_column = None;
                    for index in 0..day_count {
                        let date = first + Days::new(index as u64);
                        let column = (offset + index) / 7;
                        let row = (offset + index) % 7;
                        let center = rect.min
                            + egui::vec2(42.0 + column as f32 * 14.0, 29.0 + row as f32 * 14.0);
                        if (index == 0 || date.day() == 1)
                            && last_month_column.is_none_or(|previous| column >= previous + 3)
                        {
                            painter.text(
                                egui::pos2(center.x - 6.0, rect.top()),
                                egui::Align2::LEFT_TOP,
                                date.format("%b").to_string(),
                                font.clone(),
                                theme::TEXT_MUTED,
                            );
                            last_month_column = Some(column);
                        }
                        let (labeled, reviewed) = daily.get(&date).copied().unwrap_or_default();
                        let count = labeled + reviewed;
                        let level =
                            ((count as u128 * 4).div_ceil(maximum.max(1) as u128) as usize).min(4);
                        let cell = egui::Rect::from_center_size(center, egui::vec2(12.0, 12.0));
                        painter.rect_filled(cell, 4, BLUES[level]);
                        let detail = format!(
                            "{date}: {count} activities · {labeled} labeled · {reviewed} reviewed"
                        );
                        ui.interact(
                            egui::Rect::from_center_size(center, egui::vec2(14.0, 14.0)),
                            ui.id().with(("activity-day", date)),
                            egui::Sense::hover(),
                        )
                        .on_hover_text(&detail)
                        .widget_info(|| {
                            egui::WidgetInfo::labeled(egui::WidgetType::Label, true, &detail)
                        });
                    }
                });
        }
        ui.horizontal_wrapped(|ui| {
            ui.small("Less");
            for color in BLUES {
                let (rect, _) =
                    ui.allocate_exact_size(egui::vec2(14.0, 14.0), egui::Sense::hover());
                let cell = rect.shrink(1.0);
                ui.painter().rect_filled(cell, 4, color);
            }
            ui.small(format!("More · up to {maximum} per day · UTC"));
        });
        let selected_day = activity_day.get_or_insert(today);
        *selected_day = (*selected_day).clamp(start, today);
        let day_combo = if ui.available_width() < 260.0 {
            ui.label("Activity day:");
            egui::ComboBox::from_id_salt("Activity day")
        } else {
            egui::ComboBox::from_label("Activity day")
        };
        day_combo
            .width(ui.available_width().min(140.0))
            .truncate()
            .selected_text(selected_day.to_string())
            .height(220.0)
            .show_ui(ui, |ui| {
                let count = (today - start).num_days() as usize + 1;
                egui::ScrollArea::vertical().show_rows(ui, 44.0, count, |ui, range| {
                    for offset in range {
                        let date = today - Days::new(offset as u64);
                        ui.selectable_value(selected_day, date, date.to_string());
                    }
                });
            })
            .response
            .widget_info(|| {
                let mut info =
                    egui::WidgetInfo::labeled(egui::WidgetType::ComboBox, true, "Activity day");
                info.current_text_value = Some(selected_day.to_string());
                info
            });
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(*selected_day > start, egui::Button::new("Previous day"))
                .clicked()
            {
                *selected_day -= chrono::TimeDelta::days(1);
            }
            if ui
                .add_enabled(*selected_day < today, egui::Button::new("Next day"))
                .clicked()
            {
                *selected_day += chrono::TimeDelta::days(1);
            }
        });
        let (labeled, reviewed) = daily.get(selected_day).copied().unwrap_or_default();
        ui.label(format!(
            "Selected day {selected_day}: {} activities · {labeled} labeled · {reviewed} reviewed",
            labeled + reviewed
        ));
    });
}

fn podium(ui: &mut egui::Ui, rows: &[Row<'_>], metric: usize) {
    ui.push_id(("podium", metric), |ui| {
        theme::card_frame().show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.label(RichText::new(PODIUM_TITLES[metric]).strong());
            let winners: Vec<_> = sorted(rows, metric)
                .into_iter()
                .filter(|(_, row)| {
                    if metric == 2 {
                        value(&row.total, metric).is_some()
                    } else {
                        value(&row.total, metric).is_some_and(|v| v > 0.0)
                    }
                })
                .take(3)
                .collect();
            if winners.is_empty() {
                ui.label("No ranked activity");
                return;
            }
            let compact = ui.available_width() < 600.0;
            ui.columns(3, |columns| {
                for (column, index) in columns.iter_mut().zip([1, 0, 2]) {
                    let height = if compact {
                        [64.0, 48.0, 36.0][index]
                    } else {
                        [96.0, 68.0, 48.0][index]
                    };
                    column.add_space(if compact { 134.0 } else { 166.0 } - height);
                    if let Some((rank, row)) = winners.get(index) {
                        let (rect, response) = column.allocate_exact_size(
                            egui::vec2(column.available_width(), height),
                            egui::Sense::hover(),
                        );
                        column.painter().rect_filled(
                            rect,
                            theme::CONTROL_RADIUS,
                            PODIUM_BLUES[index],
                        );
                        let avatar_rect = egui::Rect::from_center_size(
                            rect.center_top() - egui::vec2(0.0, 26.0),
                            egui::Vec2::splat(column.available_width().min(40.0)),
                        );
                        avatar::paint(column, row.person, avatar_rect);
                        if *rank == 1 {
                            let center = avatar_rect.center_top() - egui::vec2(0.0, 12.0);
                            let points = [
                                (-12.0, -6.0),
                                (-6.0, 0.0),
                                (0.0, -10.0),
                                (6.0, 0.0),
                                (12.0, -6.0),
                                (9.0, 7.0),
                                (-9.0, 7.0),
                            ]
                            .map(|(x, y)| center + egui::vec2(x, y));
                            column.painter().add(egui::Shape::closed_line(
                                points.to_vec(),
                                egui::Stroke::new(2.0, theme::WARNING),
                            ));
                        }
                        column.painter().text(
                            rect.center(),
                            egui::Align2::CENTER_CENTER,
                            format!("#{rank}"),
                            egui::FontId::proportional(theme::SECTION_HEADING_SIZE),
                            theme::INPUT_BG,
                        );
                        let label = format!(
                            "{}: rank {rank}, {}, {}",
                            METRICS[metric],
                            row.name,
                            display(&row.total, metric)
                        );
                        response.widget_info(|| {
                            egui::WidgetInfo::labeled(egui::WidgetType::Label, true, &label)
                        });
                        column.with_layout(egui::Layout::top_down(egui::Align::Center), |column| {
                            column
                                .add_sized(
                                    [column.available_width(), 22.0],
                                    egui::Label::new(RichText::new(row.name).strong())
                                        .truncate()
                                        .halign(egui::Align::Center),
                                )
                                .on_hover_text(row.name);
                            let score = if metric == 2 {
                                format!("{:.1}%", value(&row.total, metric).unwrap_or_default())
                            } else {
                                display(&row.total, metric)
                            };
                            let score_size = if metric == 3 && column.available_width() >= 80.0 {
                                24.0
                            } else {
                                18.0
                            };
                            column
                                .add_sized(
                                    [column.available_width(), score_size + 6.0],
                                    egui::Label::new(
                                        RichText::new(&score).size(score_size).strong(),
                                    )
                                    .truncate(),
                                )
                                .on_hover_text(score);
                            let caption = if metric == 2 {
                                format!(
                                    "{}/{}",
                                    row.total.accepted,
                                    row.total.accepted + row.total.rejected
                                )
                            } else {
                                ["tasks", "reviews", "", "score"][metric].into()
                            };
                            column
                                .add_sized(
                                    [column.available_width(), 18.0],
                                    egui::Label::new(RichText::new(&caption).small()).truncate(),
                                )
                                .on_hover_text(caption);
                        });
                    } else {
                        column.add_space(height);
                        column.label("—");
                    }
                }
            });
        });
    });
}

fn history_chart(
    ui: &mut egui::Ui,
    contributors: &BTreeMap<UserId, ContributorStats>,
    selected: &[UserId],
    start: NaiveDate,
    today: NaiveDate,
    metric: usize,
) {
    if selected.is_empty() {
        ui.label("Select people to compare their history.");
        return;
    }
    let first = contributors
        .values()
        .flat_map(|person| &person.history)
        .filter_map(|day| day.day.parse::<NaiveDate>().ok())
        .min()
        .unwrap_or(today);
    let start = start.max(first).min(today);
    let mut dates = BTreeSet::from([start, today]);
    dates.extend(
        selected
            .iter()
            .flat_map(|id| &contributors[id].history)
            .filter_map(|day| day.day.parse::<NaiveDate>().ok())
            .filter(|day| *day >= start && *day <= today),
    );
    let dates: Vec<_> = dates.into_iter().collect();
    let series: Vec<_> = selected
        .iter()
        .map(|id| {
            let person = &contributors[id];
            let mut history = person.history.iter().peekable();
            let mut total = ContributorDay::default();
            let values: Vec<_> = dates
                .iter()
                .map(|date| {
                    while let Some(day) = history.peek() {
                        match day.day.parse::<NaiveDate>() {
                            Ok(day_date) if day_date > *date => break,
                            Ok(_) => add(&mut total, day),
                            Err(_) => {}
                        }
                        history.next();
                    }
                    total.clone()
                })
                .collect();
            (&person.display_name, values)
        })
        .collect();
    ui.small("Running totals through each UTC day, including work before the selected period.");
    let maximum = if metric == 2 {
        100.0
    } else {
        series
            .iter()
            .flat_map(|(_, days)| days)
            .filter_map(|day| value(day, metric))
            .fold(1.0_f64, f64::max)
    };
    let minimum = if metric == 3 {
        series
            .iter()
            .flat_map(|(_, days)| days)
            .filter_map(|day| value(day, metric))
            .fold(0.0_f64, f64::min)
    } else {
        0.0
    };
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), 245.0),
        egui::Sense::hover(),
    );
    response.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::WidgetType::Other,
            true,
            format!("{} history graph", METRICS[metric]),
        )
    });
    let plot = egui::Rect::from_min_max(
        rect.min + egui::vec2(super::stats_axis_width(maximum as usize), 12.0),
        rect.max - egui::vec2(20.0, 35.0),
    );
    let painter = ui.painter_at(rect);
    let font = egui::FontId::monospace(theme::SUPPORTING_SIZE);
    for fraction in [0.0, 0.5, 1.0] {
        let y = plot.bottom() - plot.height() * fraction;
        painter.line_segment(
            [egui::pos2(plot.left(), y), egui::pos2(plot.right(), y)],
            egui::Stroke::new(1.0, theme::BORDER_STRONG),
        );
        painter.text(
            egui::pos2(plot.left() - 4.0, y),
            egui::Align2::RIGHT_CENTER,
            format!("{:.0}", minimum + (maximum - minimum) * fraction as f64),
            font.clone(),
            theme::TEXT_MUTED,
        );
    }
    for (date, x, align) in [
        (start, plot.left(), egui::Align2::LEFT_TOP),
        (today, plot.right(), egui::Align2::RIGHT_TOP),
    ] {
        painter.text(
            egui::pos2(x, plot.bottom() + 8.0),
            align,
            date.to_string(),
            font.clone(),
            theme::TEXT_MUTED,
        );
    }
    let span = (today - start).num_days().max(1) as f32;
    for (index, (_, days)) in series.iter().enumerate() {
        let color = COLORS[index % COLORS.len()];
        let mut previous: Option<egui::Pos2> = None;
        for (date, day) in dates.iter().zip(days) {
            let Some(v) = value(day, metric) else {
                previous = None;
                continue;
            };
            let point = egui::pos2(
                plot.left() + plot.width() * (*date - start).num_days() as f32 / span,
                plot.bottom() - plot.height() * ((v - minimum) / (maximum - minimum)) as f32,
            );
            if let Some(prev) = previous {
                let corner = egui::pos2(point.x, prev.y);
                painter.line_segment([prev, corner], egui::Stroke::new(2.0, color));
                painter.line_segment([corner, point], egui::Stroke::new(2.0, color));
            }
            if index % 2 == 0 {
                painter.circle_filled(point, 3.0, color);
            } else {
                painter.rect_filled(
                    egui::Rect::from_center_size(point, egui::vec2(5.0, 5.0)),
                    0.0,
                    color,
                );
            }
            previous = Some(point);
        }
        if let Some(end) = previous {
            painter.text(
                egui::pos2(
                    end.x + 4.0,
                    (end.y - 8.0).max(rect.top() + theme::SUPPORTING_SIZE + 2.0),
                ),
                egui::Align2::LEFT_BOTTOM,
                (index + 1).to_string(),
                font.clone(),
                color,
            );
        }
    }
    if let Some(pos) = response.hover_pos() {
        let offset = ((pos.x - plot.left()) / plot.width() * span)
            .round()
            .clamp(0.0, span) as u64;
        let date = (start + Days::new(offset)).min(today);
        let index = dates.partition_point(|day| *day <= date).saturating_sub(1);
        response.on_hover_ui(|ui| {
            ui.label(date.to_string());
            for (name, days) in &series {
                ui.label(format!("{name}: {}", display(&days[index], metric)));
            }
        });
    }
    for (index, date) in dates.iter().enumerate() {
        let x = plot.left() + plot.width() * (*date - start).num_days() as f32 / span;
        let detail = std::iter::once(date.to_string())
            .chain(
                series
                    .iter()
                    .map(|(name, days)| format!("{name}: {}", display(&days[index], metric))),
            )
            .collect::<Vec<_>>()
            .join("\n");
        ui.interact(
            egui::Rect::from_center_size(
                egui::pos2(x, plot.center().y),
                egui::vec2(6.0, plot.height()),
            ),
            ui.id().with(("history-day", date)),
            egui::Sense::hover(),
        )
        .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, &detail));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn score_aggregation_ranks_exact_points_even_when_display_rounds_to_a_tie() {
        let mut first = ContributorDay::default();
        first.score.labeling = 10_000;
        let mut second = first.clone();
        second.score.labeling += 1;
        assert_eq!(display(&first, 3), display(&second, 3));
        assert_eq!(compare(&first, &second, 3), Ordering::Less);
        let mut deduction = ContributorDay::default();
        deduction.score.deductions = 1_000;
        add(&mut first, &deduction);
        assert_eq!(first.score.total(), 9_000);
        assert_eq!(display(&deduction, 3), "-31");
    }

    #[test]
    fn periods_and_acceptance_use_calendar_boundaries_and_review_counts() {
        let today = NaiveDate::from_ymd_opt(2024, 3, 31).unwrap();
        let expected = [
            "2024-03-31",
            "2024-03-25",
            "2024-03-01",
            "2024-01-01",
            "2023-04-01",
        ];
        for (period, date) in expected.iter().enumerate() {
            assert_eq!(period_start(period, today).to_string(), *date);
        }
        assert_eq!(period_start(5, today), NaiveDate::MIN);
        let mut total = ContributorDay {
            accepted: 1,
            ..Default::default()
        };
        add(
            &mut total,
            &ContributorDay {
                accepted: 1,
                rejected: 8,
                ..Default::default()
            },
        );
        assert_eq!(value(&total, 2), Some(20.0));
        assert_eq!(value(&ContributorDay::default(), 2), None);
        let same_rate = ContributorDay {
            accepted: 1,
            rejected: 4,
            ..Default::default()
        };
        assert_eq!(compare(&total, &same_rate, 2), Ordering::Equal);
        let zero_rate = ContributorDay {
            rejected: 1,
            ..Default::default()
        };
        assert_eq!(
            compare(&zero_rate, &ContributorDay::default(), 2),
            Ordering::Greater
        );
    }
}
