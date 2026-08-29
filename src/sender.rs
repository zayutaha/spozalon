use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicU16, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use tokio::net::UdpSocket;
use tokio::sync::mpsc;

use crate::config::*;
use crate::protocol::Packet;

/// Run the sender: capture audio and stream it over UDP.
pub async fn run(bind: &str, port: u16, device_name: Option<&str>, verbose: bool) -> Result<()> {
    let addr = format!("{}:{}", bind, port);

    // --- Find audio capture device ---
    let host = cpal::default_host();
    let device = find_capture_device(&host, device_name)?;

    let dev_name = device
        .description()
        .map(|d| d.name().to_string())
        .unwrap_or_else(|_| "unknown".into());
    if verbose {
        eprintln!("[sender] Using device: {}", dev_name);
    }

    let supported_config = device
        .default_input_config()
        .context("Failed to get default input config")?;

    if verbose {
        eprintln!(
            "[sender] Audio config: {} Hz, {:?}, {:?}",
            supported_config.sample_rate(),
            supported_config.channels(),
            supported_config.sample_format()
        );
    }

    let sample_rate = supported_config.sample_rate();
    let channels = supported_config.channels() as usize;
    let stream_config = supported_config.config();

    // --- Set up audio capture channel ---
    let (audio_tx, mut audio_rx) = mpsc::channel::<Vec<f32>>(RING_BUFFER_CHUNKS * 2);

    let stream = device.build_input_stream(
        stream_config,
        move |data: &[f32], _: &cpal::InputCallbackInfo| {
            let samples: Vec<f32> = if channels == 2 {
                data.to_vec()
            } else if channels == 1 {
                data.iter().flat_map(|&s| [s, s]).collect()
            } else {
                data.chunks(channels)
                    .take(CHUNK_SAMPLES)
                    .flat_map(|frame| {
                        let l = frame.get(0).copied().unwrap_or(0.0);
                        let r = frame.get(1).copied().unwrap_or(0.0);
                        [l, r]
                    })
                    .collect()
            };

            for frame in samples.chunks(CHUNK_SAMPLES * 2) {
                if frame.len() == CHUNK_SAMPLES * 2 {
                    let _ = audio_tx.try_send(frame.to_vec());
                }
            }
        },
        move |err| {
            eprintln!("[sender] Audio capture error: {}", err);
        },
        None,
    )?;

    stream.play().context("Failed to start audio capture")?;

    if verbose {
        eprintln!("[sender] Audio capture started");
    }

    // --- Volume polling ---
    let volume = Arc::new(AtomicU16::new(100));
    let volume_clone = volume.clone();
    tokio::spawn(async move {
        loop {
            let vol = read_volume();
            volume_clone.store(vol, Ordering::Relaxed);
            tokio::time::sleep(Duration::from_millis(VOLUME_POLL_MS)).await;
        }
    });

    // --- Network: UDP socket ---
    let socket = Arc::new(
        UdpSocket::bind(&addr)
            .await
            .with_context(|| format!("Failed to bind to {}. Is another spozalon running?", addr))?,
    );

    if verbose {
        eprintln!("[sender] Listening on {}", addr);
    }

    let running = Arc::new(AtomicBool::new(true));
    let running_clone = running.clone();
    let verbose_flag = verbose;

    tokio::spawn(async move {
        let _ = tokio::signal::ctrl_c().await;
        if verbose_flag {
            eprintln!("\n[sender] Shutting down...");
        }
        running_clone.store(false, Ordering::SeqCst);
    });

    let mut receiver_addr = None;
    let mut sequence: u32 = 0;
    let stream_start = Instant::now();
    let mut buf = [0u8; 65536];

    loop {
        if !running.load(Ordering::SeqCst) {
            break;
        }

        // --- Phase 1: Wait for handshake ---
        if receiver_addr.is_none() {
            if verbose {
                eprintln!("[sender] Waiting for receiver...");
            }

            loop {
                if !running.load(Ordering::SeqCst) {
                    break;
                }

                match tokio::time::timeout(
                    Duration::from_millis(500),
                    socket.recv_from(&mut buf),
                )
                .await
                {
                    Ok(Ok((len, addr))) => {
                        if let Some(pkt) = Packet::deserialize(&buf[..len]) {
                            if pkt.is_handshake() {
                                receiver_addr = Some(addr);
                                if verbose {
                                    eprintln!("[sender] Receiver connected from {}", addr);
                                }
                                break;
                            }
                        }
                    }
                    Ok(Err(e)) => {
                        if verbose {
                            eprintln!("[sender] Socket error: {}", e);
                        }
                        tokio::time::sleep(Duration::from_millis(100)).await;
                    }
                    Err(_) => {} // Timeout, loop again
                }
            }

            if receiver_addr.is_none() {
                continue;
            }
        }

        // --- Phase 2: Stream audio ---
        let addr = receiver_addr.unwrap();
        let vol = volume.load(Ordering::Relaxed);
        let mut sent_any = false;

        while let Ok(samples) = audio_rx.try_recv() {
            if !running.load(Ordering::SeqCst) {
                break;
            }

            let pkt = Packet {
                sequence,
                timestamp_ns: stream_start.elapsed().as_nanos() as u64,
                sample_count: (samples.len() / 2) as u32,
                sample_rate,
                volume_percent: vol,
                pcm_data: samples,
            };

            let bytes = pkt.serialize();
            match socket.send_to(&bytes, addr).await {
                Ok(_) => {
                    sequence = sequence.wrapping_add(1);
                    sent_any = true;
                }
                Err(e) => {
                    if verbose {
                        eprintln!("[sender] Send error: {}", e);
                    }
                    receiver_addr = None;
                    break;
                }
            }
        }

        if !sent_any {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }

    if verbose {
        eprintln!("[sender] Stopped. Sent {} packets.", sequence);
    }

    Ok(())
}

/// Find a suitable audio capture device.
fn find_capture_device(host: &cpal::Host, name: Option<&str>) -> Result<cpal::Device> {
    if let Some(name) = name {
        let devices = host
            .input_devices()
            .context("Failed to enumerate input devices")?;

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

    // Auto-detect: find a device with "monitor" in the name
    let devices = host
        .input_devices()
        .context("Failed to enumerate input devices")?;

    for device in devices {
        let is_monitor = device
            .description()
            .map(|d| d.name().to_lowercase().contains("monitor"))
            .unwrap_or(false);
        if is_monitor {
            return Ok(device);
        }
    }

    // Fallback: use default input device
    host.default_input_device()
        .context("No audio input devices found. Is PipeWire running?")
}

/// Read system volume via pactl.
fn read_volume() -> u16 {
    let output = Command::new("pactl")
        .args(["get-sink-volume", "@DEFAULT_SINK@"])
        .output();

    match output {
        Ok(o) if o.status.success() => {
            let stdout = String::from_utf8_lossy(&o.stdout);
            for part in stdout.split('/') {
                let part = part.trim();
                if let Some(pct_str) = part.strip_suffix('%') {
                    if let Ok(pct) = pct_str.trim().parse::<u16>() {
                        return pct.min(100);
                    }
                }
            }
            100
        }
        _ => 100,
    }
}
