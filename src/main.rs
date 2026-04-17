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

use camera::{CameraId, CameraManager, FrameData};
use eframe::egui;
use layout::{LayoutConfig, LayoutType};
use std::collections::HashMap;

const INITIAL_WIDTH: usize = 1280;
const INITIAL_HEIGHT: usize = 720;

/// 各カメラごとのテクスチャ管理
struct CameraTexture {
    handle: egui::TextureHandle,
    width: u32,
    height: u32,
}

/// メインアプリケーション状態
struct CameraApp {
    /// カメラ管理
    camera_manager: CameraManager,
    /// マルチビューレイアウト設定（プレビュー用）
    layout_config: LayoutConfig,
    /// カメラごとの描画用テクスチャ
    camera_textures: HashMap<CameraId, CameraTexture>,
    /// 選択中のカメラID
    selected_camera_id: Option<CameraId>,
    /// プレビュー中のカメラID
    preview_camera_id: Option<CameraId>,
    /// カメラごとの最新エラー
    camera_errors: HashMap<CameraId, String>,
    /// 入力管理ウィンドウの表示状態
    show_input_settings: bool,
    /// カメラ名などのラベルを表示するかどうか
    show_labels: bool,
}

impl CameraApp {
    /// 利用可能領域に16:9キャンバスを収める
    fn fit_canvas_size(available: egui::Vec2) -> (usize, usize) {
        let target_aspect = 16.0f32 / 9.0f32;

        // 画面ピッタリだとスクロールバーが出現してガタつく原因になるため、2pxの余裕を持たせる
        let mut width = (available.x - 2.0).max(16.0);
        let mut height = (available.y - 2.0).max(16.0);

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
            camera_textures: HashMap::new(),
            selected_camera_id,
            preview_camera_id,
            camera_errors,
            show_input_settings: false,
            show_labels: true,
        }
    }

    /// フレームを受信しテクスチャを更新する
    fn capture_all_frames(&mut self, ctx: &egui::Context) {
        // 表示が必要な全カメラIDをリストアップ
        let mut needed_cameras = std::collections::HashSet::new();

        if let Some(id) = self.preview_camera_id {
            needed_cameras.insert(id);
        }
        if let Some(id) = self.selected_camera_id {
            needed_cameras.insert(id);
        }
        for view_idx in 0..self.layout_config.view_count() {
            if let Some(view) = self.layout_config.view(view_idx) {
                if let Some(id) = view.camera_id {
                    needed_cameras.insert(id);
                }
            }
        }

        for camera_id in needed_cameras {
            match self.camera_manager.get_frame(camera_id) {
                Ok(Some(frame_data)) => {
                    self.camera_errors.remove(&camera_id);
                    let w = frame_data.width as usize;
                    let h = frame_data.height as usize;

                    // Arcに入っている生ピクセルデータをeguiのColorImageに変換
                    // Arcによりメモリコピーは発生しない（eguiロード時にRGBA変換される）
                    let color_image = egui::ColorImage::from_rgb([w, h], &frame_data.pixels);

                    // テクスチャを更新（なければ作成）
                    if let Some(tex) = self.camera_textures.get_mut(&camera_id) {
                        tex.handle.set(color_image, egui::TextureOptions::LINEAR);
                        tex.width = frame_data.width;
                        tex.height = frame_data.height;
                    } else {
                        let name = format!("camera_tex_{}", camera_id.0);
                        let handle =
                            ctx.load_texture(&name, color_image, egui::TextureOptions::LINEAR);
                        self.camera_textures.insert(
                            camera_id,
                            CameraTexture {
                                handle,
                                width: frame_data.width,
                                height: frame_data.height,
                            },
                        );
                    }
                }
                Ok(None) => {
                    // まだ新しいフレームが無いので何もしない (ノンブロッキング)
                }
                Err(e) => {
                    self.camera_errors.insert(camera_id, e);
                }
            }
        }
    }
}

