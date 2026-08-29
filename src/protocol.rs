use crate::config::{HEADER_SIZE, MAGIC};

/// A single audio packet sent over UDP.
#[derive(Debug, Clone)]
pub struct Packet {
    pub sequence: u32,
    pub timestamp_ns: u64,
    pub sample_count: u32,
    pub sample_rate: u32,
    pub volume_percent: u16,
    pub pcm_data: Vec<f32>,
}

impl Packet {
    /// Create a handshake packet (empty PCM, sample_count=0).
    pub fn handshake(sequence: u32) -> Self {
        Self {
            sequence,
            timestamp_ns: 0,
            sample_count: 0,
            sample_rate: 0,
            volume_percent: 0,
            pcm_data: Vec::new(),
        }
    }

    #[allow(dead_code)]
    /// Create a handshake packet with volume (for initial volume sync).
    pub fn handshake_with_volume(sequence: u32, volume: u16) -> Self {
        Self {
            sequence,
            timestamp_ns: 0,
            sample_count: 0,
            sample_rate: 0,
            volume_percent: volume,
            pcm_data: Vec::new(),
        }
    }

    /// Check if this is a handshake packet.
    pub fn is_handshake(&self) -> bool {
        self.sample_count == 0
    }

    /// Serialize packet to bytes for UDP transmission.
    pub fn serialize(&self) -> Vec<u8> {
        let pcm_bytes = self.pcm_data.len() * 4; // f32 = 4 bytes
        let mut buf = Vec::with_capacity(HEADER_SIZE + pcm_bytes);

        // Magic
        buf.extend_from_slice(MAGIC);
        // Sequence
        buf.extend_from_slice(&self.sequence.to_le_bytes());
        // Timestamp
        buf.extend_from_slice(&self.timestamp_ns.to_le_bytes());
        // Sample count
        buf.extend_from_slice(&self.sample_count.to_le_bytes());
        // Sample rate
        buf.extend_from_slice(&self.sample_rate.to_le_bytes());
        // Volume percent
        buf.extend_from_slice(&self.volume_percent.to_le_bytes());
        // Reserved (2 bytes padding)
        buf.extend_from_slice(&[0, 0]);
        // PCM data
        for sample in &self.pcm_data {
            buf.extend_from_slice(&sample.to_le_bytes());
        }

        buf
    }

    /// Deserialize bytes into a Packet. Returns None if data is invalid.
    pub fn deserialize(data: &[u8]) -> Option<Self> {
        if data.len() < HEADER_SIZE {
            return None;
        }

        // Validate magic
        if &data[0..4] != MAGIC {
            return None;
        }

        let sequence = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
        let timestamp_ns = u64::from_le_bytes([
            data[8], data[9], data[10], data[11], data[12], data[13], data[14], data[15],
        ]);
        let sample_count = u32::from_le_bytes([data[16], data[17], data[18], data[19]]);
        let sample_rate = u32::from_le_bytes([data[20], data[21], data[22], data[23]]);
        let volume_percent = u16::from_le_bytes([data[24], data[25]]);
        // data[26..28] is reserved padding

        // Validate PCM data length
        let expected_pcm_bytes = sample_count as usize * 8; // stereo f32 = 8 bytes per frame
        let actual_pcm_bytes = data.len() - HEADER_SIZE;
        if actual_pcm_bytes < expected_pcm_bytes {
            return None;
        }

        // Parse PCM samples
        let mut pcm_data = Vec::with_capacity(sample_count as usize * 2);
        for i in 0..sample_count as usize {
            let offset = HEADER_SIZE + i * 8;
            let left = f32::from_le_bytes([
                data[offset],
                data[offset + 1],
                data[offset + 2],
                data[offset + 3],
            ]);
            let right = f32::from_le_bytes([
                data[offset + 4],
                data[offset + 5],
                data[offset + 6],
                data[offset + 7],
            ]);
            pcm_data.push(left);
            pcm_data.push(right);
        }

        Some(Self {
            sequence,
            timestamp_ns,
            sample_count,
            sample_rate,
            volume_percent,
            pcm_data,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_handshake_packet_round_trip() {
        let pkt = Packet::handshake(42);
        let bytes = pkt.serialize();
        let decoded = Packet::deserialize(&bytes).unwrap();

        assert!(decoded.is_handshake());
        assert_eq!(decoded.sequence, 42);
        assert_eq!(decoded.timestamp_ns, 0);
        assert_eq!(decoded.pcm_data.len(), 0);
    }

    #[test]
    fn test_data_packet_round_trip() {
        let pcm = vec![0.5, -0.5, 1.0, -1.0, 0.0, 0.25];
        let pkt = Packet {
            sequence: 100,
            timestamp_ns: 1_234_567_890,
            sample_count: 3,
            sample_rate: 48000,
            volume_percent: 75,
            pcm_data: pcm.clone(),
        };

        let bytes = pkt.serialize();
        let decoded = Packet::deserialize(&bytes).unwrap();

        assert!(!decoded.is_handshake());
        assert_eq!(decoded.sequence, 100);
        assert_eq!(decoded.timestamp_ns, 1_234_567_890);
        assert_eq!(decoded.sample_count, 3);
        assert_eq!(decoded.sample_rate, 48000);
        assert_eq!(decoded.volume_percent, 75);
        assert_eq!(decoded.pcm_data, pcm);
    }

    #[test]
    fn test_too_short_data_returns_none() {
        assert!(Packet::deserialize(&[0u8; 10]).is_none());
    }

    #[test]
    fn test_bad_magic_returns_none() {
        let mut data = vec![0u8; HEADER_SIZE + 16];
        data[0..4].copy_from_slice(b"NOPE");
        assert!(Packet::deserialize(&data).is_none());
    }

    #[test]
    fn test_pcm_length_mismatch_returns_none() {
        let mut data = vec![0u8; HEADER_SIZE + 8];
        data[0..4].copy_from_slice(b"SPOZ");
        data[16..20].copy_from_slice(&2u32.to_le_bytes()); // claim 2 frames
        // but only 8 bytes of PCM (1 frame)
        assert!(Packet::deserialize(&data).is_none());
    }

    #[test]
    fn test_volume_preserved() {
        let pkt = Packet {
            sequence: 1,
            timestamp_ns: 0,
            sample_count: 1,
            sample_rate: 48000,
            volume_percent: 42,
            pcm_data: vec![0.0, 0.0],
        };
        let bytes = pkt.serialize();
        let decoded = Packet::deserialize(&bytes).unwrap();
        assert_eq!(decoded.volume_percent, 42);
    }
}
