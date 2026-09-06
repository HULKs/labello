use std::{cell::Cell, error::Error, future::Future, pin::Pin, rc::Rc, time::Duration};

use labello_ui::inspector_presets::InspectorPreset;

type LocalTask = Pin<Box<dyn Future<Output = ()> + 'static>>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum InspectorMode {
    Preset(InspectorPreset),
    Live,
}

fn main() -> Result<(), Box<dyn Error>> {
    let (mode, display) = parse_options(std::env::args().skip(1))
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidInput, error))?;
    eframe::run_native(
        "Labello MCP Inspector",
        eframe::NativeOptions {
            viewport: eframe::egui::ViewportBuilder::default().with_inner_size(display.size),
            ..Default::default()
        },
        Box::new(move |creation_context| {
            creation_context.egui_ctx.enable_accesskit();
            creation_context.egui_ctx.set_zoom_factor(display.scale);
            Ok(Box::new(InspectorApp::new(
                mode,
                &creation_context.egui_ctx,
            )?))
        }),
    )?;
    Ok(())
}

#[derive(Debug, PartialEq)]
struct DisplayOptions {
    size: [f32; 2],
    scale: f32,
}

fn parse_options(
    args: impl IntoIterator<Item = String>,
) -> Result<(InspectorMode, DisplayOptions), String> {
    let args = args.into_iter().collect::<Vec<_>>();
    let mut mode_args = Vec::new();
    let mut display_name = "wide".to_string();
    let mut scale = 1.0_f32;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--display" | "--scale" => {
                let value = args.get(index + 1).ok_or_else(usage)?;
                if args[index] == "--display" {
                    display_name = value.clone();
                } else {
                    scale = value.parse().map_err(|_| usage())?;
                }
                index += 2;
            }
            _ => {
                mode_args.push(args[index].clone());
                index += 1;
            }
        }
    }
    if !scale.is_finite() || !(0.5..=3.0).contains(&scale) {
        return Err(usage());
    }
    let matrix: serde_json::Value =
        serde_json::from_str(include_str!("../../../scripts/browser/matrix.json"))
            .expect("maintained display matrix must be valid");
    let viewport = matrix["viewports"]
        .as_array()
        .expect("matrix viewports")
        .iter()
        .find(|view| view["name"].as_str() == Some(&display_name))
        .ok_or_else(usage)?;
    let size = [
        viewport["width"].as_f64().expect("matrix width") as f32,
        viewport["height"].as_f64().expect("matrix height") as f32,
    ];
    Ok((parse_mode(mode_args)?, DisplayOptions { size, scale }))
}

fn parse_mode(args: impl IntoIterator<Item = String>) -> Result<InspectorMode, String> {
    let args = args.into_iter().collect::<Vec<_>>();
    match args.as_slice() {
        [] => Ok(InspectorMode::Preset(InspectorPreset::Annotation)),
        [argument] if argument == "--live" => Ok(InspectorMode::Live),
        [flag, name] if flag == "--preset" => InspectorPreset::from_name(name)
            .map(InspectorMode::Preset)
            .ok_or_else(usage),
        _ => Err(usage()),
    }
}

fn usage() -> String {
    format!(
        "Usage: labello-egui-mcp-inspector [--live | --preset <{}>] [--display <matrix-name>] [--scale <0.5..3>]",
        InspectorPreset::ALL
            .into_iter()
            .map(InspectorPreset::name)
            .collect::<Vec<_>>()
            .join("|")
    )
}

struct InspectorApp {
    app: labello_ui::LabelloApp,
    executor: Option<Rc<NativeExecutor>>,
}

impl InspectorApp {
    fn new(mode: InspectorMode, ctx: &eframe::egui::Context) -> std::io::Result<Self> {
        match mode {
            InspectorMode::Preset(preset) => Ok(Self {
                app: labello_ui::inspector_presets::build(preset, ctx),
                executor: None,
            }),
            InspectorMode::Live => {
                let executor = Rc::new(NativeExecutor::new()?);
                let executor_for_spawner = executor.clone();
                let mut app = labello_ui::LabelloApp::live_http(labello_ui::AppConfig {
                    application_url: Some("http://127.0.0.1:8081".to_string()),
                    ..Default::default()
                });
                app.set_native_task_spawner(move |task| executor_for_spawner.spawn(task));
                Ok(Self {
                    app,
                    executor: Some(executor),
                })
            }
        }
    }
}

