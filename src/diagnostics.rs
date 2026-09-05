use std::sync::Mutex;

static LAST: Mutex<Option<String>> = Mutex::new(None);
struct Diagnostics;
impl log::Log for Diagnostics {
    fn enabled(&self, m: &log::Metadata<'_>) -> bool {
        m.level() <= log::Level::Error && m.target().starts_with("librespot")
    }
    fn log(&self, r: &log::Record<'_>) {
        if self.enabled(r.metadata()) {
            let message = r.args().to_string();
            // Keep only a classified cause in RAM; never retain upstream text, URLs or tokens.
            let cause = if message.contains("Unavailable") || message.contains("unavailable") {
                "Track unavailable for this account or region"
            } else if message.contains("PermissionDenied") || message.contains("403") {
                "Spotify streaming authorization denied (HTTP 403)"
            } else if message.contains("Unauthenticated") || message.contains("401") {
                "Spotify streaming authentication rejected (HTTP 401)"
            } else if message.contains("404") {
                "Spotify streaming metadata endpoint returned HTTP 404"
            } else if message.contains("429") {
                "Spotify streaming endpoint is rate limited (HTTP 429)"
            } else if message.contains("deadline") || message.contains("timed out") {
                "Spotify streaming request timed out"
            } else if message.contains("audio item") {
                "Librespot could not load Spotify audio metadata"
            } else if message.contains("encrypted file") {
                "Librespot could not load the encrypted audio stream"
            } else {
                return;
            };
            if let Ok(mut last) = LAST.lock() {
                if last.is_none() {
                    *last = Some(cause.into());
                }
            }
        }
    }
    fn flush(&self) {}
}
pub fn init() {
    static LOGGER: Diagnostics = Diagnostics;
    let _ = log::set_logger(&LOGGER);
    log::set_max_level(log::LevelFilter::Error);
}
pub fn take() -> Option<String> {
    LAST.lock().ok().and_then(|mut last| last.take())
}
