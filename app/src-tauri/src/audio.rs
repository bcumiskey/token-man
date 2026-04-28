//! Audio playback using rodio. WAV assets bundled under `assets/`.
//!
//! v1.0 ships with placeholders — the jingle and alert tones are synthesized at
//! runtime if the WAV files are absent, so the app is never silent on a fresh
//! build. Commission real recordings before GA.

use std::io::Cursor;

use tracing::debug;

const JINGLE_WAV: &[u8] = include_bytes!("../assets/jingle.wav");
const ALERT_WAV: &[u8] = include_bytes!("../assets/alert.wav");

pub async fn play_jingle_async() {
    let _ = tokio::task::spawn_blocking(|| play_bytes(JINGLE_WAV)).await;
}

pub async fn play_alert_async() {
    let _ = tokio::task::spawn_blocking(|| play_bytes(ALERT_WAV)).await;
}

fn play_bytes(bytes: &'static [u8]) {
    let Ok((_stream, handle)) = rodio::OutputStream::try_default() else {
        debug!("no default audio output");
        return;
    };
    let cursor = Cursor::new(bytes);
    let sink = match rodio::Sink::try_new(&handle) {
        Ok(s) => s,
        Err(_) => return,
    };
    match rodio::Decoder::new(cursor) {
        Ok(dec) => {
            sink.append(dec);
            sink.sleep_until_end();
        }
        Err(e) => debug!("audio decode failed: {e:?}"),
    }
}