impl eframe::App for CameraApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // フレームをキャプチャ
        self.capture_all_frames(ctx);

        // ===== トップメニューバー =====
        egui::TopBottomPanel::top("menu_bar").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("Exit").clicked() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });

                ui.menu_button("Settings", |ui| {
                    if ui.button("⚙ Manage Inputs").clicked() {
                        self.show_input_settings = !self.show_input_settings;
                    }
                    ui.separator();
                    ui.checkbox(&mut self.show_labels, "Show Labels");
                });

                ui.menu_button("Cameras", |ui| {
                    let camera_names: Vec<String> = self
                        .camera_manager
                        .available_cameras()
                        .iter()
                        .map(|c| c.name.clone())
                        .collect();

                    // プログラムカメラの選択
                    ui.menu_button("Program Camera", |ui| {
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
                        for (i, name) in camera_names.iter().enumerate() {
                            ui.radio_value(&mut new_selected, i, name);
                        }

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
                    });

                    // プレビューカメラの選択
                    ui.menu_button("Preview Camera", |ui| {
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
                        for (i, name) in camera_names.iter().enumerate() {
                            ui.radio_value(&mut new_preview, i, name);
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
                });

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(format!(
                        "📹 Cameras: {}",
                        self.camera_manager.available_cameras().len()
                    ));
                });
            });
        });

        // カメラエラーの表示（別ウィンドウで表示）
        if !self.camera_errors.is_empty() {
            egui::Window::new("Camera Errors")
                .anchor(egui::Align2::RIGHT_BOTTOM, egui::Vec2::new(-10.0, -10.0))
                .collapsible(true)
                .show(ctx, |ui| {
                    for camera in self.camera_manager.available_cameras() {
                        if let Some(err) = self.camera_errors.get(&camera.id) {
                            ui.colored_label(
                                egui::Color32::RED,
                                format!("{}: {}", camera.name, err),
                            );
                        }
                    }
                });
        }

        // ===== 入力を管理 ウィンドウ =====
        let mut show_settings = self.show_input_settings;
        egui::Window::new("Manage Inputs")
            .open(&mut show_settings)
            .resizable(false)
            .collapsible(false)
            .show(ctx, |ui| {
                let available_cameras = self.camera_manager.available_cameras().to_vec();

                for idx in 0..self.layout_config.view_count() {
                    ui.horizontal(|ui| {
                        ui.label(format!("Input {}:", idx + 1));

                        let current_camera_id =
                            self.layout_config.view(idx).and_then(|v| v.camera_id);

                        let selected_text = if let Some(id) = current_camera_id {
                            available_cameras
                                .iter()
                                .find(|c| c.id == id)
                                .map(|c| c.name.clone())
                                .unwrap_or_else(|| "Unknown".to_string())
                        } else {
                            "None".to_string()
                        };

                        egui::ComboBox::from_id_source(format!("input_select_{}", idx))
                            .selected_text(selected_text)
                            .show_ui(ui, |ui| {
                                // "None" の選択肢
                                let mut is_none = current_camera_id.is_none();
                                if ui.selectable_value(&mut is_none, true, "None").clicked() {
                                    self.layout_config.assign_camera(idx, None);
                                }

                                // 利用可能なカメラの選択肢
                                for camera in &available_cameras {
                                    let mut is_selected = current_camera_id == Some(camera.id);
                                    if ui
                                        .selectable_value(&mut is_selected, true, &camera.name)
                                        .clicked()
                                    {
                                        // 選択されたカメラを開く
                                        if let Err(e) = self.camera_manager.open_camera(camera.id) {
                                            self.camera_errors
                                                .insert(camera.id, format!("open failed: {}", e));
                                        } else {
                                            self.camera_errors.remove(&camera.id);
                                        }
                                        self.layout_config.assign_camera(idx, Some(camera.id));
                                    }
                                }
                            });
                    });
                }
            });
        self.show_input_settings = show_settings;

        // ===== トップパネル（プレビュー＆プログラム）=====
        // CentralPanelの内部余白(margin)を0に設定する
        let frame = egui::Frame::central_panel(&ctx.style()).inner_margin(0.0);
        egui::CentralPanel::default().frame(frame).show(ctx, |ui| {
            let available = ui.available_size();
            let (canvas_width, canvas_height) = Self::fit_canvas_size(available);
            let top_height = canvas_height / 2;
            let bottom_height = canvas_height - top_height;
            let top_view_width = canvas_width / 2;

            // キャンバスを画面中央に配置するための開始座標（オフセット）を計算
            let x_offset = ((available.x - canvas_width as f32) / 2.0).max(0.0);
            let y_offset = ((available.y - canvas_height as f32) / 2.0).max(0.0);

            // eguiの自動レイアウトを無視して、画面全体を自由に描画できる領域(Painter)として確保
            let (response, painter) = ui.allocate_painter(available, egui::Sense::hover());

            // 背景を黒に塗りつぶす（空の入力枠などのため）
            let bg_rect = egui::Rect::from_min_size(
                response.rect.min + egui::vec2(x_offset, y_offset),
                egui::vec2(canvas_width as f32, canvas_height as f32),
            );
            painter.rect_filled(bg_rect, 0.0, egui::Color32::BLACK);

            // 画像のUVと描画をヘルパー関数で処理
            let draw_cam = |camera_id: Option<CameraId>, rect: egui::Rect, label_text: &str| {
                // 枠のボーダーを描画
                painter.rect(
                    rect,
                    0.0,
                    egui::Color32::TRANSPARENT,
                    egui::Stroke::new(1.0, egui::Color32::DARK_GRAY),
                    egui::StrokeKind::Inside,
                );

                if let Some(id) = camera_id {
                    // テクスチャがあれば取得
                    if let Some(tex) = self.camera_textures.get(&id) {
                        let img_aspect = tex.width as f32 / tex.height as f32;
                        let target_aspect = 16.0 / 9.0;

                        let mut uv =
                            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));

                        // 画像が縦長の場合（上下をカット）
                        if img_aspect < target_aspect {
                            let crop_ratio = img_aspect / target_aspect;
                            let offset = (1.0 - crop_ratio) / 2.0;
                            uv = egui::Rect::from_min_max(
                                egui::pos2(0.0, offset),
                                egui::pos2(1.0, 1.0 - offset),
                            );
                        }
                        // 画像が横長の場合（左右をカット）
                        else if img_aspect > target_aspect {
                            let crop_ratio = target_aspect / img_aspect;
                            let offset = (1.0 - crop_ratio) / 2.0;
                            uv = egui::Rect::from_min_max(
                                egui::pos2(offset, 0.0),
                                egui::pos2(1.0 - offset, 1.0),
                            );
                        }

                        painter.image(tex.handle.id(), rect, uv, egui::Color32::WHITE);
                    }
                }

                // 文字ラベルの描画
                if self.show_labels && !label_text.is_empty() {
                    let text_color = egui::Color32::WHITE;
                    let bg_color = egui::Color32::from_black_alpha(160); // 半透明の黒
                    let font_id = egui::FontId::proportional(16.0);

                    // フォントのレイアウトを計算するため、一時的に galley を作成
                    let galley =
                        painter.layout_no_wrap(label_text.to_string(), font_id, text_color);

                    let text_size = galley.size();
                    // 中央下部に配置。下からすこし(4px)だけ浮かせる
                    let text_pos = egui::pos2(
                        rect.center().x - text_size.x / 2.0,
                        rect.max.y - text_size.y - 4.0,
                    );

                    // 背景の矩形をテキストより少し大きめに描画（パディング2px）
                    let bg_rect = egui::Rect::from_min_size(
                        text_pos - egui::vec2(6.0, 2.0),
                        text_size + egui::vec2(12.0, 4.0),
                    );

                    painter.rect_filled(bg_rect, 4.0, bg_color); // 角丸4px
                    painter.galley(text_pos, galley, egui::Color32::WHITE);
                }
            };

            let base_pos = response.rect.min + egui::vec2(x_offset, y_offset);

            // ① プレビュー（左上）
            let preview_rect = egui::Rect::from_min_size(
                base_pos,
                egui::vec2(top_view_width as f32, top_height as f32),
            );
            draw_cam(self.preview_camera_id, preview_rect, "Preview");

            // ② プログラム（右上）
            let program_rect = egui::Rect::from_min_size(
                base_pos + egui::vec2(top_view_width as f32, 0.0),
                egui::vec2(top_view_width as f32, top_height as f32),
            );
            draw_cam(self.selected_camera_id, program_rect, "Program");

            // ③ マルチビュー（下段 4x2）
            let mut view_idx = 0;
            // self.layout_config.input_rows: usize, columns: usize があれば良いが
            // layout::LayoutConfig の仕様が不明な場合決め打ちでもOK
            // LayoutConfigに依存せず、常に 4x2 として描画
            let cols = 4;
            let rows = 2;
            let cell_width = canvas_width as f32 / cols as f32;
            let cell_height = bottom_height as f32 / rows as f32;

            for r in 0..rows {
                for c in 0..cols {
                    let rect = egui::Rect::from_min_size(
                        base_pos
                            + egui::vec2(
                                c as f32 * cell_width,
                                top_height as f32 + r as f32 * cell_height,
                            ),
                        egui::vec2(cell_width, cell_height),
                    );

                    // レイアウト上のカメラIDを取得
                    let (cam_id, cam_label) = if view_idx < self.layout_config.view_count() {
                        (
                            self.layout_config.view(view_idx).and_then(|v| v.camera_id),
                            format!("Cam {}", view_idx + 1),
                        )
                    } else {
                        (None, String::new())
                    };

                    draw_cam(cam_id, rect, &cam_label);
                    view_idx += 1;
                }
            }
        });

        // アニメーションを維持するために再描画リクエスト
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
