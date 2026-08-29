/// Magic bytes identifying spozalon packets
pub const MAGIC: &[u8; 4] = b"SPOZ";

/// Default UDP port for audio streaming
pub const DEFAULT_PORT: u16 = 44100;

/// Audio sample rate in Hz
pub const SAMPLE_RATE: u32 = 48000;

/// Number of audio channels
pub const CHANNELS: u16 = 2;

/// Disconnect timeout before pausing (ms)
pub const DISCONNECT_TIMEOUT_MS: u64 = 2000;

/// Handshake retry interval (ms)
pub const HANDSHAKE_RETRY_MS: u64 = 1000;

/// Ring buffer size in chunks (~10ms)
pub const RING_BUFFER_CHUNKS: usize = 1;

/// Volume polling interval (ms)
pub const VOLUME_POLL_MS: u64 = 100;

/// Packet header size in bytes (4 magic + 4 seq + 8 ts + 4 samples + 4 rate + 2 vol + 2 reserved)
pub const HEADER_SIZE: usize = 28;
