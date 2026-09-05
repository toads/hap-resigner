use std::borrow::Cow;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{Duration, Instant};

use crate::agc::client::CERTIFICATE_MANAGEMENT_URL;
use crate::device::hdc::DeviceInfo;
use eframe::egui;

use super::state::{AppPhase, AppState, LogLevel, WorkflowEvent};
use super::workflow::{spawn_device_scan, spawn_install, spawn_resign};

const SELECTED_DEVICE_UDID_KEY: &str = "selected-device-udid";
const DEVICE_SCAN_INTERVAL: Duration = Duration::from_secs(3);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct DeviceScanPlan {
    start: bool,
    pending: bool,
}

impl DeviceScanPlan {
    const fn idle() -> Self {
        Self {
            start: false,
            pending: false,
        }
    }

    const fn start() -> Self {
        Self {
            start: true,
            pending: false,
        }
    }

    const fn queued() -> Self {
        Self {
            start: false,
            pending: true,
        }
    }
}

fn device_scan_plan(
    phase: AppPhase,
    scan_running: bool,
    scan_pending: bool,
    elapsed: Duration,
    menu_just_opened: bool,
) -> DeviceScanPlan {
    if !device_controls_enabled(phase) {
        return DeviceScanPlan::idle();
    }
    let requested = scan_pending || menu_just_opened || elapsed >= DEVICE_SCAN_INTERVAL;
    match (scan_running, requested) {
        (true, true) => DeviceScanPlan::queued(),
        (false, true) => DeviceScanPlan::start(),
        _ => DeviceScanPlan::idle(),
    }
}

pub fn run() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([640.0, 520.0])
            .with_min_inner_size([560.0, 480.0])
            .with_drag_and_drop(true),
        ..Default::default()
    };
    eframe::run_native(
        "HAP 重签名工具",
        options,
        Box::new(|context| Ok(Box::new(HapResignerApp::new(context)))),
    )
}

#[derive(Clone, Copy)]
struct Palette {
    background: egui::Color32,
    surface: egui::Color32,
    surface_alt: egui::Color32,
    border: egui::Color32,
    text: egui::Color32,
    muted: egui::Color32,
    accent: egui::Color32,
    accent_soft: egui::Color32,
    success: egui::Color32,
    warning: egui::Color32,
    error: egui::Color32,
}

impl Palette {
    fn for_theme(theme: egui::Theme) -> Self {
        match theme {
            egui::Theme::Light => Self {
                background: egui::Color32::from_rgb(247, 249, 252),
                surface: egui::Color32::WHITE,
                surface_alt: egui::Color32::from_rgb(239, 244, 252),
                border: egui::Color32::from_rgb(215, 222, 232),
                text: egui::Color32::from_rgb(25, 33, 45),
                muted: egui::Color32::from_rgb(100, 112, 129),
                accent: egui::Color32::from_rgb(45, 105, 210),
                accent_soft: egui::Color32::from_rgb(229, 238, 255),
                success: egui::Color32::from_rgb(35, 145, 88),
                warning: egui::Color32::from_rgb(190, 125, 30),
                error: egui::Color32::from_rgb(196, 52, 68),
            },
            egui::Theme::Dark => Self {
                background: egui::Color32::from_rgb(19, 23, 31),
                surface: egui::Color32::from_rgb(29, 35, 46),
                surface_alt: egui::Color32::from_rgb(35, 43, 57),
                border: egui::Color32::from_rgb(59, 70, 88),
                text: egui::Color32::from_rgb(235, 239, 246),
                muted: egui::Color32::from_rgb(163, 174, 191),
                accent: egui::Color32::from_rgb(102, 157, 255),
                accent_soft: egui::Color32::from_rgb(41, 58, 86),
                success: egui::Color32::from_rgb(74, 194, 126),
                warning: egui::Color32::from_rgb(232, 171, 70),
                error: egui::Color32::from_rgb(246, 103, 119),
            },
        }
    }
}

struct HapResignerApp {
    state: AppState,
    selected_hap: Option<PathBuf>,
    operation_device: Option<DeviceInfo>,
    remembered_device_udid: Option<String>,
    device_scan_running: bool,
    device_scan_pending: bool,
    last_device_scan_started: Instant,
    install_started: bool,
    announced_ready: bool,
    events_tx: Sender<WorkflowEvent>,
    events_rx: Receiver<WorkflowEvent>,
}

impl HapResignerApp {
    fn new(context: &eframe::CreationContext<'_>) -> Self {
        install_cjk_font(&context.egui_ctx);
        context.egui_ctx.set_theme(egui::ThemePreference::System);
        let remembered_device_udid = context
            .storage
            .and_then(|storage| storage.get_string(SELECTED_DEVICE_UDID_KEY));
        let (events_tx, events_rx) = channel();
        let device_scan_started = Instant::now();
        spawn_device_scan(events_tx.clone());
        Self {
            state: AppState::default(),
            selected_hap: None,
            operation_device: None,
            remembered_device_udid,
            device_scan_running: true,
            device_scan_pending: false,
            last_device_scan_started: device_scan_started,
            install_started: false,
            announced_ready: false,
            events_tx,
            events_rx,
        }
    }

