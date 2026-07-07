use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::thread;
use eframe::egui;
use egui_plot::{Line, Plot, PlotPoints};
use polars::prelude::*;
use rfd::FileDialog;

/// Message sent from the background thread to the UI thread.
pub enum Message {
    Loaded {
        path: PathBuf,
        df: DataFrame,
        column_names: Vec<String>,
        numeric_columns: Vec<String>,
    },
    Error(String),
}

/// Statistics for a selected column.
#[derive(Clone, Copy, Debug)]
pub struct ColStats {
    pub min: f64,
    pub max: f64,
    pub mean: f64,
    pub std_dev: f64,
    pub count: usize,
}

pub struct CsvPlotterApp {
    // Current loaded dataset metadata
    file_path: Option<PathBuf>,
    df: Option<DataFrame>,
    column_names: Vec<String>,
    numeric_columns: Vec<String>,

    // Selected axes
    x_axis: Option<String>,
    y_axis: Option<String>,

    // Column statistics
    x_stats: Option<ColStats>,
    y_stats: Option<ColStats>,

    // Cached plot data
    plot_points: Option<Vec<[f64; 2]>>,

    // Plot customization and performance settings
    downsample: bool,
    max_points: usize,
    line_color: egui::Color32,
    line_width: f32,

    // Communication channels
    tx: Sender<Message>,
    rx: Receiver<Message>,

    // App state
    loading: bool,
    error: Option<String>,
    reset_view: bool,
}

impl CsvPlotterApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        // Apply custom visual styling for a premium dark mode feel
        let mut style = (*cc.egui_ctx.style()).clone();
        style.visuals.window_rounding = 8.0.into();
        style.visuals.widgets.noninteractive.rounding = 4.0.into();
        style.visuals.widgets.inactive.rounding = 4.0.into();
        style.visuals.widgets.hovered.rounding = 4.0.into();
        style.visuals.widgets.active.rounding = 4.0.into();
        cc.egui_ctx.set_style(style);

        let (tx, rx) = channel();

        Self {
            file_path: None,
            df: None,
            column_names: Vec::new(),
            numeric_columns: Vec::new(),
            x_axis: None,
            y_axis: None,
            x_stats: None,
            y_stats: None,
            plot_points: None,
            downsample: true,
            max_points: 20_000,
            line_color: egui::Color32::from_rgb(0, 191, 255), // DeepSkyBlue
            line_width: 1.5,
            tx,
            rx,
            loading: false,
            error: None,
            reset_view: false,
        }
    }

    /// Asynchronously read CSV using Polars on a background thread.
    fn load_csv_async(&mut self, path: PathBuf, ctx: egui::Context) {
        self.loading = true;
        self.error = None;
        let tx = self.tx.clone();

        thread::spawn(move || {
            let result = (|| -> Result<Message, String> {
                // Parse CSV efficiently using CsvReadOptions
                let df = CsvReadOptions::default()
                    .with_has_header(true)
                    .try_into_reader_with_file_path(Some(path.clone()))
                    .map_err(|e| format!("Failed to initialize reader: {}", e))?
                    .finish()
                    .map_err(|e| format!("Failed to parse CSV: {}", e))?;

                let schema = df.schema();

                // Extract all column names
                let column_names: Vec<String> = schema
                    .iter_fields()
                    .map(|f| f.name().to_string())
                    .collect();

                // Filter numeric columns
                let numeric_columns: Vec<String> = schema
                    .iter_fields()
                    .filter(|f| f.data_type().is_numeric())
                    .map(|f| f.name().to_string())
                    .collect();

                Ok(Message::Loaded {
                    path,
                    df,
                    column_names,
                    numeric_columns,
                })
            })();

            match result {
                Ok(msg) => {
                    let _ = tx.send(msg);
                }
                Err(err_msg) => {
                    let _ = tx.send(Message::Error(err_msg));
                }
            }
            ctx.request_repaint(); // Wake up GUI thread
        });
    }

    /// Calculate column statistics and plot points using selected columns
    fn recalculate_plot_data(&mut self) {
        self.plot_points = None;
        self.x_stats = None;
        self.y_stats = None;

        let df = match &self.df {
            Some(df) => df,
            None => return,
        };

        let x_name = match &self.x_axis {
            Some(name) => name,
            None => return,
        };

        let y_name = match &self.y_axis {
            Some(name) => name,
            None => return,
        };

        // Helper to compute stats for a Series by casting to Float64
        let calculate_stats = |series: &Series| -> Option<ColStats> {
            let f64_series = series.cast(&DataType::Float64).ok()?;
            let ca = f64_series.f64().ok()?;
            
            let min = ca.min()?;
            let max = ca.max()?;
            let mean = ca.mean()?;
            let std_dev = ca.std(1).unwrap_or(0.0);
            let count = ca.len() - ca.null_count();

            Some(ColStats {
                min,
                max,
                mean,
                std_dev,
                count,
            })
        };

        // Retrieve Series references from the Polars DataFrame
        let x_series = match df.column(x_name) {
            Ok(s) => s,
            Err(e) => {
                self.error = Some(format!("X-axis column retrieval failed: {}", e));
                return;
            }
        };

        let y_series = match df.column(y_name) {
            Ok(s) => s,
            Err(e) => {
                self.error = Some(format!("Y-axis column retrieval failed: {}", e));
                return;
            }
        };

        self.x_stats = calculate_stats(x_series);
        self.y_stats = calculate_stats(y_series);

        // Map and clean up points for egui_plot
        let points_result = (|| -> Result<Vec<[f64; 2]>, String> {
            let x_ca = x_series
                .cast(&DataType::Float64)
                .map_err(|e| e.to_string())?;
            let x_ca = x_ca.f64().map_err(|e| e.to_string())?;

            let y_ca = y_series
                .cast(&DataType::Float64)
                .map_err(|e| e.to_string())?;
            let y_ca = y_ca.f64().map_err(|e| e.to_string())?;

            let mut raw_points: Vec<[f64; 2]> = Vec::with_capacity(x_ca.len());
            for (x_val, y_val) in x_ca.iter().zip(y_ca.iter()) {
                if let (Some(x), Some(y)) = (x_val, y_val) {
                    raw_points.push([x, y]);
                }
            }

            let total_len = raw_points.len();
            if self.downsample && total_len > self.max_points {
                let step = (total_len / self.max_points).max(1);
                let mut downsampled = Vec::with_capacity(total_len / step + 1);
                for i in (0..total_len).step_by(step) {
                    downsampled.push(raw_points[i]);
                }
                Ok(downsampled)
            } else {
                Ok(raw_points)
            }
        })();

        match points_result {
            Ok(pts) => {
                self.plot_points = Some(pts);
                self.reset_view = true; // Auto-focus plot boundaries on load
            }
            Err(e) => {
                self.error = Some(format!("Failed to prepare plot points: {}", e));
            }
        }
    }
}