impl eframe::App for InspectorApp {
    fn ui(&mut self, ui: &mut eframe::egui::Ui, frame: &mut eframe::Frame) {
        if let Some(executor) = &self.executor {
            executor.tick();
        }
        eframe::App::ui(&mut self.app, ui, frame);
        if self
            .executor
            .as_ref()
            .is_some_and(|executor| executor.pending_tasks() > 0)
        {
            ui.ctx().request_repaint_after(Duration::from_millis(16));
        }
    }
}

struct NativeExecutor {
    runtime: tokio::runtime::Runtime,
    tasks: Rc<tokio::task::LocalSet>,
    pending: Rc<Cell<usize>>,
}

impl NativeExecutor {
    fn new() -> std::io::Result<Self> {
        Ok(Self {
            runtime: tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()?,
            tasks: Rc::new(tokio::task::LocalSet::new()),
            pending: Rc::new(Cell::new(0)),
        })
    }

    fn spawn(&self, task: LocalTask) {
        self.pending.set(self.pending.get() + 1);
        let pending = self.pending.clone();
        self.tasks.spawn_local(async move {
            let _pending_task = PendingTask(pending);
            task.await;
        });
    }

    fn tick(&self) {
        if self.pending_tasks() == 0 {
            return;
        }
        self.runtime.block_on(
            self.tasks
                .run_until(async { tokio::task::yield_now().await }),
        );
    }

    fn pending_tasks(&self) -> usize {
        self.pending.get()
    }
}

struct PendingTask(Rc<Cell<usize>>);

impl Drop for PendingTask {
    fn drop(&mut self) {
        self.0.set(self.0.get().saturating_sub(1));
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::Cell, rc::Rc};

    use super::*;

    #[test]
    fn annotation_is_the_default_and_live_mode_is_explicit() {
        assert_eq!(
            parse_mode(Vec::<String>::new()).unwrap(),
            InspectorMode::Preset(InspectorPreset::Annotation)
        );
        assert_eq!(
            parse_mode(["--live".to_string()]).unwrap(),
            InspectorMode::Live
        );
    }

    #[test]
    fn every_named_preset_is_accepted() {
        for preset in InspectorPreset::ALL {
            assert_eq!(
                parse_mode(["--preset".to_string(), preset.name().to_string()]).unwrap(),
                InspectorMode::Preset(preset)
            );
        }
    }

    #[test]
    fn unknown_arguments_are_rejected() {
        let error = parse_mode(["--unknown".to_string()]).unwrap_err();
        assert!(error.contains("Usage:"));
    }

    #[test]
    fn shared_matrix_display_and_scale_are_explicit_and_validated() {
        let (_, display) =
            parse_options(["--display", "mobile", "--scale", "2"].map(str::to_string)).unwrap();
        assert_eq!(
            display,
            DisplayOptions {
                size: [390.0, 844.0],
                scale: 2.0
            }
        );
        for arguments in [
            vec!["--display", "missing"],
            vec!["--scale", "NaN"],
            vec!["--scale", "0"],
            vec!["--scale"],
        ] {
            assert!(parse_options(arguments.into_iter().map(str::to_string)).is_err());
        }
    }

    #[test]
    fn native_executor_advances_and_finishes_local_tasks() {
        let executor = NativeExecutor::new().unwrap();
        let completed = Rc::new(Cell::new(false));
        let completed_for_task = completed.clone();
        executor.spawn(Box::pin(async move {
            completed_for_task.set(true);
        }));

        assert_eq!(executor.pending_tasks(), 1);
        executor.tick();

        assert!(completed.get());
        assert_eq!(executor.pending_tasks(), 0);
    }

    #[test]
    fn native_executor_drives_timer_tasks() {
        let executor = NativeExecutor::new().unwrap();
        let completed = Rc::new(Cell::new(false));
        let completed_for_task = completed.clone();
        executor.spawn(Box::pin(async move {
            tokio::time::sleep(Duration::from_millis(1)).await;
            completed_for_task.set(true);
        }));

        for _ in 0..50 {
            executor.tick();
            if completed.get() {
                break;
            }
            std::thread::sleep(Duration::from_millis(1));
        }

        assert!(completed.get());
        assert_eq!(executor.pending_tasks(), 0);
    }
}