    fn handle_event(&mut self, event: WorkflowEvent) {
        match event {
            WorkflowEvent::Devices(devices) => {
                let current_udid = self
                    .state
                    .device
                    .as_ref()
                    .map(|device| device.udid.as_str());
                let selected = resolve_selected_device(
                    &devices,
                    current_udid,
                    self.remembered_device_udid.as_deref(),
                );
                self.state.apply(WorkflowEvent::Devices(devices));
                self.state.device = selected;
                self.device_scan_running = false;
                self.request_device_scan(false);
            }
            WorkflowEvent::DeviceScanFailed(error) => {
                self.state.apply(WorkflowEvent::DeviceScanFailed(error));
                self.device_scan_running = false;
                self.request_device_scan(false);
            }
            event => self.state.apply(event),
        }
    }

    fn request_device_scan(&mut self, menu_just_opened: bool) {
        let plan = device_scan_plan(
            self.state.phase,
            self.device_scan_running,
            self.device_scan_pending,
            self.last_device_scan_started.elapsed(),
            menu_just_opened,
        );
        self.device_scan_pending = plan.pending;
        if plan.start {
            self.device_scan_running = true;
            self.last_device_scan_started = Instant::now();
            spawn_device_scan(self.events_tx.clone());
        }
    }

    fn start_resign(&mut self, path: PathBuf) {
        if !self.state.can_start() {
            return;
        }
        let Some(device) = active_device_or_fail(&mut self.state) else {
            return;
        };
        self.selected_hap = Some(path.clone());
        self.operation_device = Some(device.clone());
        self.install_started = false;
        self.state.logs.clear();
        self.state.apply(WorkflowEvent::Phase(AppPhase::Inspecting));
        self.state.signed_hap = None;
        self.state.bundle_name = None;
        self.state.ability = None;
        spawn_resign(path, device, self.events_tx.clone());
    }

    fn choose_hap(&mut self) {
        if self.state.device.is_none() {
            active_device_or_fail(&mut self.state);
            return;
        }
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("HarmonyOS HAP", &["hap"])
            .pick_file()
        {
            self.start_resign(path);
        }
    }

    fn start_install(&mut self) {
        if self.install_started || !self.state.can_install() {
            return;
        }
        let (Some(hap), Some(bundle), Some(ability), Some(device)) = (
            self.state.signed_hap.clone(),
            self.state.bundle_name.clone(),
            self.state.ability.clone(),
            self.operation_device.clone(),
        ) else {
            self.state
                .apply(WorkflowEvent::Failed("请先在顶栏选择设备".to_owned()));
            return;
        };
        self.install_started = true;
        spawn_install(hap, bundle, ability, device, self.events_tx.clone());
    }

    fn retry(&mut self) {
        if self.state.device.is_none() {
            active_device_or_fail(&mut self.state);
            return;
        }
        if let Some(path) = self.selected_hap.clone() {
            self.start_resign(path);
        } else {
            self.choose_hap();
        }
    }

    fn handle_dropped_files(&mut self, context: &egui::Context) {
        let files = context.input(|input| input.raw.dropped_files.clone());
        if let Some(path) = files
            .into_iter()
            .map(|file| file.path().to_path_buf())
            .find(|path| {
                path.extension()
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("hap"))
            })
        {
            self.start_resign(path);
        }
    }
}

impl eframe::App for HapResignerApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        if !self.announced_ready {
            eprintln!("GUI_READY: HAP 重签名工具");
            self.announced_ready = true;
        }
        while let Ok(event) = self.events_rx.try_recv() {
            self.handle_event(event);
        }
        self.request_device_scan(false);
        if should_auto_install(self.state.phase, self.install_started) {
            self.start_install();
        }

        let context = ui.ctx().clone();
        self.handle_dropped_files(&context);
        context.request_repaint_after(Duration::from_millis(100));

        let palette = Palette::for_theme(context.theme());
        ui.painter()
            .rect_filled(ui.max_rect(), 0.0, palette.background);
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                egui::Frame::new()
                    .inner_margin(egui::Margin::symmetric(28, 24))
                    .show(ui, |ui| {
                        header(ui, self, palette, &context);
                        ui.add_space(20.0);
                        main_card(ui, self, palette);
                        if !self.state.logs.is_empty() {
                            ui.add_space(10.0);
                            problem_details(ui, &self.state, palette);
                        }
                    });
            });
        paint_footer(ui, palette);
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        save_selected_device(storage, &self.state, self.remembered_device_udid.as_deref());
    }
}

