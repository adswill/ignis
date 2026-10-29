// Smoke test for the audio system (legacy stats-driven fallback engine + live device).
// cargo run --release --example audio_demo   (AUDIO_WAV=out.wav to dump, AUDIO_NO_DEVICE=1 to skip live)
#![allow(dead_code, unused_imports, unused_variables, unused_mut, clippy::all)]
#[path = "../src/audio.rs"]
mod audio;

fn main() {
    audio::demo_cli();
}
