use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use tokio::net::UdpSocket;
use tokio::time::sleep;

use crate::config::*;
use crate::protocol::Packet;

/// Flat sample buffer — a continuous ring of f32 stereo samples.
/// Network thread pushes samples, audio callback reads sequentially.
/// No gaps between chunks — eliminates bursty silence.
struct SampleBuffer {
    samples: Vec<f32>,
    write_pos: usize,
    read_pos: usize,
    filled: usize, // how many valid samples between read_pos and write_pos
    target_fill: usize, // adaptive target: how many samples to keep buffered
}

impl SampleBuffer {
    /// capacity in stereo samples (each frame = left + right = 2 f32s)
    fn new(capacity_frames: usize) -> Self {
        let capacity = capacity_frames * 2;
        Self {
            samples: vec![0.0; capacity],
            write_pos: 0,
            read_pos: 0,
            filled: 0,
            target_fill: capacity / 3, // start at 1/3 capacity
        }
    }

    fn capacity(&self) -> usize {
        self.samples.len()
    }

    /// Report fill level to adaptive controller — call after push or read
    fn update_target(&mut self) {
        let cap = self.capacity();
        let fill_ratio = self.filled as f64 / cap as f64;
        // If buffer is >70% full, we're lagging behind — shrink target
        // If buffer is <30% full, we're starving — grow target
        // Hysteresis prevents oscillation
        if fill_ratio > 0.7 {
            self.target_fill = (self.target_fill * 3 / 4).max(cap / 8);
        } else if fill_ratio < 0.2 && self.target_fill < cap * 2 / 3 {
            self.target_fill = (self.target_fill * 5 / 4).min(cap * 2 / 3);
        }
    }

    /// Push stereo interleaved samples [L0, R0, L1, R1, ...]
    fn push(&mut self, data: &[f32]) {
        for &s in data {
            self.samples[self.write_pos] = s;
            self.write_pos = (self.write_pos + 1) % self.capacity();
            if self.filled < self.capacity() {
                self.filled += 1;
            } else {
                // Overwrite oldest — advance read_pos
                self.read_pos = (self.read_pos + 1) % self.capacity();
            }
        }
        self.update_target();
    }

    /// Read up to `max_frames` stereo frames into `out` (interleaved).
    /// Returns number of frames actually written.
    fn read(&mut self, out: &mut [f32], channels: usize) -> usize {
        let frames_needed = out.len() / channels;
        let frames_available = self.filled / 2; // stereo frames
        let frames_to_read = frames_needed.min(frames_available);

        for i in 0..frames_to_read {
            let left = self.samples[self.read_pos];
            let right = self.samples[(self.read_pos + 1) % self.capacity()];
            self.read_pos = (self.read_pos + 2) % self.capacity();

            let frame_idx = i * channels;
            if frame_idx < out.len() {
                out[frame_idx] = left;
            }
            if frame_idx + 1 < out.len() {
                out[frame_idx + 1] = right;
            }
        }
        self.filled -= frames_to_read * 2;

        frames_to_read
    }

    fn clear(&mut self) {
        self.write_pos = 0;
        self.read_pos = 0;
        self.filled = 0;
        self.target_fill = self.capacity() / 6; // restart conservative
    }
}

