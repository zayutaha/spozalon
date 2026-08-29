# SPOZALON — Spec

## Goal

Stream all system audio from a Linux machine (Mac Mini 2018) to a MacBook Pro's
speakers over a direct Thunderbolt 3 USB-C cable, with <10ms end-to-end latency.

Two binaries: `spozalon send` (Linux, captures audio) and `spozalon recv` (macOS,
plays audio). Sender waits for receiver, handles disconnects silently, and cleans
up fully on exit.

- 48kHz / 32-bit float / stereo raw PCM over UDP
- Fixed port 44100, static IPs on Thunderbolt network (10.0.1.1 / 10.0.1.2)
- No encryption, no mDNS discovery, no virtual audio devices
- PipeWire monitor capture on Linux, CoreAudio default output on macOS
- Volume sync: Linux volume level sent in each packet, applied on macOS
- Drift correction: timestamp-based sync with ±5ms threshold
- Optional systemd unit + launchd agent for auto-start

## Testing

### Unit Tests (run anywhere, no hardware needed)
- Packet serialization/deserialize round-trip
- Drift calculation with mock timestamps
- Volume level encoding/decoding

### Integration Tests (single machine, virtual audio devices)
- Linux: create PipeWire null sink, play test tone into it, capture from monitor,
  verify audio data is received correctly
- macOS: send fake UDP packets with sine wave, verify they play through speakers

### Manual Tests (two machines required)
- **Basic:** Play music on Linux, hear it on MacBook speakers
- **Latency:** Clap on Linux, measure offset on MacBook recording (<10ms target)
- **Drift:** Stream for 1+ hour, verify no pitch shift or stuttering
- **Disconnect:** Unplug cable mid-stream, verify auto-resume within 2s
- **Kill sender:** `kill` sender process, verify Mac outputs silence gracefully
- **Kill receiver:** `kill` receiver process, verify sender pauses without spam
- **Volume:** Change volume on Linux, verify MacBook volume follows
- **Repeated start/stop:** Start/stop 10 times, no port conflicts or leaks
- **Long run:** Stream 4+ hours, no memory growth or audio degradation

### Test Commands (Linux)
```bash
# Create virtual sink for testing
pactl load-module module-null-sink sink_name=spozalon_test \
  sink_properties=device.description=SpozalonTest

# Play test tone into virtual sink
pw-play --device=spozalon_test /usr/share/sounds/freedesktop/stereo/bell.oga

# Run sender against virtual sink
cargo run -- send --device spozalon_test.monitor --verbose
```

## Constraints

- Must compile on Linux (x86_64) and macOS (aarch64 / x86_64)
- cpal 0.18+ handles cross-platform audio — no platform-specific code needed
- No runtime dependencies beyond PipeWire/CoreAudio defaults
- Sender must not crash or spam logs if receiver disconnects
- When spozalon exits, macOS audio state must be identical to before it started
- UDP is acceptable — Thunderbolt cable has negligible packet loss
- 40ms chunk size (1920 samples) is batching interval, not latency
- Ring buffer on receiver = 4 chunks (~160ms) absorbs jitter
- Drift correction threshold: ±5ms (below human perception)
- Volume synced via metadata in each packet (25Hz update rate)
