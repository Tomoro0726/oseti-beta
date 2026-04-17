//! Oseti Beta - マルチビューカメラスイッチャー
//!
//! 複数のカメラ入力を受け付け、柔軟なグリッドレイアウトでマルチビュー出力する
//! OBS風のリアルタイムカメラアプリケーション。
//!
//! # 機能
//!
//! - **自動レイアウト**: カメラ数に応じた自動レイアウト選択
//! - **アスペクト比保持**: 1920×1080前提、異なる解像度は16:9でクロップ
//! - **OBS風UI**: 上段プレビュー＆プログラム、下段マルチビュー＆コントロール
//!
//! # アーキテクチャ
//!
//! - `camera`: 複数カメラのライフサイクル管理
//! - `layout`: マルチビューレイアウト設定（自動選択対応）
//! - `renderer`: グリッドレイアウトでのレンダリング（アスペクト比対応）

mod camera;
mod layout;
mod renderer;

use camera::{CameraId, CameraManager};
use eframe::egui;
use layout::{LayoutConfig, LayoutType};
use renderer::{FrameData, MultiViewRenderer};
use std::collections::HashMap;

const INITIAL_WIDTH: usize = 1280;
const INITIAL_HEIGHT: usize = 720;

/// メインアプリケーション状態
struct CameraApp {
    /// カメラ管理
    camera_manager: CameraManager,
    /// マルチビューレイアウト設定（プレビュー用）
    layout_config: LayoutConfig,
    /// マルチビューレンダラー（プレビュー用）
    preview_renderer: MultiViewRenderer,
    /// プログラム用レンダラー（選択カメラ表示）
    program_renderer: MultiViewRenderer,
    /// マルチビュー用レンダラー（下段グリッド）
    multiview_renderer: MultiViewRenderer,
    /// プレビュー表示用テクスチャ
    preview_texture: Option<egui::TextureHandle>,
    /// プログラム表示用テクスチャ
    program_texture: Option<egui::TextureHandle>,
    /// マルチビュー表示用テクスチャ
    multiview_texture: Option<egui::TextureHandle>,
    /// 選択中のカメラID
    selected_camera_id: Option<CameraId>,
    /// プレビュー中のカメラID
    preview_camera_id: Option<CameraId>,
    /// カメラごとの最新エラー
    camera_errors: HashMap<CameraId, String>,
}

impl CameraApp {
    /// 利用可能領域に16:9キャンバスを収める
    fn fit_canvas_size(available: egui::Vec2) -> (usize, usize) {
        let target_aspect = 16.0f32 / 9.0f32;

        let mut width = available.x.max(16.0);
        let mut height = available.y.max(16.0);

        if width / height > target_aspect {
            width = height * target_aspect;
        } else {
            height = width / target_aspect;
        }

        // 下段4x2分割・上段2分割で端数が出ないように丸める
        let width_px = ((width.floor() as usize).max(16) / 4) * 4;
        let height_px = ((height.floor() as usize).max(16) / 2) * 2;

        (width_px, height_px)
    }

    /// アプリケーションを初期化
    fn new(_ctx: &egui::Context) -> Self {
        nokhwa::nokhwa_initialize(|_| {});

        let mut camera_manager = CameraManager::new();
        // 入力は常に8枠（4x2）固定
        let input_layout_type = LayoutType::Inputs4x2;
        let mut layout_config = LayoutConfig::new(input_layout_type);

        // すべてのカメラを割り当て
        let available_cameras: Vec<_> = camera_manager.available_cameras().to_vec();
        let mut camera_errors = HashMap::new();
        for (i, camera_info) in available_cameras.iter().enumerate() {
            if i < layout_config.view_count() {
                match camera_manager.open_camera(camera_info.id) {
                    Ok(_) => {
                        layout_config.assign_camera(i, Some(camera_info.id));
                    }
                    Err(e) => {
                        camera_errors.insert(camera_info.id, format!("open failed: {}", e));
                        layout_config.assign_camera(i, None);
                    }
                }
            }
        }

        let preview_camera_id = available_cameras.first().map(|c| c.id);
        let selected_camera_id = available_cameras.get(1).map(|c| c.id).or(preview_camera_id);

        if let Some(camera_id) = preview_camera_id {
            if let Err(e) = camera_manager.open_camera(camera_id) {
                camera_errors.insert(camera_id, format!("open failed: {}", e));
            }
        }
        if let Some(camera_id) = selected_camera_id {
            if let Err(e) = camera_manager.open_camera(camera_id) {
                camera_errors.insert(camera_id, format!("open failed: {}", e));
            }
        }

        Self {
            camera_manager,
            layout_config,
            preview_renderer: MultiViewRenderer::new(LayoutType::Single),
            program_renderer: MultiViewRenderer::new(LayoutType::Single),
            multiview_renderer: MultiViewRenderer::new(input_layout_type),
            preview_texture: None,
            program_texture: None,
            multiview_texture: None,
            selected_camera_id,
            preview_camera_id,
            camera_errors,
        }
    }