fn header(ui: &mut egui::Ui, app: &mut HapResignerApp, palette: Palette, context: &egui::Context) {
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new("HAP 重签名")
                .size(22.0)
                .strong()
                .color(palette.text),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let controls_enabled = device_controls_enabled(app.state.phase);
            let devices = app.state.devices.clone();
            let mut selected_udid = app.state.device.as_ref().map(|device| device.udid.clone());
            let selector_button = egui::Button::new(device_capsule_job(app, palette))
                .min_size(egui::vec2(220.0, 48.0))
                .corner_radius(14)
                .wrap_mode(egui::TextWrapMode::Extend);
            let (selector_response, _) = ui
                .add_enabled_ui(controls_enabled, |ui| {
                    configure_device_button_visuals(ui, palette);
                    egui::containers::menu::MenuButton::from_button(selector_button).ui(ui, |ui| {
                        configure_device_button_visuals(ui, palette);
                        ui.set_min_width(232.0);
                        if devices.is_empty() {
                            ui.add_space(6.0);
                            ui.label(egui::RichText::new("等待设备").color(palette.muted).small());
                            ui.add_space(6.0);
                        } else {
                            for device in &devices {
                                let selected =
                                    selected_udid.as_deref() == Some(device.udid.as_str());
                                let mut row =
                                    egui::Button::new(device_menu_row_job(device, palette))
                                        .min_size(egui::vec2(232.0, 44.0))
                                        .corner_radius(10)
                                        .selected(selected)
                                        .wrap_mode(egui::TextWrapMode::Extend);
                                if selected {
                                    row = row
                                        .fill(palette.accent_soft)
                                        .stroke(egui::Stroke::new(1.0, palette.accent));
                                }
                                let row_response = ui.add(row);
                                paint_device_menu_row_icons(ui, &row_response, selected, palette);
                                if row_response.clicked() {
                                    selected_udid = Some(device.udid.clone());
                                    ui.close();
                                }
                            }
                        }
                    })
                })
                .inner;
            paint_device_capsule_icons(
                ui,
                &selector_response,
                device_capsule_dot_color(app, palette),
                palette,
            );

            if let Some(error) = &app.state.device_scan_error {
                selector_response.clone().on_hover_text(error);
            }
            let menu_just_opened = selector_response.clicked()
                && egui::Popup::is_id_open(
                    context,
                    egui::Popup::default_response_id(&selector_response),
                );
            app.request_device_scan(menu_just_opened);

            if selected_udid.as_deref()
                != app.state.device.as_ref().map(|device| device.udid.as_str())
            {
                app.remembered_device_udid = selected_udid.clone();
                app.state.device = selected_udid
                    .and_then(|udid| devices.into_iter().find(|device| device.udid == udid));
            }
        });
    });
}

fn configure_device_button_visuals(ui: &mut egui::Ui, palette: Palette) {
    ui.spacing_mut().button_padding.x = 32.0;
    let widgets = &mut ui.visuals_mut().widgets;
    widgets.inactive.weak_bg_fill = palette.surface;
    widgets.inactive.bg_stroke = egui::Stroke::new(1.0, palette.border);
    widgets.hovered.weak_bg_fill = palette.surface_alt;
    widgets.hovered.bg_stroke = egui::Stroke::new(1.0, palette.accent);
    widgets.active.weak_bg_fill = palette.accent_soft;
    widgets.active.bg_stroke = egui::Stroke::new(1.0, palette.accent);
    widgets.open.weak_bg_fill = palette.surface_alt;
    widgets.open.bg_stroke = egui::Stroke::new(1.0, palette.accent);
}

fn device_capsule_dot_color(app: &HapResignerApp, palette: Palette) -> egui::Color32 {
    if app.state.device.is_some() || !app.state.devices.is_empty() {
        palette.success
    } else {
        palette.muted
    }
}

fn paint_device_capsule_icons(
    ui: &egui::Ui,
    response: &egui::Response,
    dot_color: egui::Color32,
    palette: Palette,
) {
    let painter = ui.painter();
    let center_y = response.rect.center().y;
    let dot_color = if response.enabled() {
        dot_color
    } else {
        palette.muted
    };
    painter.circle_filled(
        egui::pos2(response.rect.left() + 16.0, center_y),
        4.0,
        dot_color,
    );

    let chevron_color = if response.hovered() && response.enabled() {
        palette.accent
    } else {
        palette.muted
    };
    let chevron_center = egui::pos2(response.rect.right() - 16.0, center_y);
    let chevron_stroke = egui::Stroke::new(1.8, chevron_color);
    painter.line_segment(
        [chevron_center + egui::vec2(-4.0, -2.0), chevron_center],
        chevron_stroke,
    );
    painter.line_segment(
        [chevron_center, chevron_center + egui::vec2(4.0, -2.0)],
        chevron_stroke,
    );
}

