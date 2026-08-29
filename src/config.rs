/// Magic bytes identifying spozalon packets
pub const MAGIC: &[u8; 4] = b"SPOZ";

#[allow(dead_code)]
/// Default UDP port for audio streaming
pub const DEFAULT_PORT: u16 = 44100;

#[allow(dead_code)]
/// Audio sample rate in Hz
pub const SAMPLE_RATE: u32 = 48000;

#[allow(dead_code)]
/// Number of audio channels
pub const CHANNELS: u16 = 2;

/// Samples per packet (40ms at 48kHz)
pub const CHUNK_SAMPLES: usize = 1920;

/// Disconnect timeout before pausing (ms)
pub const DISCONNECT_TIMEOUT_MS: u64 = 2000;

/// Handshake retry interval (ms)
pub const HANDSHAKE_RETRY_MS: u64 = 1000;

/// Ring buffer size in chunks (~160ms)
pub const RING_BUFFER_CHUNKS: usize = 4;

#[allow(dead_code)]
/// Drift correction threshold in nanoseconds (±5ms)
pub const DRIFT_THRESHOLD_NS: i64 = 5_000_000;

/// Volume polling interval (ms)
pub const VOLUME_POLL_MS: u64 = 100;

/// Packet header size in bytes (4 magic + 4 seq + 8 ts + 4 samples + 4 rate + 2 vol + 2 reserved)
pub const HEADER_SIZE: usize = 28;
