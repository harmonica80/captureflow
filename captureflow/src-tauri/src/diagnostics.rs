use serde::Serialize;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OcrLanguage {
    language_tag: String,
    display_name: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CapabilityReport {
    windows_graphics_capture: bool,
    windows_ocr: bool,
    traditional_chinese_ocr: bool,
    english_ocr: bool,
    ocr_max_image_dimension: u32,
    ocr_languages: Vec<OcrLanguage>,
    recording_path: &'static str,
    mp4_encoder: &'static str,
    gif_encoder: &'static str,
}

#[cfg(windows)]
pub fn run() -> Result<CapabilityReport, String> {
    use windows::{
        Graphics::Capture::GraphicsCaptureSession,
        Media::Ocr::OcrEngine,
        Win32::System::WinRT::{RoInitialize, RoUninitialize, RO_INIT_MULTITHREADED},
    };

    struct WinRtApartment;
    impl Drop for WinRtApartment {
        fn drop(&mut self) {
            unsafe { RoUninitialize() };
        }
    }
    unsafe { RoInitialize(RO_INIT_MULTITHREADED) }.map_err(|error| error.to_string())?;
    let _apartment = WinRtApartment;
    let windows_graphics_capture = GraphicsCaptureSession::IsSupported().unwrap_or(false);
    let languages = OcrEngine::AvailableRecognizerLanguages().map_err(|error| error.to_string())?;
    let mut ocr_languages = Vec::new();
    for index in 0..languages.Size().map_err(|error| error.to_string())? {
        let language = languages.GetAt(index).map_err(|error| error.to_string())?;
        ocr_languages.push(OcrLanguage {
            language_tag: language
                .LanguageTag()
                .map_err(|error| error.to_string())?
                .to_string(),
            display_name: language
                .DisplayName()
                .map_err(|error| error.to_string())?
                .to_string(),
        });
    }
    let traditional_chinese_ocr = ocr_languages.iter().any(|language| {
        let tag = language.language_tag.to_ascii_lowercase();
        tag == "zh-hant" || tag.starts_with("zh-tw") || tag.starts_with("zh-hk")
    });
    let english_ocr = ocr_languages
        .iter()
        .any(|language| language.language_tag.to_ascii_lowercase().starts_with("en"));

    Ok(CapabilityReport {
        windows_graphics_capture,
        windows_ocr: !ocr_languages.is_empty(),
        traditional_chinese_ocr,
        english_ocr,
        ocr_max_image_dimension: OcrEngine::MaxImageDimension()
            .map_err(|error| error.to_string())?,
        ocr_languages,
        recording_path: "Windows.Graphics.Capture + Direct3D 11",
        mp4_encoder: "Windows Media Foundation H.264",
        gif_encoder: "Optional external FFmpeg (LGPL build)",
    })
}

#[cfg(not(windows))]
pub fn run() -> Result<CapabilityReport, String> {
    Err("Capability diagnostics are available on Windows only.".into())
}