fn paint_device_menu_row_icons(
    ui: &egui::Ui,
    response: &egui::Response,
    selected: bool,
    palette: Palette,
) {
    let painter = ui.painter();
    let center_y = response.rect.center().y;
    painter.circle_filled(
        egui::pos2(response.rect.left() + 16.0, center_y),
        3.5,
        palette.success,
    );

    if selected {
        let first = egui::pos2(response.rect.right() - 23.0, center_y);
        let middle = first + egui::vec2(4.0, 4.0);
        let last = middle + egui::vec2(8.0, -9.0);
        let stroke = egui::Stroke::new(2.0, palette.accent);
        painter.line_segment([first, middle], stroke);
        painter.line_segment([middle, last], stroke);
    }
}

fn short_device_target(target: &str) -> Cow<'_, str> {
    const MAX_CHARS: usize = 23;
    const HEAD_CHARS: usize = 12;
    const TAIL_CHARS: usize = 10;

    if target.chars().count() <= MAX_CHARS {
        return Cow::Borrowed(target);
    }
    let head_end = target
        .char_indices()
        .nth(HEAD_CHARS)
        .map_or(target.len(), |(index, _)| index);
    let tail_start = target
        .char_indices()
        .rev()
        .nth(TAIL_CHARS - 1)
        .map_or(0, |(index, _)| index);
    Cow::Owned(format!("{}…{}", &target[..head_end], &target[tail_start..]))
}

fn device_capsule_job(app: &HapResignerApp, palette: Palette) -> egui::text::LayoutJob {
    let (title, subtitle) = if let Some(device) = app.state.device.as_ref() {
        (device.model.as_str(), short_device_target(&device.target))
    } else if app.state.devices.is_empty() {
        (
            "等待设备",
            Cow::Borrowed(if app.device_scan_running {
                "正在扫描可用设备"
            } else {
                "连接后将自动发现"
            }),
        )
    } else {
        ("选择设备", Cow::Borrowed("请选择一台在线设备"))
    };
    device_text_job(title, &subtitle, palette)
}

fn device_menu_row_job(device: &DeviceInfo, palette: Palette) -> egui::text::LayoutJob {
    let target = short_device_target(&device.target);
    device_text_job(&device.model, &target, palette)
}

fn device_text_job(title: &str, subtitle: &str, palette: Palette) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::default();
    job.append(
        title,
        0.0,
        egui::TextFormat {
            font_id: egui::FontId::proportional(13.5),
            color: palette.text,
            ..Default::default()
        },
    );
    let subtitle_format = egui::TextFormat {
        font_id: egui::FontId::proportional(11.0),
        color: palette.muted,
        ..Default::default()
    };
    job.append("\n", 0.0, subtitle_format.clone());
    job.append(subtitle, 0.0, subtitle_format);
    job
}

fn footer_anchor(window_rect: egui::Rect) -> egui::Pos2 {
    egui::pos2(window_rect.right() - 16.0, window_rect.bottom() - 12.0)
}

fn paint_footer(ui: &egui::Ui, palette: Palette) {
    ui.painter().text(
        footer_anchor(ui.max_rect()),
        egui::Align2::RIGHT_BOTTOM,
        version_label(),
        egui::FontId::proportional(10.0),
        palette.muted.gamma_multiply(0.72),
    );
}

fn main_card(ui: &mut egui::Ui, app: &mut HapResignerApp, palette: Palette) {
    let phase = app.state.phase;
    let (fill, stroke) = match phase {
        AppPhase::Done => (palette.success.gamma_multiply(0.10), palette.success),
        AppPhase::Error => (palette.error.gamma_multiply(0.09), palette.error),
        AppPhase::Idle => (palette.accent_soft, palette.accent),
        _ => (palette.surface, palette.border),
    };
    egui::Frame::new()
        .fill(fill)
        .stroke(egui::Stroke::new(1.2, stroke.gamma_multiply(0.75)))
        .corner_radius(15)
        .inner_margin(egui::Margin::symmetric(28, 24))
        .show(ui, |ui| {
            ui.set_min_height(245.0);
            ui.vertical_centered(|ui| match phase {
                AppPhase::Idle => idle_content(ui, app, palette),
                AppPhase::WaitingForLogin => waiting_for_login_content(ui, app, palette),
                AppPhase::Done => done_content(ui, app, palette),
                AppPhase::Error => error_content(ui, app, palette),
                _ => busy_content(ui, app, palette),
            });
        });
}

