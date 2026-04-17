//! カメラ管理モジュール
//!
//! 複数のカメラインスタンスのライフサイクル管理とアクセスを提供します。

use nokhwa::pixel_format::RgbFormat;
use nokhwa::utils::{ApiBackend, CameraIndex, RequestedFormat, RequestedFormatType};
use nokhwa::{Camera, native_api_backend, query};

/// カメラIDの型安全なラッパー
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CameraId(pub usize);

/// カメラ情報（メタデータ）
#[derive(Debug, Clone)]
pub struct CameraInfo {
    pub id: CameraId,
    pub name: String,
    pub index: nokhwa::utils::CameraIndex,
}

/// カメラマネージャー
///
/// 複数のカメラをアクティブに管理し、フレーム取得とカメラの切り替えを処理します。
pub struct CameraManager {
    /// 利用可能なカメラの情報リスト
    available_cameras: Vec<CameraInfo>,
    /// 現在アクティブなカメラマップ（ID -> Camera）
    active_cameras: std::collections::HashMap<CameraId, Camera>,
}

impl CameraManager {
    /// 新しいカメラマネージャーを初期化します
    ///
    /// システムに接続されているすべてのカメラを列挙します。
    pub fn new() -> Self {
        let backend = native_api_backend().unwrap_or(nokhwa::utils::ApiBackend::Auto);
        let cameras = query(backend).unwrap_or_default();

        let available_cameras = cameras
            .into_iter()
            .enumerate()
            .map(|(i, info)| CameraInfo {
                id: CameraId(i),
                name: info.human_name(),
                index: info.index().clone(),
            })
            .collect();

        Self {
            available_cameras,
            active_cameras: std::collections::HashMap::new(),
        }
    }

    /// 利用可能なカメラ情報を取得
    pub fn available_cameras(&self) -> &[CameraInfo] {
        &self.available_cameras
    }

    /// カメラを開く（ストリーミング開始）
    ///
    /// 同じIDのカメラが既に開かれている場合は何もしません。
    pub fn open_camera(&mut self, camera_id: CameraId) -> Result<(), String> {
        if self.active_cameras.contains_key(&camera_id) {
            return Ok(());
        }

        let info = self
            .available_cameras
            .iter()
            .find(|c| c.id == camera_id)
            .ok_or_else(|| format!("Camera {:?} not found", camera_id))?;

        let requested =
            RequestedFormat::new::<RgbFormat>(RequestedFormatType::AbsoluteHighestFrameRate);

        let mut cam = Camera::new(info.index.clone(), requested)
            .map_err(|e| format!("Failed to create camera: {}", e))?;

        cam.open_stream()
            .map_err(|e| format!("Failed to open stream: {}", e))?;

        self.active_cameras.insert(camera_id, cam);
        Ok(())
    }

    /// カメラを閉じる
    pub fn close_camera(&mut self, camera_id: CameraId) {
        self.active_cameras.remove(&camera_id);
    }

    /// カメラからフレームを取得
    ///
    /// 成功時はRGB画像データとサイズ(width, height)を返します。
    pub fn get_frame(&mut self, camera_id: CameraId) -> Result<(Vec<u8>, u32, u32), String> {
        if !self.active_cameras.contains_key(&camera_id) {
            self.open_camera(camera_id)?;
        }

        let cam = self
            .active_cameras
            .get_mut(&camera_id)
            .ok_or_else(|| format!("Camera {:?} not open", camera_id))?;

        let frame = cam
            .frame()
            .map_err(|e| format!("Frame capture failed: {}", e))?;

        let rgb_image = frame
            .decode_image::<RgbFormat>()
            .map_err(|e| format!("Frame decode failed: {}", e))?;

        let width = rgb_image.width();
        let height = rgb_image.height();
        let pixels = rgb_image.as_raw().to_vec();

        // 解像度チェック
        if width == 0 || height == 0 {
            return Err(format!(
                "Camera {:?} returned invalid resolution: {}x{}",
                camera_id, width, height
            ));
        }

        // フレームデータサイズチェック
        let expected_size = (width as usize) * (height as usize) * 3;
        if pixels.len() != expected_size {
            eprintln!(
                "WARNING: Camera {:?} frame size mismatch: expected {} bytes for {}x{}, got {}",
                camera_id,
                expected_size,
                width,
                height,
                pixels.len()
            );
        }

        Ok((pixels, width, height))
    }

    /// すべてのアクティブなカメラを取得
    pub fn active_camera_ids(&self) -> Vec<CameraId> {
        self.active_cameras.keys().copied().collect()
    }
}

impl Default for CameraManager {
    fn default() -> Self {
        Self::new()
    }
}
