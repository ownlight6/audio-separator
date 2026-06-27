use std::path::PathBuf;
use std::sync::mpsc;

use audio_separator::{separate_file, ProgressUpdate, SeparationResult};

// ---------------------------------------------------------------------------
// i18n
// ---------------------------------------------------------------------------

#[derive(Default, PartialEq, Clone, Copy)]
enum Language {
    #[default]
    Zh,
    En,
}

struct Translations {
    title: &'static str,
    subtitle: &'static str,
    input_file: &'static str,
    output_dir: &'static str,
    browse: &'static str,
    separate_btn: &'static str,
    working: &'static str,
    input_required: &'static str,
    drag_drop_hint: &'static str,
    select_file: &'static str,
    select_output_dir: &'static str,
    completed: &'static str,
    vocals_label: &'static str,
    accomp_label: &'static str,
    stage_downloading: &'static str,
    stage_decoding: &'static str,
    stage_resampling: &'static str,
    stage_separating: &'static str,
    stage_writing: &'static str,
    duration_label: &'static str,
}

const ZH: Translations = Translations {
    title: "Audio Separator",
    subtitle: "分离音频中的人声与伴奏",
    input_file: "输入文件：",
    output_dir: "输出目录：",
    browse: "浏览…",
    separate_btn: "分离",
    working: "处理中…",
    input_required: "请选择输入文件",
    drag_drop_hint: "或将文件拖放到此处",
    select_file: "选择音频文件",
    select_output_dir: "选择输出目录",
    completed: "完成！",
    vocals_label: "人声：",
    accomp_label: "伴奏：",
    stage_downloading: "正在下载模型…",
    stage_decoding: "正在解码音频…",
    stage_resampling: "正在重采样…",
    stage_separating: "正在分离…",
    stage_writing: "正在写入…",
    duration_label: "时长：{:.1} 秒",
};

const EN: Translations = Translations {
    title: "Audio Separator",
    subtitle: "Separate vocals from background music",
    input_file: "Input file:",
    output_dir: "Output directory:",
    browse: "Browse…",
    separate_btn: "Separate",
    working: "Working…",
    input_required: "Please select an input file",
    drag_drop_hint: "or drag and drop files here",
    select_file: "Select audio file",
    select_output_dir: "Select output directory",
    completed: "Done!",
    vocals_label: "Vocals:",
    accomp_label: "Accompaniment:",
    stage_downloading: "Downloading model…",
    stage_decoding: "Decoding audio…",
    stage_resampling: "Resampling…",
    stage_separating: "Separating…",
    stage_writing: "Writing output…",
    duration_label: "Duration: {:.1}s",
};

fn t(lang: Language) -> &'static Translations {
    match lang {
        Language::Zh => &ZH,
        Language::En => &EN,
    }
}

// ---------------------------------------------------------------------------
// CJK Font Loading
// ---------------------------------------------------------------------------