fn idle_content(ui: &mut egui::Ui, app: &mut HapResignerApp, palette: Palette) {
    ui.add_space(28.0);
    ui.label(
        egui::RichText::new("把 HAP 拖到这里")
            .size(25.0)
            .strong()
            .color(palette.text),
    );
    ui.add_space(5.0);
    ui.label(
        egui::RichText::new("剩下的交给我")
            .size(15.0)
            .color(palette.muted),
    );
    ui.add_space(22.0);
    if ui
        .add(
            egui::Button::new(
                egui::RichText::new("选择 HAP")
                    .strong()
                    .color(egui::Color32::WHITE),
            )
            .fill(palette.accent)
            .stroke(egui::Stroke::NONE)
            .corner_radius(9)
            .min_size(egui::vec2(150.0, 42.0)),
        )
        .clicked()
    {
        app.choose_hap();
    }
}

fn busy_content(ui: &mut egui::Ui, app: &HapResignerApp, palette: Palette) {
    ui.add_space(26.0);
    ui.spinner();
    ui.add_space(14.0);
    ui.label(
        egui::RichText::new(friendly_phase(app.state.phase))
            .size(21.0)
            .strong()
            .color(palette.text),
    );
    ui.add_space(6.0);
    if let Some(name) = selected_file_name(&app.selected_hap) {
        ui.label(egui::RichText::new(name).color(palette.muted));
    }
    ui.add_space(20.0);
    ui.add(
        egui::ProgressBar::new(progress(app.state.phase))
            .desired_width(360.0)
            .fill(palette.accent)
            .corner_radius(6)
            .animate(true),
    );
    ui.add_space(8.0);
    ui.small(egui::RichText::new("请保持设备连接").color(palette.muted));
}

fn waiting_for_login_content(ui: &mut egui::Ui, app: &HapResignerApp, palette: Palette) {
    ui.add_space(26.0);
    ui.spinner();
    ui.add_space(14.0);
    ui.label(
        egui::RichText::new("请在浏览器完成华为账号登录")
            .size(21.0)
            .strong()
            .color(palette.text),
    );
    ui.add_space(7.0);
    ui.label(egui::RichText::new("登录成功后会自动继续，请不要关闭此窗口").color(palette.muted));
    if let Some(name) = selected_file_name(&app.selected_hap) {
        ui.add_space(7.0);
        ui.label(egui::RichText::new(name).small().color(palette.muted));
    }
}

fn done_content(ui: &mut egui::Ui, app: &mut HapResignerApp, palette: Palette) {
    ui.add_space(26.0);
    ui.label(
        egui::RichText::new("已安装并启动")
            .size(25.0)
            .strong()
            .color(palette.success),
    );
    ui.add_space(7.0);
    if let Some(name) = selected_file_name(&app.selected_hap) {
        ui.label(egui::RichText::new(name).color(palette.muted));
    }
    ui.add_space(22.0);
    if ui
        .add(
            egui::Button::new("再处理一个")
                .fill(palette.surface)
                .corner_radius(9)
                .min_size(egui::vec2(138.0, 40.0)),
        )
        .clicked()
    {
        app.choose_hap();
    }
}

fn error_content(ui: &mut egui::Ui, app: &mut HapResignerApp, palette: Palette) {
    ui.add_space(12.0);
    ui.label(
        egui::RichText::new(failure_title(app.state.failed_phase))
            .size(24.0)
            .strong()
            .color(palette.error),
    );
    ui.add_space(8.0);
    egui::ScrollArea::vertical()
        .max_height(92.0)
        .auto_shrink([false, true])
        .show(ui, |ui| {
            ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(error_body(&app.state))
                            .monospace()
                            .color(palette.text),
                    )
                    .selectable(true)
                    .wrap(),
                );
            });
        });
    let certificate_limit = app
        .state
        .error
        .as_deref()
        .is_some_and(needs_certificate_management);
    if certificate_limit {
        ui.add_space(8.0);
        ui.label(
            egui::RichText::new("调试证书数量已达上限，请先在 AGC 废除不再使用的历史证书")
                .color(palette.muted),
        );
        ui.hyperlink_to("前往 AGC 管理证书", CERTIFICATE_MANAGEMENT_URL);
    }
    ui.add_space(14.0);
    ui.horizontal(|ui| {
        if ui
            .add(
                egui::Button::new(
                    egui::RichText::new("重试")
                        .strong()
                        .color(egui::Color32::WHITE),
                )
                .fill(palette.accent)
                .stroke(egui::Stroke::NONE)
                .corner_radius(9)
                .min_size(egui::vec2(120.0, 40.0)),
            )
            .clicked()
        {
            app.retry();
        }
        if ui
            .add(
                egui::Button::new("换一个 HAP")
                    .fill(palette.surface)
                    .corner_radius(9)
                    .min_size(egui::vec2(120.0, 40.0)),
            )
            .clicked()
        {
            app.choose_hap();
        }
    });
}

