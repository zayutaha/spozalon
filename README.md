# SPOZALON

Stream your Linux machine's audio to your MacBook's speakers over a direct
Thunderbolt 3 USB-C cable with <10ms latency.

## What is this?

Spozalon captures all system audio on a Linux machine and plays it through a
MacBook Pro's speakers in real-time. The two machines are connected via a
Thunderbolt 3 USB-C cable, which creates a private 10+ Gbps network link with
sub-millisecond latency.

**Features:**
- 48kHz / 32-bit float stereo audio
- <10ms end-to-end latency (imperceptible)
- Volume sync from Linux to macOS
- Automatic disconnect/reconnect handling
- Clean shutdown (no leftover config on macOS)

## Hardware Required

- Mac Mini 2018 (or any Linux machine with Thunderbolt 3)
- MacBook Pro (any with Thunderbolt 3/4)
- Thunderbolt 3 USB-C cable (not a regular USB-C cable — must be Thunderbolt)

## Setup

### 1. Configure Thunderbolt Networking

**On Linux (Mac Mini):**
```bash
# Load thunderbolt network module
sudo modprobe thunderbolt_net

# Set a static IP on the thunderbolt interface
# (interface name may vary, check `ip link`)
sudo ip addr add 10.0.1.1/24 dev thunderbolt0
sudo ip link set thunderbolt0 up
```

**On macOS (MacBook Pro):**
1. Plug in the Thunderbolt cable
2. Open System Settings → Network
3. You should see "Thunderbolt Bridge" — add it if not present
4. Set IP manually: 10.0.1.2, subnet mask: 255.255.255.0

**Verify connection:**
```bash
# From Linux:
ping 10.0.1.2

# From macOS:
ping 10.0.1.1
```

### 2. Build & Install

**On Linux:**
```bash
# Install dependencies (Arch)
sudo pacman -S alsa-lib

# Build
cargo build --release

# Install (optional)
sudo cp target/release/spozalon /usr/local/bin/
```

**On macOS:**
```bash
# Build
cargo build --release

# Install (optional)
sudo cp target/release/spozalon /usr/local/bin/
```

### 3. Run

**On Linux (start first):**
```bash
spozalon send --verbose
```

**On macOS:**
```bash
spozalon recv 10.0.1.1 --verbose
```

Play any audio on Linux — it will come out of the MacBook's speakers.

## Usage

```
spozalon send [OPTIONS]
  --port <PORT>       UDP port [default: 44100]
  --bind <ADDR>       Bind address [default: 0.0.0.0]
  --device <NAME>     Capture device (default: auto-detect monitor)
  --verbose           Print status to stderr

spozalon recv <SENDER_IP> [OPTIONS]
  --port <PORT>       UDP port [default: 44100]
  --device <NAME>     Playback device (default: system default)
  --verbose           Print status to stderr
```

## Auto-Start (Optional)

### Linux (systemd)
```bash
sudo cp spozalon-send.service /etc/systemd/system/
sudo systemctl enable --now spozalon-send
```

### macOS (launchd)
```bash
cp com.spozalon.recv.plist ~/Library/LaunchAgents/
launchctl load ~/Library/LaunchAgents/com.spozalon.recv.plist
```

## How It Works

1. **Thunderbolt Networking:** The USB-C Thunderbolt cable creates a virtual
   Ethernet interface on both machines, forming a private 10+ Gbps LAN.

2. **Audio Capture:** On Linux, spozalon captures from PipeWire's monitor source
   — this is whatever is playing on the default audio output, without touching
   the audio graph.

3. **UDP Streaming:** Audio is sent as raw PCM packets (48kHz, 32-bit float,
   stereo) over UDP port 44100. Each packet carries 40ms of audio (~15KB).

4. **Playback:** On macOS, the receiver plays the stream through CoreAudio's
   default output device. A ring buffer absorbs jitter.

5. **Sync:** Volume changes on Linux are sent as metadata in each packet and
   applied on the macOS side.

## Latency Budget

| Stage | Latency |
|---|---|
| PipeWire capture buffer | ~2-3ms |
| Serialization + send | <0.1ms |
| Thunderbolt network | ~0.05ms |
| Kernel network stack | ~0.3ms |
| CoreAudio playback buffer | ~2-3ms |
| **Total** | **~5-7ms** |

## License

TBD
