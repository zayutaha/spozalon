use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicU16, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use tokio::net::UdpSocket;
use tokio::sync::mpsc;

use crate::capture;
use crate::config::*;
use crate::protocol::Packet;

/// Run the sender: capture audio and stream it over UDP.
pub async fn run(bind: &str, port: u16, _device_name: Option<&str>, verbose: bool) -> Result<()> {
    let addr = format!("{}:{}", bind, port);

    // --- Set up audio capture channel ---
    let (audio_tx, mut audio_rx) = mpsc::channel::<Vec<f32>>(64);
    let running = Arc::new(AtomicBool::new(true));

    // --- Start PipeWire capture ---
    let _capture_child = capture::start_capture(audio_tx, running.clone(), verbose)?;

    if verbose {
        eprintln!("[sender] Audio capture started (PipeWire)");
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
                                // Send handshake response so receiver knows we're alive
                                let resp = Packet::handshake(0);
                                let resp_bytes = resp.serialize();
                                let _ = socket.send_to(&resp_bytes, addr).await;
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

        // Drain ALL pending audio and send each chunk immediately
        loop {
            match audio_rx.try_recv() {
                Ok(samples) => {
                    if !running.load(Ordering::SeqCst) {
                        break;
                    }
                    if samples.is_empty() {
                        continue;
                    }

                    let pkt = Packet {
                        sequence,
                        timestamp_ns: stream_start.elapsed().as_nanos() as u64,
                        sample_count: (samples.len() / 2) as u32,
                        sample_rate: SAMPLE_RATE,
                        volume_percent: vol,
                        pcm_data: samples,
                    };

                    let bytes = pkt.serialize();
                    match socket.send_to(&bytes, addr).await {
                        Ok(_) => {
                            sequence = sequence.wrapping_add(1);
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
                Err(_) => break, // Channel empty, done for this tick
            }
        }

        // Yield briefly to avoid busy-spinning when no audio is playing
        tokio::time::sleep(Duration::from_millis(1)).await;
    }

    if verbose {
        eprintln!("[sender] Stopped. Sent {} packets.", sequence);
    }

    Ok(())
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