    /// すべてのビューのフレームをキャプチャ
    fn capture_all_frames(&mut self) {
        for view_idx in 0..self.layout_config.view_count() {
            if let Some(view) = self.layout_config.view(view_idx) {
                if let Some(camera_id) = view.camera_id {
                    match self.camera_manager.get_frame(camera_id) {
                        Ok((pixels, width, height)) => {
                            let frame_data = FrameData {
                                pixels,
                                width,
                                height,
                            };
                            // 下段8入力用にキャッシュ
                            self.multiview_renderer.cache_frame(view_idx, frame_data);
                            self.camera_errors.remove(&camera_id);
                        }
                        Err(e) => {
                            self.multiview_renderer.clear_frame(view_idx);
                            self.camera_errors.insert(camera_id, e.clone());
                            eprintln!(
                                "ERROR [View {}]: Failed to capture frame from camera: {}",
                                view_idx, e
                            );
                        }
                    }
                } else {
                    self.multiview_renderer.clear_frame(view_idx);
                }
            }
        }
    }

    /// プレビュー（選択カメラ）のフレームをキャプチャ
    fn capture_preview_frame(&mut self) {
        if let Some(camera_id) = self.preview_camera_id {
            match self.camera_manager.get_frame(camera_id) {
                Ok((pixels, width, height)) => {
                    let frame_data = FrameData {
                        pixels,
                        width,
                        height,
                    };
                    self.preview_renderer.cache_frame(0, frame_data);
                    self.camera_errors.remove(&camera_id);
                }
                Err(e) => {
                    self.preview_renderer.clear_frame(0);
                    self.camera_errors.insert(camera_id, e.clone());
                    eprintln!(
                        "ERROR [Preview Camera]: Failed to capture frame from camera: {}",
                        e
                    );
                }
            }
        } else {
            self.preview_renderer.clear_frame(0);
        }
    }

    /// プログラム（選択カメラ）のフレームをキャプチャ
    fn capture_program_frame(&mut self) {
        if let Some(camera_id) = self.selected_camera_id {
            match self.camera_manager.get_frame(camera_id) {
                Ok((pixels, width, height)) => {
                    let frame_data = FrameData {
                        pixels,
                        width,
                        height,
                    };
                    self.program_renderer.cache_frame(0, frame_data);
                    self.camera_errors.remove(&camera_id);
                }
                Err(e) => {
                    self.program_renderer.clear_frame(0);
                    self.camera_errors.insert(camera_id, e.clone());
                    eprintln!(
                        "ERROR [Program Camera]: Failed to capture frame from camera: {}",
                        e
                    );
                }
            }
        } else {
            self.program_renderer.clear_frame(0);
        }
    }
}