/// Run the receiver: get audio stream and play through speakers.
pub async fn run(
    sender_ip: &str,
    port: u16,
    device_name: Option<&str>,
    verbose: bool,
) -> Result<()> {
    let target_addr = format!("{}:{}", sender_ip, port);

    // --- Find audio playback device ---
    let host = cpal::default_host();
    let device = find_playback_device(&host, device_name)?;

    let dev_name = device
        .description()
        .map(|d| d.name().to_string())
        .unwrap_or_else(|_| "unknown".into());
    if verbose {
        eprintln!("[receiver] Using device: {}", dev_name);
    }

    let supported_config = device
        .default_output_config()
        .context("Failed to get default output config")?;

    let _sample_rate = supported_config.sample_rate();
    let channels = supported_config.channels() as usize;

    // Use lowest latency config available
    let mut stream_config = supported_config.config();
    stream_config.buffer_size = cpal::BufferSize::Default;

    // --- Shared state ---
    // ~40ms buffer — adaptive, grows on WiFi jitter, shrinks when stable
    let buf_frames = _sample_rate as usize * 40 / 1000;
    let buffer = Arc::new(Mutex::new(SampleBuffer::new(buf_frames)));
    let current_volume = Arc::new(AtomicU32::new(100));
    let initialized = Arc::new(AtomicBool::new(false));

    // --- Audio playback stream ---
    let buffer_clone = buffer.clone();
    let vol_clone = current_volume.clone();
    let init_clone = initialized.clone();

    let stream = device.build_output_stream(
        stream_config,
        move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
            let vol = vol_clone.load(Ordering::Relaxed) as f32 / 100.0;
            let _ = init_clone.load(Ordering::Relaxed);

            let mut buf = buffer_clone.lock().unwrap();
            let frames_read = buf.read(data, channels);

            // Apply volume
            for i in 0..frames_read * channels {
                if i < data.len() {
                    data[i] *= vol;
                }
            }

            // Fill any remaining frames with silence
            for s in data.iter_mut().skip(frames_read * channels) {
                *s = 0.0;
            }
        },
        move |err| {
            eprintln!("[receiver] Audio playback error: {}", err);
        },
        None,
    )?;

    stream.play().context("Failed to start audio playback")?;

    if verbose {
        eprintln!("[receiver] Audio playback started");
    }

    // --- Network ---
    let socket = Arc::new(
        UdpSocket::bind("0.0.0.0:0")
            .await
            .context("Failed to bind UDP socket")?,
    );

    let running = Arc::new(AtomicBool::new(true));
    let running_clone = running.clone();
    let verbose_flag = verbose;

    tokio::spawn(async move {
        let _ = tokio::signal::ctrl_c().await;
        if verbose_flag {
            eprintln!("\n[receiver] Shutting down...");
        }
        running_clone.store(false, Ordering::SeqCst);
    });

    let mut connected = false;
    let handshake_interval = sleep(Duration::from_millis(HANDSHAKE_RETRY_MS));
    tokio::pin!(handshake_interval);

    let mut net_buf = [0u8; 65536];

    loop {
        if !running.load(Ordering::SeqCst) {
            break;
        }

        if !connected {
            tokio::select! {
                _ = &mut handshake_interval => {
                    let pkt = Packet::handshake(0);
                    let bytes = pkt.serialize();
                    if let Err(e) = socket.send_to(&bytes, &target_addr).await {
                        if verbose {
                            eprintln!("[receiver] Handshake error: {}", e);
                        }
                    } else if verbose {
                        eprintln!("[receiver] Sent handshake to {}", target_addr);
                    }
                    handshake_interval.as_mut().reset(
                        tokio::time::Instant::now() + Duration::from_millis(HANDSHAKE_RETRY_MS)
                    );
                }
                result = socket.recv_from(&mut net_buf) => {
                    match result {
                        Ok((len, _)) => {
                            if let Some(pkt) = Packet::deserialize(&net_buf[..len]) {
                                connected = true;
                                current_volume.store(pkt.volume_percent as u32, Ordering::Relaxed);
                                initialized.store(true, Ordering::Relaxed);
                                if verbose {
                                    eprintln!("[receiver] Connected! Volume: {}%", pkt.volume_percent);
                                }
                            }
                        }
                        Err(e) => {
                            if verbose {
                                eprintln!("[receiver] Receive error: {}", e);
                            }
                        }
                    }
                }
            }
            continue;
        }

        // --- Receive packets and push directly into flat buffer ---
        match tokio::time::timeout(
            Duration::from_millis(DISCONNECT_TIMEOUT_MS),
            socket.recv_from(&mut net_buf),
        )
        .await
        {
            Ok(Ok((len, _))) => {
                if let Some(pkt) = Packet::deserialize(&net_buf[..len]) {
                    if !pkt.is_handshake() {
                        current_volume.store(pkt.volume_percent as u32, Ordering::Relaxed);

                        // Push directly into flat sample buffer — no chunk boundaries
                        let mut buf = buffer.lock().unwrap();
                        buf.push(&pkt.pcm_data);
                    }
                }
            }
            Ok(Err(e)) => {
                if verbose {
                    eprintln!("[receiver] Receive error: {}", e);
                }
            }
            Err(_) => {
                if verbose {
                    eprintln!("[receiver] Connection lost, reconnecting...");
                }
                connected = false;
                initialized.store(false, Ordering::Relaxed);
                let mut buf = buffer.lock().unwrap();
                buf.clear();
            }
        }
    }

    if verbose {
        eprintln!("[receiver] Stopped.");
    }

    Ok(())
}

/// Find a suitable audio playback device.
fn find_playback_device(host: &cpal::Host, name: Option<&str>) -> Result<cpal::Device> {
    if let Some(name) = name {
        let devices = host
            .output_devices()
            .context("Failed to enumerate output devices")?;

        for device in devices {
            let matches = device
                .description()
                .map(|d| d.name() == name)
                .unwrap_or(false);
            if matches {
                return Ok(device);
            }
        }

        anyhow::bail!("Audio device '{}' not found", name);
    }

    host.default_output_device()
        .context("No audio output devices found")
}
