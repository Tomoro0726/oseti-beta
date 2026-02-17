use eframe::egui;
use nokhwa::pixel_format::RgbFormat;
use nokhwa::utils::{ApiBackend, CameraIndex, RequestedFormat, RequestedFormatType};
use nokhwa::{Camera, native_api_backend, query};
use std::time::Instant;

fn main() -> eframe::Result {
    nokhwa::nokhwa_initialize(|_| {});

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1280.0, 800.0]),
        ..Default::default()
    };

    eframe::run_native(
        "Oseti Beta - Camera Switcher & Fader",
        options,
        Box::new(|cc| Ok(Box::new(CameraApp::new(&cc.egui_ctx)))),
    )
}

struct CameraApp {
    camera: Option<Camera>,
    texture: Option<egui::TextureHandle>,
    // カメラリスト管理
    available_cameras: Vec<nokhwa::utils::CameraInfo>,
    selected_index: usize,
    // フェード管理
    alpha: f32, // 0.0 = Camera, 1.0 = Pattern
    target_alpha: f32,
    start_time: Instant,
}

impl CameraApp {
    fn new(_ctx: &egui::Context) -> Self {
        let backend = native_api_backend().unwrap_or(ApiBackend::Auto);
        let cameras = query(backend).unwrap_or_default();

        let mut app = Self {
            camera: None,
            texture: None,
            available_cameras: cameras,
            selected_index: 0,
            alpha: 0.0,
            target_alpha: 0.0,
            start_time: Instant::now(),
        };

        // 最初のカメラを初期化
        if !app.available_cameras.is_empty() {
            app.init_camera(0);
        }

        app
    }

    fn init_camera(&mut self, index: usize) {
        // 現在のストリームを停止してリセット
        self.camera = None;

        if let Some(info) = self.available_cameras.get(index) {
            let requested =
                RequestedFormat::new::<RgbFormat>(RequestedFormatType::AbsoluteHighestFrameRate);

            if let Ok(mut cam) = Camera::new(info.index().clone(), requested) {
                if cam.open_stream().is_ok() {
                    self.camera = Some(cam);
                    self.selected_index = index;
                    println!("Started camera: {}", info.human_name());
                }
            }
        }
    }
}

impl eframe::App for CameraApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let dt = ctx.input(|i| i.stable_dt);
        let fade_speed = 2.0;
        if (self.alpha - self.target_alpha).abs() > 0.001 {
            if self.alpha < self.target_alpha {
                self.alpha = (self.alpha + dt * fade_speed).min(1.0);
            } else {
                self.alpha = (self.alpha - dt * fade_speed).max(0.0);
            }
            ctx.request_repaint();
        }

        if let Some(ref mut cam) = self.camera {
            if let Ok(frame) = cam.frame() {
                if let Ok(rgb_image) = frame.decode_image::<RgbFormat>() {
                    let w = rgb_image.width() as usize;
                    let h = rgb_image.height() as usize;
                    let cam_pixels = rgb_image.as_raw();
                    let time = self.start_time.elapsed().as_secs_f32();

                    let mut blended_pixels = vec![0u8; w * h * 4];

                    for i in 0..(w * h) {
                        let idx_rgb = i * 3;
                        let idx_rgba = i * 4;

                        let x = (i % w) as f32;
                        let y = (i / w) as f32;

                        let p_val = (((x * 0.05 + time * 2.0).sin()
                            + (y * 0.05 + time).cos()
                            + 2.0)
                            * 60.0) as u8;

                        let r_c = cam_pixels[idx_rgb] as f32;
                        let g_c = cam_pixels[idx_rgb + 1] as f32;
                        let b_c = cam_pixels[idx_rgb + 2] as f32;

                        let r_p = p_val as f32;
                        let g_p = (p_val / 2) as f32;
                        let b_p = 255.0 * self.alpha; // 青みを強く

                        blended_pixels[idx_rgba] =
                            (r_c * (1.0 - self.alpha) + r_p * self.alpha) as u8;
                        blended_pixels[idx_rgba + 1] =
                            (g_c * (1.0 - self.alpha) + g_p * self.alpha) as u8;
                        blended_pixels[idx_rgba + 2] =
                            (b_c * (1.0 - self.alpha) + b_p * self.alpha) as u8;
                        blended_pixels[idx_rgba + 3] = 255;
                    }

                    let color_image =
                        egui::ColorImage::from_rgba_unmultiplied([w, h], &blended_pixels);
                    if let Some(texture) = &mut self.texture {
                        texture.set(color_image, egui::TextureOptions::LINEAR);
                    } else {
                        self.texture = Some(ctx.load_texture(
                            "main_stream",
                            color_image,
                            egui::TextureOptions::LINEAR,
                        ));
                    }
                }
            }
        }

        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading("Oseti Beta - Switcher");
                ui.separator();

                let prev_index = self.selected_index;
                egui::ComboBox::from_id_source("camera_list")
                    .width(250.0)
                    .selected_text(
                        self.available_cameras
                            .get(self.selected_index)
                            .map(|c| c.human_name())
                            .unwrap_or_else(|| "No Camera".to_string()),
                    )
                    .show_ui(ui, |ui| {
                        for (i, info) in self.available_cameras.iter().enumerate() {
                            ui.selectable_value(&mut self.selected_index, i, info.human_name());
                        }
                    });

                if prev_index != self.selected_index {
                    self.init_camera(self.selected_index);
                }

                ui.separator();

                if ui
                    .selectable_label(self.target_alpha == 0.0, "CAM")
                    .clicked()
                {
                    self.target_alpha = 0.0;
                }
                if ui
                    .selectable_label(self.target_alpha == 1.0, "PAT")
                    .clicked()
                {
                    self.target_alpha = 1.0;
                }

                ui.add(egui::Slider::new(&mut self.alpha, 0.0..=1.0).text("Mix Rate"));
            });
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            if let Some(texture) = &self.texture {
                ui.image((texture.id(), ui.available_size()));
            } else {
                ui.centered_and_justified(|ui| {
                    ui.label("Waiting for Camera Stream...");
                });
            }
        });

        ctx.request_repaint();
    }
}