impl eframe::App for CameraApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // フレームをキャプチャ
        self.capture_all_frames();
        self.capture_preview_frame();
        self.capture_program_frame();

        // ===== ボトムパネル（コントロール） =====
        egui::TopBottomPanel::bottom("control_panel").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(format!(
                    "📹 Cameras: {}",
                    self.camera_manager.available_cameras().len()
                ));
                ui.separator();
                ui.label("Layout: Fixed 8 Inputs (2x4)");
                ui.separator();

                // カメラ選択
                let camera_names: Vec<String> = self
                    .camera_manager
                    .available_cameras()
                    .iter()
                    .map(|c| c.name.clone())
                    .collect();

                let selected_idx = self
                    .selected_camera_id
                    .and_then(|id| {
                        self.camera_manager
                            .available_cameras()
                            .iter()
                            .position(|c| c.id == id)
                    })
                    .unwrap_or(0);

                let mut new_selected = selected_idx;
                egui::ComboBox::from_label("Program Camera")
                    .selected_text(camera_names.get(selected_idx).cloned().unwrap_or_default())
                    .show_ui(ui, |ui| {
                        for (i, name) in camera_names.iter().enumerate() {
                            ui.selectable_value(&mut new_selected, i, name);
                        }
                    });

                let preview_idx = self
                    .preview_camera_id
                    .and_then(|id| {
                        self.camera_manager
                            .available_cameras()
                            .iter()
                            .position(|c| c.id == id)
                    })
                    .unwrap_or(0);
                let mut new_preview = preview_idx;
                egui::ComboBox::from_label("Preview Camera")
                    .selected_text(camera_names.get(preview_idx).cloned().unwrap_or_default())
                    .show_ui(ui, |ui| {
                        for (i, name) in camera_names.iter().enumerate() {
                            ui.selectable_value(&mut new_preview, i, name);
                        }
                    });

                if new_selected != selected_idx
                    && new_selected < self.camera_manager.available_cameras().len()
                {
                    let new_camera = &self.camera_manager.available_cameras()[new_selected];
                    let new_id = new_camera.id;
                    match self.camera_manager.open_camera(new_id) {
                        Ok(_) => {
                            self.selected_camera_id = Some(new_id);
                            self.camera_errors.remove(&new_id);
                        }
                        Err(e) => {
                            self.camera_errors
                                .insert(new_id, format!("open failed: {}", e));
                        }
                    }
                }

                if new_preview != preview_idx
                    && new_preview < self.camera_manager.available_cameras().len()
                {
                    let new_camera = &self.camera_manager.available_cameras()[new_preview];
                    let new_id = new_camera.id;
                    match self.camera_manager.open_camera(new_id) {
                        Ok(_) => {
                            self.preview_camera_id = Some(new_id);
                            self.camera_errors.remove(&new_id);
                        }
                        Err(e) => {
                            self.camera_errors
                                .insert(new_id, format!("open failed: {}", e));
                        }
                    }
                }
            });

            if !self.camera_errors.is_empty() {
                ui.separator();
                ui.colored_label(egui::Color32::YELLOW, "Camera Errors:");
                for camera in self.camera_manager.available_cameras() {
                    if let Some(err) = self.camera_errors.get(&camera.id) {
                        ui.colored_label(egui::Color32::RED, format!("{}: {}", camera.name, err));
                    }
                }
            }
        });

        // ===== トップパネル（プレビュー＆プログラム）=====
        egui::CentralPanel::default().show(ctx, |ui| {
            let available = ui.available_size();
            let (canvas_width, canvas_height) = Self::fit_canvas_size(available);
            let top_height = canvas_height / 2;
            let bottom_height = canvas_height - top_height;
            let top_view_width = canvas_width / 2;

            // テクスチャ初期化（初回のみ、最小1x1）
            if self.preview_texture.is_none() {
                let color_image = egui::ColorImage::from_rgba_unmultiplied([1, 1], &[0, 0, 0, 255]);
                self.preview_texture =
                    Some(ctx.load_texture("preview", color_image, egui::TextureOptions::LINEAR));
            }
            if self.program_texture.is_none() {
                let color_image = egui::ColorImage::from_rgba_unmultiplied([1, 1], &[0, 0, 0, 255]);
                self.program_texture =
                    Some(ctx.load_texture("program", color_image, egui::TextureOptions::LINEAR));
            }
            if self.multiview_texture.is_none() {
                let color_image = egui::ColorImage::from_rgba_unmultiplied([1, 1], &[0, 0, 0, 255]);
                self.multiview_texture =
                    Some(ctx.load_texture("multiview", color_image, egui::TextureOptions::LINEAR));
            }

            let preview_pixels = self.preview_renderer.render(top_view_width, top_height);
            let preview_image = egui::ColorImage::from_rgba_unmultiplied(
                [top_view_width, top_height],
                preview_pixels,
            );
            if let Some(texture) = &mut self.preview_texture {
                texture.set(preview_image, egui::TextureOptions::LINEAR);
            }

            let program_pixels = self.program_renderer.render(top_view_width, top_height);
            let program_image = egui::ColorImage::from_rgba_unmultiplied(
                [top_view_width, top_height],
                program_pixels,
            );
            if let Some(texture) = &mut self.program_texture {
                texture.set(program_image, egui::TextureOptions::LINEAR);
            }

            let multiview_pixels = self.multiview_renderer.render(canvas_width, bottom_height);
            let multiview_image = egui::ColorImage::from_rgba_unmultiplied(
                [canvas_width, bottom_height],
                multiview_pixels,
            );
            if let Some(texture) = &mut self.multiview_texture {
                texture.set(multiview_image, egui::TextureOptions::LINEAR);
            }

            // 画面サイズに応じて中央に収めて表示
            ui.vertical_centered(|ui| {
                // eguiの自動余白（隙間）をゼロにしてピッタリ配置
                ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);

                ui.horizontal_centered(|ui| {
                    if let Some(texture) = &self.preview_texture {
                        ui.image((
                            texture.id(),
                            egui::vec2(top_view_width as f32, top_height as f32),
                        ));
                    }
                    if let Some(texture) = &self.program_texture {
                        ui.image((
                            texture.id(),
                            egui::vec2(top_view_width as f32, top_height as f32),
                        ));
                    }
                });
                if let Some(texture) = &self.multiview_texture {
                    ui.image((
                        texture.id(),
                        egui::vec2(canvas_width as f32, bottom_height as f32),
                    ));
                }
            });
        });

        ctx.request_repaint();
    }
}

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([INITIAL_WIDTH as f32, INITIAL_HEIGHT as f32 + 40.0]),
        ..Default::default()
    };

    eframe::run_native(
        "Oseti Beta - OBS-style Multi-View",
        options,
        Box::new(|cc| Ok(Box::new(CameraApp::new(&cc.egui_ctx)))),
    )
}
