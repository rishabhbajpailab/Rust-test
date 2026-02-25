//! Stable ingest ID generation.
//!
//! The ingest ID is a hex-encoded SHA-256 hash of the canonical fields that
//! uniquely identify a telemetry reading.

use sha2::{Digest, Sha256};

/// Compute a stable ingest ID from telemetry envelope fields.
pub fn compute(device_uid: &str, plant_id: &str, seq: u32, timestamp_ns: i64) -> String {
    let mut hasher = Sha256::new();
    hasher.update(device_uid.as_bytes());
    hasher.update(b"\0");
    hasher.update(plant_id.as_bytes());
    hasher.update(b"\0");
    hasher.update(seq.to_le_bytes());
    hasher.update(timestamp_ns.to_le_bytes());
    hex::encode(hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_inputs_produce_same_id() {
        let id1 = compute("dev-1", "plant-uuid", 42, 1_700_000_000_000_000_000);
        let id2 = compute("dev-1", "plant-uuid", 42, 1_700_000_000_000_000_000);
        assert_eq!(id1, id2);
    }

    #[test]
    fn different_seq_produces_different_id() {
        let id1 = compute("dev-1", "plant-uuid", 42, 1_000_000);
        let id2 = compute("dev-1", "plant-uuid", 43, 1_000_000);
        assert_ne!(id1, id2);
    }

    #[test]
    fn different_timestamp_produces_different_id() {
        let id1 = compute("dev-1", "plant-uuid", 1, 1_000_000);
        let id2 = compute("dev-1", "plant-uuid", 1, 2_000_000);
        assert_ne!(id1, id2);
    }

    #[test]
    fn different_device_uid_produces_different_id() {
        let id1 = compute("dev-1", "plant-uuid", 1, 1_000_000);
        let id2 = compute("dev-2", "plant-uuid", 1, 1_000_000);
        assert_ne!(id1, id2);
    }

    #[test]
    fn id_is_64_hex_chars() {
        let id = compute("dev-1", "plant-uuid", 1, 1_000_000);
        assert_eq!(id.len(), 64);
        assert!(id.chars().all(|c| c.is_ascii_hexdigit()));
    }
}
