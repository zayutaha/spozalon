# SPOZALON — Tasks

## Phase 1: Project Setup
- [x] Write SPEC.md, PLAN.md, TASKS.md, IMPLEMENTATION_NOTES.md
- [x] Update Cargo.toml with dependencies (cpal, tokio, clap, anyhow)
- [x] Create src/config.rs with shared constants
- [x] Create src/cli.rs with clap subcommand definitions
- [x] Create src/main.rs with subcommand dispatch

## Phase 2: Protocol
- [x] Create src/protocol.rs
- [x] Implement Packet struct (header + PCM data + volume)
- [x] Implement serialize (Packet -> Vec<u8>)
- [x] Implement deserialize (&[u8] -> Option<Packet>)
- [x] Unit tests for round-trip serialization
- [x] Unit tests for malformed packet rejection

## Phase 3: Sender
- [x] Create src/sender.rs
- [x] Implement audio capture via cpal (find monitor source)
- [x] Implement volume polling via pactl
- [x] Implement UDP socket + handshake listener
- [x] Implement stream loop (capture -> packetize -> send)
- [x] Implement disconnect detection + silent retry
- [x] Implement graceful shutdown (SIGINT/SIGTERM)

## Phase 4: Receiver
- [x] Create src/receiver.rs
- [x] Implement UDP socket + handshake sender
- [x] Implement receive loop (parse -> validate -> push to ring buffer)
- [x] Implement ring buffer (4 chunks, Mutex-based)
- [x] Implement audio playback via cpal (default output)
- [x] Implement volume application (packet volume -> amplitude multiplier)
- [x] Implement disconnect handling (drain, retry)

## Phase 5: Polish
- [x] Add --verbose flag with connection status logging
- [ ] Create spozalon-send.service (systemd)
- [ ] Create com.spozalon.recv.plist (launchd)
- [x] Write README.md

## Phase 6: Testing
- [x] Unit tests pass: 6/6 protocol tests
- [x] Compiles clean on macOS (aarch64-apple-darwin) with zero warnings
- [ ] Integration test: virtual PipeWire sink capture (needs Linux machine)
- [ ] Manual test: two-machine basic streaming (needs both machines)
- [ ] Manual test: disconnect/reconnect
- [ ] Manual test: volume sync

## Notes
- cpal 0.18 API: `device.description().name()` not `device.name()`
- cpal 0.18 API: `build_input_stream`/`build_output_stream` take `StreamConfig` by value (no `&`)
- cpal 0.18 API: `sample_rate()` returns `u32` directly (not `SampleRate(u32)`)
- cpal 0.18 API: `mpsc::sync_channel` → `tokio::sync::mpsc::channel` (tokio is async)
- HEADER_SIZE is 28 bytes (4 magic + 4 seq + 8 ts + 4 samples + 4 rate + 2 vol + 2 reserved)
