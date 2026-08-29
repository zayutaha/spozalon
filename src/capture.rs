use std::io::Read;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use anyhow::{Context, Result};
use tokio::sync::mpsc;

use crate::config::*;

/// Capture system audio via pw-record (PipeWire) and send PCM chunks to a channel.
/// Returns the child process handle so the caller can kill it on shutdown.
pub fn start_capture(
    audio_tx: mpsc::Sender<Vec<f32>>,
    running: Arc<AtomicBool>,
    verbose: bool,
) -> Result<Child> {
    let monitor_source = find_monitor_source()?;

    if verbose {
        eprintln!("[capture] Using PipeWire source: {}", monitor_source);
    }

    let mut child = Command::new("pw-record")
        .args([
            "--raw",
            "--format", "f32",
            "--rate", &SAMPLE_RATE.to_string(),
            "--channels", &CHANNELS.to_string(),
            "--latency", "2ms",
            "--target", &monitor_source,
            "-", // output to stdout
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("Failed to start pw-record. Is PipeWire installed?")?;

    let stdout = child.stdout.take().context("Failed to capture pw-record stdout")?;

    // Read raw PCM from pw-record stdout in a blocking thread
    std::thread::Builder::new()
        .name("pw-record-reader".into())
        .spawn(move || {
            // Read fixed-size chunks: 2.5ms at 48kHz stereo = 120 samples * 2ch * 4 bytes
            let chunk_bytes = 120 * CHANNELS as usize * std::mem::size_of::<f32>();
            let mut reader = stdout;
            let mut buf = vec![0u8; chunk_bytes];

            while running.load(Ordering::Relaxed) {
                match reader.read_exact(&mut buf) {
                    Ok(()) => {
                        let samples: Vec<f32> = buf
                            .chunks_exact(4)
                            .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
                            .collect();
                        let _ = audio_tx.blocking_send(samples);
                    }
                    Err(_) => break,
                }
            }
        })?;

    Ok(child)
}

/// Find the default PipeWire monitor source via pactl.
fn find_monitor_source() -> Result<String> {
    let output = Command::new("pactl")
        .args(["get-default-source"])
        .output()
        .context("Failed to run pactl")?;

    let source = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if source.is_empty() {
        anyhow::bail!("No default PipeWire source found. Set one with: pactl set-default-source <source>");
    }
    Ok(source)
}
