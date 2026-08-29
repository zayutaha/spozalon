# SPOZALON — Technical Plan

## 1. Wire Protocol (protocol.rs)

Fixed 24-byte header + variable PCM payload per UDP packet.

```
Offset  Size  Field            Description
0       4     magic            b"SPOZ"
4       4     sequence         u32 monotonically increasing
8       8     timestamp        u64 nanoseconds since stream start
16      4     sample_count     u32 samples in this packet
20      2     volume_percent   u16 0-100
22      2     reserved         padding for alignment
24      N     pcm_data         f32 interleaced stereo
```

Typical packet: 24 + 1920*8 = 15,384 bytes. 25 packets/sec at 48kHz.

Serialization: manual write to `Vec<u8>` / parse from `&[u8]`. No serde overhead.

## 2. Sender (sender.rs)

### Audio Capture
- `cpal` default input config on the **monitor** of default output sink.
- On PipeWire: monitor source is typically "<sink_name>.monitor".
- List all input devices, find one containing "monitor" in name.
- Fallback: use first available input device, print warning.
- Buffer: capture in chunks of 1920 samples (40ms).
- cpal callback pushes into `std::sync::mpsc::SyncSender`.

### Volume Reading
- Poll `pactl get-sink-volume @DEFAULT_SINK@` every 100ms via `std::process::Command`.
- Parse percentage from output: "Volume: front-left: 32768 /  50% / ..."
- Store as `AtomicU16`, read in packet construction.

### Network
- `tokio::net::UdpSocket` bound to `0.0.0.0:44100`.
- Listen for handshake: packet with magic "SPOZ" and empty PCM (sample_count=0).
- On handshake: record receiver address, begin streaming.
- Stream loop: receive audio chunks from channel, wrap in packet, send.
- If no data for 1s, send zero-filled packet (keepalive).
- Disconnect: if no handshake response for 2s, pause and re-listen.

### Lifecycle
- SIGINT/SIGTERM via `tokio::signal::ctrl_c()`.
- On shutdown: close audio device, close socket, exit 0.

## 3. Receiver (receiver.rs)

### Network
- `tokio::net::UdpSocket` bound to `0.0.0.0:44100`.
- Send handshake packet to sender IP:44100. Retry every 1s until response.
- Receive loop: parse packet, validate magic, check sequence continuity.
- Push valid PCM data into ring buffer.

### Drift Correction
- Track `expected_timestamp`: advances at real-time rate from first packet.
- Each packet provides `packet_timestamp`.
- `drift = packet_timestamp - expected_timestamp` (in nanoseconds).
- If drift > +5ms: pad with interpolated samples (slow down).
- If drift < -5ms: skip samples (speed up).
- Correction is per-sample interpolation to avoid clicks.

### Ring Buffer
- Fixed-size circular buffer: 4 chunks = 4 * 1920 * 8 bytes = ~61KB.
- Producer: network thread (tokio).
- Consumer: audio playback thread (cpal callback).
- `Arc<Mutex<Vec<f32>>>` or lock-free ring buffer.
- Underflow: output silence.
- Overflow: drop oldest chunk.

### Audio Playback
- `cpal` default output device (CoreAudio on macOS).
- Output stream at 48kHz / f32 / stereo.
- Callback reads from ring buffer.
- On disconnect: drain buffer, output silence, retry handshake.

### Volume Application
- Each packet carries `volume_percent` (0-100).
- Map to CoreAudio amplitude: `amplitude = volume / 100.0`.
- Apply as multiplier in playback callback: `sample * amplitude`.
- Alternative: use CoreAudio API to set hardware volume directly (cleaner).

## 4. CLI (cli.rs)

Uses `clap` derive macros.

```
spozalon send [--port 44100] [--bind 0.0.0.0] [--device NAME] [--verbose]
spozalon recv <SENDER_IP> [--port 44100] [--device NAME] [--verbose]
```

Single binary with subcommands via `clap` enum.

## 5. Config (config.rs)

```rust
pub const MAGIC: &[u8; 4] = b"SPOZ";
pub const DEFAULT_PORT: u16 = 44100;
pub const SAMPLE_RATE: u32 = 48000;
pub const CHANNELS: u16 = 2;
pub const CHUNK_SAMPLES: usize = 1920;        // 40ms at 48kHz
pub const DISCONNECT_TIMEOUT_MS: u64 = 2000;
pub const HANDSHAKE_RETRY_MS: u64 = 1000;
pub const RING_BUFFER_CHUNKS: usize = 4;      // ~160ms
pub const DRIFT_THRESHOLD_NS: i64 = 5_000_000; // ±5ms
pub const VOLUME_POLL_MS: u64 = 100;
```

## 6. Build

Single `Cargo.toml` with one binary `spozalon` using subcommands.

cpal features: `alsa` on Linux (enabled via target cfg), CoreAudio default on macOS.

Each machine builds natively — no cross-compilation needed.

## 7. Service Files

### spozalon-send.service (systemd)
```ini
[Unit]
Description=Spozalon Audio Sender
After=pipewire.service

[Service]
Type=simple
ExecStart=/usr/local/bin/spozalon send
Restart=on-failure
RestartSec=5

[Install]
WantedBy=multi-user.target
```

### com.spozalon.recv.plist (launchd)
```xml
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN"
  "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>com.spozalon.recv</string>
    <key>ProgramArguments</key>
    <array>
        <string>/usr/local/bin/spozalon</string>
        <string>recv</string>
        <string>10.0.1.1</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <true/>
    <key>StandardOutPath</key>
    <string>/tmp/spozalon.log</string>
    <key>StandardErrorPath</key>
    <string>/tmp/spozalon.log</string>
</dict>
</plist>
```
