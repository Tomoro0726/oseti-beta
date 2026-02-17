use eframe::egui;
use nokhwa::pixel_format::RgbFormat;
use nokhwa::utils::{ApiBackend, CameraIndex, RequestedFormat, RequestedFormatType};
use nokhwa::{Camera, native_api_backend, query};
use std::time::Instant;

fn main() -> eframe::Result {
    nokhwa::nokhwa_initialize(|_| {});
    let options = eframe::NativeOptions::default();
    eframe::run_native(
        "Oseti beta",
        options,
        Box::new(|cc| Ok(Box::new(CameraApp::new(&cc.egui_ctx)))),
    )
}

struct CameraApp {
    camera: Option<Camera>,
    texture: Option<egui::TextureHandle>,
    // フェードの管理
    alpha: f32, // 0.0 = Camera, 1.0 = Pattern
    target_alpha: f32,
    start_time: Instant,
}

impl CameraApp {
    fn new(_ctx: &egui::Context) -> Self {
        let backend = native_api_backend().unwrap_or(ApiBackend::Auto);
        let cameras = query(backend).unwrap_or_default();

        let mut camera = None;
        if let Some(info) = cameras.first() {
            let requested =
                RequestedFormat::new::<RgbFormat>(RequestedFormatType::AbsoluteHighestFrameRate);
            if let Ok(mut cam) = Camera::new(info.index().clone(), requested) {
                let _ = cam.open_stream();
                camera = Some(cam);
            }
        }

        Self {
            camera,
            texture: None,
            alpha: 0.0,
            target_alpha: 0.0,
            start_time: Instant::now(),
        }
    }
}

impl eframe::App for CameraApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let dt = ctx.input(|i| i.stable_dt);
        let fade_speed = 2.0; // 0.5秒で完了
        if (self.alpha - self.target_alpha).abs() > 0.01 {
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
                        let p_val = (((x * 0.1 + time * 5.0).sin() + 1.0) * 127.0) as u8;

                        let r_c = cam_pixels[idx_rgb] as f32;
                        let g_c = cam_pixels[idx_rgb + 1] as f32;
                        let b_c = cam_pixels[idx_rgb + 2] as f32;

                        let r_p = p_val as f32;
                        let g_p = p_val as f32;
                        let b_p = 200.0;

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
                            "main",
                            color_image,
                            egui::TextureOptions::LINEAR,
                        ));
                    }
                }
            }
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.horizontal(|ui| {
                if ui
                    .selectable_label(self.target_alpha == 0.0, "Camera")
                    .clicked()
                {
                    self.target_alpha = 0.0;
                }
                if ui
                    .selectable_label(self.target_alpha == 1.0, "Test Pattern")
                    .clicked()
                {
                    self.target_alpha = 1.0;
                }
                ui.add(egui::Slider::new(&mut self.alpha, 0.0..=1.0).text("Mix"));
            });

            if let Some(texture) = &self.texture {
                ui.image((texture.id(), ui.available_size()));
            }
        });

        ctx.request_repaint();
    }
}