fn problem_details(ui: &mut egui::Ui, state: &AppState, palette: Palette) {
    egui::CollapsingHeader::new("问题详情")
        .default_open(false)
        .show(ui, |ui| {
            egui::Frame::new()
                .fill(palette.surface)
                .stroke(egui::Stroke::new(1.0, palette.border))
                .corner_radius(8)
                .inner_margin(egui::Margin::same(10))
                .show(ui, |ui| {
                    egui::ScrollArea::vertical()
                        .max_height(130.0)
                        .stick_to_bottom(true)
                        .show(ui, |ui| {
                            for entry in &state.logs {
                                ui.colored_label(log_color(entry.level, palette), &entry.message);
                            }
                        });
                });
        });
}

fn should_auto_install(phase: AppPhase, install_started: bool) -> bool {
    phase == AppPhase::ReadyToInstall && !install_started
}

fn resolve_selected_device(
    devices: &[DeviceInfo],
    current_udid: Option<&str>,
    remembered_udid: Option<&str>,
) -> Option<DeviceInfo> {
    if let [device] = devices {
        return Some(device.clone());
    }
    current_udid
        .and_then(|udid| devices.iter().find(|device| device.udid == udid))
        .or_else(|| {
            remembered_udid.and_then(|udid| devices.iter().find(|device| device.udid == udid))
        })
        .cloned()
}

fn active_device_or_fail(state: &mut AppState) -> Option<DeviceInfo> {
    if let Some(device) = state.device.clone() {
        return Some(device);
    }
    state.apply(WorkflowEvent::Failed("请先在顶栏选择设备".to_owned()));
    None
}

fn save_selected_device(
    storage: &mut dyn eframe::Storage,
    state: &AppState,
    remembered_udid: Option<&str>,
) {
    let udid = state
        .device
        .as_ref()
        .map(|device| device.udid.as_str())
        .or(remembered_udid);
    if let Some(udid) = udid {
        storage.set_string(SELECTED_DEVICE_UDID_KEY, udid.to_owned());
    } else {
        storage.remove_string(SELECTED_DEVICE_UDID_KEY);
    }
}

fn device_controls_enabled(phase: AppPhase) -> bool {
    matches!(phase, AppPhase::Idle | AppPhase::Done | AppPhase::Error)
}

fn version_label() -> &'static str {
    concat!("v", env!("CARGO_PKG_VERSION"))
}

fn failure_title(failed_phase: Option<AppPhase>) -> &'static str {
    match failed_phase {
        Some(AppPhase::Installing) => "安装失败",
        Some(AppPhase::Authenticating | AppPhase::WaitingForLogin) => "认证失败",
        _ => "处理失败",
    }
}

fn error_body(state: &AppState) -> &str {
    state.error.as_deref().unwrap_or("")
}
fn needs_certificate_management(error: &str) -> bool {
    error.contains("CERTIFICATE_LIMIT_REACHED")
}

fn selected_file_name(path: &Option<PathBuf>) -> Option<String> {
    path.as_ref()
        .and_then(|path| path.file_name())
        .and_then(|name| name.to_str())
        .map(ToOwned::to_owned)
}

fn friendly_phase(phase: AppPhase) -> &'static str {
    match phase {
        AppPhase::Inspecting => "正在检查 HAP",
        AppPhase::Authenticating => "正在登录开发者账号",
        AppPhase::WaitingForLogin => "等待浏览器登录",
        AppPhase::PreparingMaterials => "正在准备签名",
        AppPhase::Signing | AppPhase::ReadyToInstall => "正在重签名",
        AppPhase::Installing => "正在安装并启动",
        _ => "正在处理",
    }
}

fn progress(phase: AppPhase) -> f32 {
    match phase {
        AppPhase::Idle => 0.0,
        AppPhase::Inspecting => 0.15,
        AppPhase::Authenticating => 0.32,
        AppPhase::WaitingForLogin => 0.4,
        AppPhase::PreparingMaterials => 0.48,
        AppPhase::Signing => 0.68,
        AppPhase::ReadyToInstall => 0.78,
        AppPhase::Installing => 0.9,
        AppPhase::Done => 1.0,
        AppPhase::Error => 0.0,
    }
}

fn log_color(level: LogLevel, palette: Palette) -> egui::Color32 {
    match level {
        LogLevel::Info => palette.accent,
        LogLevel::Success => palette.success,
        LogLevel::Warning => palette.warning,
        LogLevel::Error => palette.error,
    }
}