impl eframe::App for CsvPlotterApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Poll background thread channel messages
        if let Ok(msg) = self.rx.try_recv() {
            self.loading = false;
            match msg {
                Message::Loaded {
                    path,
                    df,
                    column_names,
                    numeric_columns,
                } => {
                    self.file_path = Some(path);
                    self.column_names = column_names;
                    self.numeric_columns = numeric_columns;

                    // Automatically choose default X and Y columns
                    if self.numeric_columns.len() >= 2 {
                        self.x_axis = Some(self.numeric_columns[0].clone());
                        self.y_axis = Some(self.numeric_columns[1].clone());
                    } else if !self.numeric_columns.is_empty() {
                        self.x_axis = Some(self.numeric_columns[0].clone());
                        self.y_axis = Some(self.numeric_columns[0].clone());
                    } else if !self.column_names.is_empty() {
                        self.x_axis = Some(self.column_names[0].clone());
                        self.y_axis = Some(self.column_names[0].clone());
                    }

                    self.df = Some(df);
                    self.recalculate_plot_data();
                }
                Message::Error(e) => {
                    self.error = Some(e);
                }
            }
        }

        // Top Panel: Header & File Loading Controls
        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading("⚡ Fast CSV Plotter");
                ui.separator();

                if ui.button("📂 Open CSV File").clicked() && !self.loading {
                    if let Some(path) = FileDialog::new()
                        .add_filter("CSV Files", &["csv"])
                        .pick_file()
                    {
                        self.load_csv_async(path, ctx.clone());
                    }
                }

                if self.loading {
                    ui.spinner();
                    ui.label("Processing dataset with Polars engine...");
                } else if let Some(path) = &self.file_path {
                    ui.label(format!("File: {}", path.to_string_lossy()));
                    if let Some(df) = &self.df {
                        ui.label(format!(
                            "({} rows, {} columns)",
                            df.height(),
                            df.width()
                        ));
                    }
                } else {
                    ui.label("No file loaded");
                }
            });
        });

        // Main Area
        egui::CentralPanel::default().show(ctx, |ui| {
            // Render error bar if any error occurs
            if let Some(err) = &self.error {
                ui.horizontal(|ui| {
                    ui.colored_label(egui::Color32::LIGHT_RED, format!("⚠️ Error: {}", err));
                    if ui.button("Dismiss").clicked() {
                        self.error = None;
                    }
                });
            }

            if self.df.is_some() {
                // Split central panel into a sidebar for settings/stats, and plot window
                egui::SidePanel::left("left_sidebar")
                    .resizable(true)
                    .default_width(260.0)
                    .show_inside(ui, |ui| {
                        egui::ScrollArea::vertical().show(ui, |ui| {
                            ui.vertical_centered(|ui| {
                                ui.heading("Settings & Stats");
                            });
                            ui.add_space(8.0);

                            // Axes Selection Configuration
                            ui.group(|ui| {
                                ui.label("Axis Configuration");
                                ui.separator();
                                
                                let mut changed = false;

                                ui.horizontal(|ui| {
                                    ui.label("X-Axis:");
                                    egui::ComboBox::from_id_salt("x_combo")
                                        .selected_text(self.x_axis.as_deref().unwrap_or("Select..."))
                                        .show_ui(ui, |ui| {
                                            for col in &self.column_names {
                                                changed |= ui
                                                    .selectable_value(&mut self.x_axis, Some(col.clone()), col)
                                                    .changed();
                                            }
                                        });
                                });

                                ui.horizontal(|ui| {
                                    ui.label("Y-Axis:");
                                    egui::ComboBox::from_id_salt("y_combo")
                                        .selected_text(self.y_axis.as_deref().unwrap_or("Select..."))
                                        .show_ui(ui, |ui| {
                                            for col in &self.column_names {
                                                changed |= ui
                                                    .selectable_value(&mut self.y_axis, Some(col.clone()), col)
                                                    .changed();
                                            }
                                        });
                                });

                                if changed {
                                    self.recalculate_plot_data();
                                }
                            });

                            ui.add_space(8.0);

                            // Performance Settings Configuration
                            ui.group(|ui| {
                                ui.label("Performance & Style");
                                ui.separator();

                                if ui.checkbox(&mut self.downsample, "Downsample Data").changed() {
                                    self.recalculate_plot_data();
                                }

                                if self.downsample {
                                    ui.horizontal(|ui| {
                                        ui.label("Max Points:");
                                        let speed_slider = ui.add(
                                            egui::Slider::new(&mut self.max_points, 1_000..=100_000)
                                                .logarithmic(true)
                                                .text(""),
                                        );
                                        if speed_slider.changed() {
                                            self.recalculate_plot_data();
                                        }
                                    });
                                }

                                ui.horizontal(|ui| {
                                    ui.label("Line Color:");
                                    ui.color_edit_button_srgba(&mut self.line_color);
                                });

                                ui.horizontal(|ui| {
                                    ui.label("Line Width:");
                                    ui.add(egui::Slider::new(&mut self.line_width, 0.5..=5.0));
                                });

                                ui.add_space(4.0);
                                if ui.button("🔄 Reset Plot View").clicked() {
                                    self.reset_view = true;
                                }
                            });

                            ui.add_space(8.0);

                            // Statistics Panel
                            if self.x_stats.is_some() || self.y_stats.is_some() {
                                ui.group(|ui| {
                                    ui.label("Statistical Analysis");
                                    ui.separator();

                                    let render_stats = |ui: &mut egui::Ui, name: &str, stats: &ColStats| {
                                        ui.label(egui::RichText::new(name).strong());
                                        egui::Grid::new(format!("{}_grid", name))
                                            .num_columns(2)
                                            .spacing([10.0, 4.0])
                                            .show(ui, |ui| {
                                                ui.label("Min:");
                                                ui.label(format!("{:.4}", stats.min));
                                                ui.end_row();

                                                ui.label("Max:");
                                                ui.label(format!("{:.4}", stats.max));
                                                ui.end_row();

                                                ui.label("Mean:");
                                                ui.label(format!("{:.4}", stats.mean));
                                                ui.end_row();

                                                ui.label("Std Dev:");
                                                ui.label(format!("{:.4}", stats.std_dev));
                                                ui.end_row();

                                                ui.label("Count:");
                                                ui.label(format!("{}", stats.count));
                                                ui.end_row();
                                            });
                                        ui.add_space(4.0);
                                    };

                                    if let (Some(x_name), Some(x_s)) = (&self.x_axis, &self.x_stats) {
                                        render_stats(ui, x_name, x_s);
                                    }

                                    if let (Some(y_name), Some(y_s)) = (&self.y_axis, &self.y_stats) {
                                        if self.x_axis != self.y_axis {
                                            ui.separator();
                                            render_stats(ui, y_name, y_s);
                                        }
                                    }
                                });
                            }
                        });
                    });

                // Plot Area
                egui::CentralPanel::default().show_inside(ui, |ui| {
                    if let Some(points) = &self.plot_points {
                        let line_name = format!(
                            "{} vs {}",
                            self.y_axis.as_deref().unwrap_or(""),
                            self.x_axis.as_deref().unwrap_or("")
                        );

                        let line = Line::new(PlotPoints::new(points.clone()))
                            .color(self.line_color)
                            .width(self.line_width)
                            .name(&line_name);

                        let mut target_bounds = None;
                        // Adjust plot bounds programmatically if requested
                        if self.reset_view {
                            if let (Some(x_s), Some(y_s)) = (&self.x_stats, &self.y_stats) {
                                let x_margin = (x_s.max - x_s.min) * 0.05;
                                let y_margin = (y_s.max - y_s.min) * 0.05;
                                let x_min = x_s.min - if x_margin == 0.0 { 1.0 } else { x_margin };
                                let x_max = x_s.max + if x_margin == 0.0 { 1.0 } else { x_margin };
                                let y_min = y_s.min - if y_margin == 0.0 { 1.0 } else { y_margin };
                                let y_max = y_s.max + if y_margin == 0.0 { 1.0 } else { y_margin };

                                target_bounds = Some(egui_plot::PlotBounds::from_min_max(
                                    [x_min, y_min],
                                    [x_max, y_max]
                                ));
                            }
                            self.reset_view = false;
                        }

                        let plot = Plot::new("csv_plot")
                            .legend(egui_plot::Legend::default())
                            .x_axis_label(self.x_axis.as_deref().unwrap_or("X"))
                            .y_axis_label(self.y_axis.as_deref().unwrap_or("Y"));

                        plot.show(ui, |plot_ui| {
                            if let Some(bounds) = target_bounds {
                                plot_ui.set_plot_bounds(bounds);
                            }
                            plot_ui.line(line);
                        });
                    } else {
                        ui.centered_and_justified(|ui| {
                            ui.label("Configure X and Y axis columns to render the plot.");
                        });
                    }
                });
            } else {
                // Landing Screen Visual Design
                ui.centered_and_justified(|ui| {
                    ui.vertical_centered(|ui| {
                        ui.add_space(ui.available_height() * 0.3);
                        ui.heading(
                            egui::RichText::new("⚡ Welcome to Fast CSV Plotter")
                                .size(28.0)
                                .strong(),
                        );
                        ui.add_space(8.0);
                        ui.label(
                            egui::RichText::new(
                                "High-performance facility CSV data visualizer powered by Polars & egui",
                            )
                            .size(16.0)
                            .weak(),
                        );
                        ui.add_space(20.0);

                        let original_padding = ui.spacing().button_padding;
                        ui.spacing_mut().button_padding = egui::vec2(16.0, 8.0);
                        let import_clicked = ui
                            .add(egui::Button::new(
                                egui::RichText::new("📂 Import CSV File").size(18.0),
                            ))
                            .clicked();
                        ui.spacing_mut().button_padding = original_padding;

                        if import_clicked {
                            if let Some(path) = FileDialog::new()
                                .add_filter("CSV Files", &["csv"])
                                .pick_file()
                            {
                                self.load_csv_async(path, ctx.clone());
                            }
                        }

                        ui.add_space(12.0);
                        ui.label("Handles millions of rows smoothly using background processing and decimation.");
                    });
                });
            }
        });
    }
}