fn load_cjk_system_font() -> Option<Vec<u8>> {
    let candidates: &[&str] = &[
        "/System/Library/Fonts/STHeiti Medium.ttc",
        "/System/Library/Fonts/STHeiti Light.ttc",
        "/System/Library/Fonts/Supplemental/Songti.ttc",
        "/Library/Fonts/Arial Unicode.ttf",
        "C:\\Windows\\Fonts\\msyh.ttc",
        "C:\\Windows\\Fonts\\msyhbd.ttc",
        "C:\\Windows\\Fonts\\simhei.ttf",
        "C:\\Windows\\Fonts\\simsun.ttc",
        "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/truetype/wqy/wqy-microhei.ttc",
    ];

    for path in candidates {
        if let Ok(data) = std::fs::read(path) {
            return Some(data);
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

pub fn run() {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([680.0, 480.0])
            .with_min_inner_size([500.0, 400.0])
            .with_title("Audio Separator"),
        ..Default::default()
    };

    eframe::run_native(
        "Audio Separator",
        options,
        Box::new(|cc| {
            if let Some(font_data) = load_cjk_system_font() {
                cc.egui_ctx.add_font(egui::epaint::text::FontInsert::new(
                    "cjk_system_font",
                    egui::FontData::from_owned(font_data),
                    vec![
                        egui::epaint::text::InsertFontFamily {
                            family: egui::FontFamily::Proportional,
                            priority: egui::epaint::text::FontPriority::Lowest,
                        },
                        egui::epaint::text::InsertFontFamily {
                            family: egui::FontFamily::Monospace,
                            priority: egui::epaint::text::FontPriority::Lowest,
                        },
                    ],
                ));
            }
            Ok(Box::new(App::default()))
        }),
    )
    .expect("Failed to launch GUI");
}

// ---------------------------------------------------------------------------
// Operation status
// ---------------------------------------------------------------------------

#[derive(Default)]
enum OperationStatus {
    #[default]
    Idle,
    Running,
    Success(SeparationResult),
    Error(String),
}

// ---------------------------------------------------------------------------
// App
// ---------------------------------------------------------------------------

#[derive(Default)]
struct App {
    input_path: String,
    output_dir: String,
    lang: Language,

    status: OperationStatus,
    progress_stage: String,
    progress_value: f32,

    result_rx: Option<mpsc::Receiver<Result<SeparationResult, String>>>,
    progress_rx: Option<mpsc::Receiver<ProgressUpdate>>,
    input_picker_rx: Option<mpsc::Receiver<Option<String>>>,
    output_picker_rx: Option<mpsc::Receiver<Option<String>>>,
}

impl App {
    fn spawn_file_picker(
        ctx: &egui::Context,
        title: &str,
    ) -> mpsc::Receiver<Option<String>> {
        let (tx, rx) = mpsc::channel();
        let ctx = ctx.clone();
        let title = title.to_string();
        std::thread::spawn(move || {
            let dialog = rfd::AsyncFileDialog::new()
                .set_title(&title)
                .add_filter(
                    "Audio Files",
                    &["mp3", "flac", "ogg", "wav"],
                );
            let file = pollster::block_on(dialog.pick_file());
            let path = file.map(|f| f.path().display().to_string());
            let _ = tx.send(path);
            ctx.request_repaint();
        });
        rx
    }

    fn spawn_folder_picker(
        ctx: &egui::Context,
        title: &str,
    ) -> mpsc::Receiver<Option<String>> {
        let (tx, rx) = mpsc::channel();
        let ctx = ctx.clone();
        let title = title.to_string();
        std::thread::spawn(move || {
            let dialog = rfd::AsyncFileDialog::new().set_title(&title);
            let folder = pollster::block_on(dialog.pick_folder());
            let path = folder.map(|f| f.path().display().to_string());
            let _ = tx.send(path);
            ctx.request_repaint();
        });
        rx
    }

    fn poll_pending_pickers(&mut self) {
        if let Some(rx) = self.input_picker_rx.take() {
            if let Ok(path) = rx.try_recv() {
                if let Some(p) = path {
                    self.input_path = p;
                    self.auto_populate_output();
                }
            } else {
                self.input_picker_rx = Some(rx);
            }
        }
        if let Some(rx) = self.output_picker_rx.take() {
            if let Ok(path) = rx.try_recv() {
                if let Some(p) = path {
                    self.output_dir = p;
                }
            } else {
                self.output_picker_rx = Some(rx);
            }
        }
    }

    fn auto_populate_output(&mut self) {
        if self.output_dir.is_empty() {
            let input = PathBuf::from(&self.input_path);
            if let Some(parent) = input.parent() {
                self.output_dir = parent.display().to_string();
            }
        }
    }

    fn poll_progress(&mut self, ctx: &egui::Context) {
        if let Some(ref rx) = self.progress_rx {
            while let Ok(update) = rx.try_recv() {
                self.progress_stage = update.stage;
                self.progress_value = update.progress;
            }
            ctx.request_repaint();
        }
    }

    fn poll_operation_result(&mut self) {
        if let Some(rx) = self.result_rx.take() {
            if let Ok(result) = rx.try_recv() {
                self.progress_rx = None;
                self.status = match result {
                    Ok(sep_result) => OperationStatus::Success(sep_result),
                    Err(e) => OperationStatus::Error(e),
                };
            } else {
                self.result_rx = Some(rx);
            }
        }
    }

    fn handle_dropped_files(&mut self, ctx: &egui::Context) {
        let dropped = ctx.input(|i| i.raw.dropped_files.clone());
        if let Some(file) = dropped.first() {
            if let Some(path) = &file.path {
                self.input_path = path.display().to_string();
                self.auto_populate_output();
            }
        }
    }

    fn stage_label(&self, tr: &Translations) -> &str {
        match self.progress_stage.as_str() {
            "downloading_model" => tr.stage_downloading,
            "decoding" => tr.stage_decoding,
            "resampling" | "resampling_output" => tr.stage_resampling,
            "separating" => tr.stage_separating,
            "writing" => tr.stage_writing,
            _ => tr.working,
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_pending_pickers();
        self.poll_progress(ctx);
        self.poll_operation_result();
        self.handle_dropped_files(ctx);

        let tr = t(self.lang);
        let is_running = matches!(self.status, OperationStatus::Running);

        egui::CentralPanel::default().show(ctx, |ui| {
            // Title row with language toggle
            ui.horizontal(|ui| {
                ui.heading(tr.title);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let lang_label = match self.lang {
                        Language::Zh => "EN",
                        Language::En => "中文",
                    };
                    if ui.small_button(lang_label).clicked() {
                        self.lang = match self.lang {
                            Language::Zh => Language::En,
                            Language::En => Language::Zh,
                        };
                    }
                    ui.add_space(4.0);
                    ui.hyperlink_to("GitHub", "https://github.com/ownlight6/audio-separator");
                });
            });
            ui.label(tr.subtitle);
            ui.add_space(8.0);

            // ===== Input section =====
            ui.label(tr.input_file);
            ui.add_enabled(
                !is_running,
                egui::TextEdit::singleline(&mut self.input_path)
                    .desired_width(f32::INFINITY),
            );
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(!is_running, egui::Button::new(tr.browse))
                    .clicked()
                {
                    self.input_picker_rx =
                        Some(Self::spawn_file_picker(ctx, tr.select_file));
                }
                ui.label(
                    egui::RichText::new(tr.drag_drop_hint)
                        .small()
                        .color(egui::Color32::GRAY),
                );
            });
            ui.add_space(4.0);

            // ===== Output section =====
            ui.label(tr.output_dir);
            ui.add_enabled(
                !is_running,
                egui::TextEdit::singleline(&mut self.output_dir)
                    .desired_width(f32::INFINITY),
            );
            if ui
                .add_enabled(!is_running, egui::Button::new(tr.browse))
                .clicked()
            {
                self.output_picker_rx =
                    Some(Self::spawn_folder_picker(ctx, tr.select_output_dir));
            }
            ui.add_space(8.0);

            // ===== Action button =====
            if ui
                .add_enabled(!is_running, egui::Button::new(tr.separate_btn))
                .clicked()
            {
                if self.input_path.is_empty() {
                    self.status = OperationStatus::Error(tr.input_required.into());
                } else {
                    let input = PathBuf::from(&self.input_path);
                    let output_dir = if self.output_dir.is_empty() {
                        input
                            .parent()
                            .unwrap_or(std::path::Path::new("."))
                            .to_path_buf()
                    } else {
                        PathBuf::from(&self.output_dir)
                    };

                    let (result_tx, result_rx) = mpsc::channel();
                    let (progress_tx, progress_rx) = mpsc::channel();
                    let ctx_clone = ctx.clone();

                    self.status = OperationStatus::Running;
                    self.progress_stage.clear();
                    self.progress_value = 0.0;
                    self.result_rx = Some(result_rx);
                    self.progress_rx = Some(progress_rx);

                    std::thread::spawn(move || {
                        let result = separate_file(
                            &input,
                            &output_dir,
                            None,
                            Some(progress_tx),
                        );
                        let _ = result_tx.send(result);
                        ctx_clone.request_repaint();
                    });
                }
            }

            ui.add_space(8.0);

            // ===== Status display =====
            match &self.status {
                OperationStatus::Idle => {}
                OperationStatus::Running => {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(self.stage_label(tr));
                    });
                    let progress = egui::ProgressBar::new(self.progress_value)
                        .show_percentage();
                    ui.add(progress);
                }
                OperationStatus::Success(result) => {
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(format!("✓ {}", tr.completed))
                                .color(egui::Color32::from_rgb(0x2E, 0x7D, 0x32)),
                        )
                        .wrap(),
                    );
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        ui.label(tr.vocals_label);
                        ui.monospace(result.vocals_path.display().to_string());
                    });
                    ui.horizontal(|ui| {
                        ui.label(tr.accomp_label);
                        ui.monospace(result.accompaniment_path.display().to_string());
                    });
                    ui.label(
                        egui::RichText::new(format!(
                            "{}",
                            tr.duration_label
                                .replacen("{:.1}", &format!("{:.1}", result.duration_secs), 1)
                        ))
                        .small()
                        .color(egui::Color32::GRAY),
                    );
                }
                OperationStatus::Error(err) => {
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(format!("✗ {}", err))
                                .color(egui::Color32::from_rgb(0xC6, 0x28, 0x28)),
                        )
                        .wrap(),
                    );
                }
            }
        });
    }
}