fn install_cjk_font(context: &egui::Context) {
    let candidates: &[&str] = if cfg!(target_os = "macos") {
        &[
            "/System/Library/Fonts/STHeiti Medium.ttc",
            "/System/Library/Fonts/STHeiti Light.ttc",
            "/System/Library/Fonts/Hiragino Sans GB.ttc",
        ]
    } else if cfg!(target_os = "windows") {
        &[
            r"C:\Windows\Fonts\msyh.ttc",
            r"C:\Windows\Fonts\msyhbd.ttc",
            r"C:\Windows\Fonts\simhei.ttf",
            r"C:\Windows\Fonts\simsun.ttc",
        ]
    } else {
        &["/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc"]
    };
    let Some((path, bytes)) = candidates
        .iter()
        .find_map(|path| std::fs::read(path).ok().map(|bytes| (*path, bytes)))
    else {
        eprintln!("CJK_FONT_MISSING");
        return;
    };
    eprintln!("CJK_FONT_READY: {path}");
    let mut fonts = egui::FontDefinitions::default();
    let mut font = egui::FontData::from_owned(bytes);
    font.index = 0;
    fonts.font_data.insert("system-cjk".to_owned(), font.into());
    for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
        fonts
            .families
            .entry(family)
            .or_default()
            .push("system-cjk".to_owned());
    }
    context.set_fonts(fonts);
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::time::Duration;

    use eframe::Storage;

    use super::{
        AppPhase, AppState, DeviceInfo, DeviceScanPlan, SELECTED_DEVICE_UDID_KEY,
        active_device_or_fail, device_scan_plan, error_body, failure_title, footer_anchor,
        install_cjk_font, needs_certificate_management, resolve_selected_device,
        save_selected_device, short_device_target, should_auto_install, version_label,
    };

    #[test]
    fn ready_hap_triggers_install_exactly_once() {
        assert!(should_auto_install(AppPhase::ReadyToInstall, false));
        assert!(!should_auto_install(AppPhase::ReadyToInstall, true));
        assert!(!should_auto_install(AppPhase::Signing, false));
    }
    #[test]
    fn periodic_device_scan_starts_after_three_seconds() {
        assert_eq!(
            device_scan_plan(
                AppPhase::Idle,
                false,
                false,
                Duration::from_millis(2_999),
                false,
            ),
            DeviceScanPlan::idle(),
        );
        assert_eq!(
            device_scan_plan(AppPhase::Idle, false, false, Duration::from_secs(3), false,),
            DeviceScanPlan::start(),
        );
    }

    #[test]
    fn processing_phase_pauses_periodic_and_menu_scans() {
        assert_eq!(
            device_scan_plan(
                AppPhase::Signing,
                false,
                false,
                Duration::from_secs(3),
                true,
            ),
            DeviceScanPlan::idle(),
        );
    }

    #[test]
    fn opening_device_menu_requests_an_immediate_scan() {
        assert_eq!(
            device_scan_plan(AppPhase::Done, false, false, Duration::ZERO, true,),
            DeviceScanPlan::start(),
        );
    }

    #[test]
    fn in_flight_scan_requests_are_coalesced_and_started_once_after_completion() {
        let queued = device_scan_plan(AppPhase::Error, true, false, Duration::from_secs(3), false);
        assert_eq!(queued, DeviceScanPlan::queued());
        assert_eq!(
            device_scan_plan(
                AppPhase::Error,
                true,
                queued.pending,
                Duration::from_secs(4),
                true,
            ),
            DeviceScanPlan::queued(),
        );
        assert_eq!(
            device_scan_plan(
                AppPhase::Error,
                false,
                queued.pending,
                Duration::ZERO,
                false,
            ),
            DeviceScanPlan::start(),
        );
    }

    #[test]
    fn footer_anchor_keeps_fixed_right_and_bottom_window_margins() {
        let window = eframe::egui::Rect::from_min_size(
            eframe::egui::pos2(20.0, 30.0),
            eframe::egui::vec2(640.0, 520.0),
        );
        let anchor = footer_anchor(window);

        assert_eq!(window.right() - anchor.x, 16.0);
        assert_eq!(window.bottom() - anchor.y, 12.0);
    }

    #[test]
    fn single_device_is_selected_automatically() {
        let only = device("target-1", "udid-1", "model-1");

        assert_eq!(
            resolve_selected_device(&[only.clone()], None, None),
            Some(only)
        );
    }

    #[test]
    fn multiple_devices_restore_the_persisted_udid() {
        let first = device("target-1", "udid-1", "model-1");
        let remembered = device("target-2", "udid-2", "model-2");

        assert_eq!(
            resolve_selected_device(&[first, remembered.clone()], None, Some("udid-2")),
            Some(remembered)
        );
    }

    #[test]
    fn multiple_devices_stay_unselected_when_the_remembered_device_is_offline() {
        let devices = [
            device("target-1", "udid-1", "model-1"),
            device("target-2", "udid-2", "model-2"),
        ];

        assert_eq!(
            resolve_selected_device(&devices, None, Some("offline-udid")),
            None
        );
    }

    #[test]
    fn rescanning_keeps_the_current_device_when_it_is_still_online() {
        let current = device("target-1-new", "udid-1", "model-1");
        let devices = [current.clone(), device("target-2", "udid-2", "model-2")];

        assert_eq!(
            resolve_selected_device(&devices, Some("udid-1"), Some("udid-2")),
            Some(current)
        );
    }
    #[test]
    fn long_device_target_is_shortened_without_losing_both_ends() {
        assert_eq!(
            short_device_target("192.168.100.123:55555-very-long-target"),
            "192.168.100.…ong-target",
        );
        assert_eq!(short_device_target("USB-1234"), "USB-1234");
    }

    #[test]
    fn missing_device_fails_before_an_operation_can_start() {
        let mut state = AppState::default();

        assert_eq!(active_device_or_fail(&mut state), None);
        assert_eq!(state.phase, AppPhase::Error);
        assert_eq!(state.failed_phase, Some(AppPhase::Idle));
        assert_eq!(state.error.as_deref(), Some("请先在顶栏选择设备"));
    }

    #[test]
    fn selected_device_storage_contains_only_the_udid() {
        let mut storage = MemoryStorage::default();
        let mut state = AppState::default();
        state.device = Some(device("network-target", "persisted-udid", "Secret Model"));

        save_selected_device(&mut storage, &state, None);

        assert_eq!(
            storage.get_string(SELECTED_DEVICE_UDID_KEY).as_deref(),
            Some("persisted-udid")
        );
        assert_eq!(storage.values.len(), 1);
        assert!(
            !storage.values.values().any(|value| {
                value.contains("network-target") || value.contains("Secret Model")
            })
        );
    }

    #[test]
    fn offline_selected_device_keeps_the_remembered_udid() {
        let mut storage = MemoryStorage::default();
        let state = AppState::default();

        save_selected_device(&mut storage, &state, Some("remembered-offline-udid"));

        assert_eq!(
            storage.get_string(SELECTED_DEVICE_UDID_KEY).as_deref(),
            Some("remembered-offline-udid")
        );
    }

    #[test]
    fn version_label_uses_the_cargo_package_version() {
        assert_eq!(version_label(), format!("v{}", env!("CARGO_PKG_VERSION")));
        assert_eq!(version_label(), "v1.0.0");
    }

    #[test]
    fn failure_title_distinguishes_install_authentication_and_processing() {
        assert_eq!(failure_title(Some(AppPhase::Installing)), "安装失败");
        assert_eq!(failure_title(Some(AppPhase::Authenticating)), "认证失败");
        assert_eq!(failure_title(Some(AppPhase::WaitingForLogin)), "认证失败");
        assert_eq!(failure_title(Some(AppPhase::Signing)), "处理失败");
        assert_eq!(failure_title(None), "处理失败");
    }

    #[test]
    fn error_body_preserves_original_case_and_newlines() {
        let mut state = AppState::default();
        state.error = Some("INSTALL_FAILED_VERSION_DOWNGRADE\nPermission denied".to_owned());

        assert_eq!(
            error_body(&state),
            "INSTALL_FAILED_VERSION_DOWNGRADE\nPermission denied"
        );
    }

    #[test]
    fn certificate_limit_error_enables_the_management_entry() {
        assert!(needs_certificate_management(
            "remote material failed: CERTIFICATE_LIMIT_REACHED: limit"
        ));
        assert!(!needs_certificate_management("ordinary network failure"));
    }

    #[test]
    fn selected_system_font_covers_chinese_ui_glyphs() {
        let context = eframe::egui::Context::default();
        install_cjk_font(&context);
        let mut covered = false;
        let mut output = context.run_ui(Default::default(), |ui| {
            ui.fonts_mut(|fonts| {
                covered = fonts.has_glyphs(
                    &eframe::egui::FontId::proportional(16.0),
                    "重签名工具拖入选择设备自动安装启动问题详情重试证书数量上限前往管理废除历史浏览器华为账号登录成功继续关闭窗口顶栏认证处理失败扫描中换一个等待设备正在扫描可用设备连接后将自动发现请选择一台在线设备",
                );
            });
        });
        output.textures_delta.clear();
        assert!(covered, "selected CJK font does not cover the Chinese UI");
    }

    fn device(target: &str, udid: &str, model: &str) -> DeviceInfo {
        DeviceInfo {
            target: target.to_owned(),
            udid: udid.to_owned(),
            model: model.to_owned(),
        }
    }

    #[derive(Default)]
    struct MemoryStorage {
        values: HashMap<String, String>,
    }

    impl Storage for MemoryStorage {
        fn get_string(&self, key: &str) -> Option<String> {
            self.values.get(key).cloned()
        }

        fn set_string(&mut self, key: &str, value: String) {
            self.values.insert(key.to_owned(), value);
        }

        fn remove_string(&mut self, key: &str) {
            self.values.remove(key);
        }

        fn flush(&mut self) {}
    }
}
