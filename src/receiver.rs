use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use tokio::net::UdpSocket;
use tokio::time::sleep;

use crate::config::*;
use crate::protocol::Packet;

/// Ring buffer for received audio chunks.
struct RingBuffer {
    chunks: VecDeque<Vec<f32>>,
    capacity: usize,
}

impl RingBuffer {
    fn new(capacity: usize) -> Self {
        Self {
            chunks: VecDeque::with_capacity(capacity),
            capacity,
        }
    }

    fn push(&mut self, chunk: Vec<f32>) {
        if self.chunks.len() >= self.capacity {
            self.chunks.pop_front();
        }
        self.chunks.push_back(chunk);
    }

    fn pop_or_silence(&mut self, frames_needed: usize) -> Vec<f32> {
        match self.chunks.pop_front() {
            Some(chunk) => chunk,
            // Silence: frames_needed stereo frames
            None => vec![0.0; frames_needed * 2],
        }
    }

    fn len(&self) -> usize {
        self.chunks.len()
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

    if verbose {
        eprintln!(
            "[receiver] Audio config: {} Hz, {:?}, {:?}",
            supported_config.sample_rate(),
            supported_config.channels(),
            supported_config.sample_format()
        );
    }

    let _sample_rate = supported_config.sample_rate();
    let channels = supported_config.channels() as usize;
    let stream_config = supported_config.config();

    // --- Shared state ---
    let ring = Arc::new(Mutex::new(RingBuffer::new(RING_BUFFER_CHUNKS)));
    let current_volume = Arc::new(AtomicU32::new(100));
    let initialized = Arc::new(AtomicBool::new(false));

    // --- Audio playback stream ---
    let ring_clone = ring.clone();
    let vol_clone = current_volume.clone();
    let init_clone = initialized.clone();

    let stream = device.build_output_stream(
        stream_config,
        move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
            let vol = vol_clone.load(Ordering::Relaxed) as f32 / 100.0;
            let _ = init_clone.load(Ordering::Relaxed); // reserved for future drift correction

            let frames_needed = data.len() / channels;
            let mut written = 0;

            {
                let mut buf = ring_clone.lock().unwrap();

                while written < frames_needed {
                    if buf.len() == 0 {
                        // Ring empty — fill rest with silence
                        for s in data.iter_mut().skip(written * channels) {
                            *s = 0.0;
                        }
                        break;
                    }

                    let chunk = buf.pop_or_silence(frames_needed - written);
                    let frames_in_chunk = chunk.len() / 2;

                    for i in 0..frames_in_chunk {
                        if written >= frames_needed {
                            break;
                        }
                        let frame_idx = written * channels;
                        if frame_idx < data.len() {
                            data[frame_idx] = chunk[i * 2] * vol;
                        }
                        if frame_idx + 1 < data.len() {
                            data[frame_idx + 1] = chunk[i * 2 + 1] * vol;
                        }
                        written += 1;
                    }
                }
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

    let mut buf = [0u8; 65536];

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
                result = socket.recv_from(&mut buf) => {
                    match result {
                        Ok((len, _)) => {
                            if let Some(pkt) = Packet::deserialize(&buf[..len]) {
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

        // --- Receive packets ---
        match tokio::time::timeout(
            Duration::from_millis(DISCONNECT_TIMEOUT_MS),
            socket.recv_from(&mut buf),
        )
        .await
        {
            Ok(Ok((len, _))) => {
                if let Some(pkt) = Packet::deserialize(&buf[..len]) {
                    if !pkt.is_handshake() {
                        current_volume.store(pkt.volume_percent as u32, Ordering::Relaxed);

                        // Push to ring buffer
                        let mut ring = ring.lock().unwrap();
                        ring.push(pkt.pcm_data);
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
                let mut ring = ring.lock().unwrap();
                ring.chunks.clear();
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
