# SPOZALON — Implementation Notes

_Use this file to log discoveries, surprises, workarounds, and anything that
doesn't fit neatly into the spec or plan. Update as you go._

## Known Unknowns (pre-implementation)
- cpal 0.18 may or may not see PipeWire monitor sources directly. If not,
  may need to fall back to `pactl` CLI to list sources, or use `pipewire` crate.
- macOS CoreAudio behavior with cpal default output — need to verify writing to
  default output doesn't interfere with other audio.

## Thunderbolt Networking
- Mac Mini 2018 has Thunderbolt 3 (Intel Alpine Ridge). Works with `thunderbolt_net`
  on Linux kernel 5.x+.
- macOS: Thunderbolt Bridge appears in Network settings when cable is plugged in.
  May need to manually set IP first time.
- iperf3 over Thunderbolt 3: ~13 Gbps. More than enough for 3 MB/s audio stream.

## cpal Notes
- cpal 0.18 added native PipeWire support (previously ALSA-only on Linux).
- Monitor source naming varies by distro. Common patterns:
  - `Monitor of Built-in Audio Analog Stereo`
  - `<sink_name>.monitor`
  - `alsa_output.pci-...analog-stereo.monitor`
- Strategy: list all input devices, find one with "monitor" (case-insensitive) in name.
- On macOS, cpal CoreAudio backend is well-tested. Output-only is safe.

## UDP Considerations
- No flow control — if receiver is slow, kernel drops packets.
- With Thunderbolt bandwidth, packet loss is essentially impossible for our size.
- No jumbo frames needed — 15KB packets well under standard MTU.

## Errors to Watch For
- "Device not available" — audio device unplugged or PipeWire not running.
- "Address already in use" on port 44100 — another instance running.
- macOS mic permission dialog — only if cpal opens input device (we don't).
- PipeWire not running on Linux — `pactl` commands fail silently.

## Latency Tuning (post-MVP)
- If measured latency >10ms:
  1. PipeWire quantum: default 1024 = 21ms. Reduce to 128 or 256.
  2. CoreAudio buffer: cpal default 512-1024. Reduce to 128-256.
  3. Ring buffer: currently 4 chunks = 160ms. Reduce to 2.
- These are runtime/config changes, not code changes.

## Volume Sync Approach
- Send volume_percent (0-100) in each packet header.
- Receiver applies as amplitude multiplier in playback callback.
- Alternative (better): use CoreAudio API to set hardware output volume directly.
- User can still override with MacBook volume keys locally.

## Drift Correction Approach
- Each packet has timestamp (nanos since stream start).
- Receiver tracks expected_timestamp (advances at 1 sample = 20.8µs).
- drift = packet_timestamp - expected_timestamp.
- |drift| > 5ms: interpolate samples to speed up/slow down.
- 5ms threshold well below perception (~30ms for audio desync).
- At ±10ppm clock drift: ~36ms/hour. Correction fires every ~10-15 minutes.

## Things That Surprised Me

### cpal 0.18 API Changes (from 0.17)
- `device.name()` removed → use `device.description().name()` (returns `&str`)
- `device.description()` returns `Result<DeviceDescription>`, not `Result<String>`
- `DeviceDescription` is a struct with `.name()`, `.manufacturer()`, `.device_type()`, etc.
- `build_input_stream` / `build_output_stream` take `StreamConfig` by VALUE, not reference
- `sample_rate()` returns `u32` directly, not `SampleRate(u32)`
- `stream.play()` still works but `stream.start()` is preferred (0.19 deprecates play)

### HEADER_SIZE Bug
- Initially set to 24 bytes, but actual header is 28 (volume=2 + reserved=2 extra)
- Caused test_data_packet_round_trip to fail: first PCM sample was garbage
- Fix: `pub const HEADER_SIZE: usize = 28;`

### tokio mpsc vs std mpsc
- `std::sync::mpsc::sync_channel` exists but is synchronous/blocking
- cpal callbacks run on a separate thread, need async channel to bridge to tokio
- Solution: `tokio::sync::mpsc::channel` (async, bounded)

### Volume Reading on Linux
- `pactl get-sink-volume @DEFAULT_SINK@` returns multi-line output
- Parse first occurrence of `XX%` in the output
- Fallback to 100% if pactl not available (e.g., on macOS during development)

### What We Can't Test on macOS
- Sender: PipeWire is Linux-only, no monitor sources on macOS
- Receiver: Works, but needs a sender to provide audio
- Full integration: requires both machines connected via Thunderbolt

### Current State (end of initial implementation)
- Compiles clean on macOS (aarch64-apple-darwin), zero warnings
- Compiles clean on Arch Linux (amd64) via Docker, zero warnings
- 6/6 unit tests pass on both platforms
- CLI help works for both `send` and `recv` subcommands
- Graceful error when no audio device found (Docker test confirmed)
- PipeWire 1.6.8 installed correctly on Arch
- cpal correctly enumerates ALSA default device on Linux
- Ready for testing on real Linux hardware with PipeWire + Thunderbolt network
