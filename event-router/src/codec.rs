//! UDP payload codec.
//!
//! Decodes JSON-encoded telemetry messages from ESP32-S3 devices.

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// A raw telemetry message as received over UDP.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct UdpTelemetryMessage {
    /// Protocol version. Currently only `1` is accepted.
    pub version: u8,
    /// Globally unique device identifier.
    pub device_uid: String,
    /// Plant UUID this device is monitoring.
    pub plant_id: String,
    /// Monotonic sequence number (wraps at u32::MAX).
    pub seq: u32,
    /// Unix nanoseconds timestamp of the reading.
    pub timestamp_ns: i64,

    pub soil_moisture:       Option<f64>,
    pub ambient_light_lux:   Option<f64>,
    pub ambient_humidity_rh: Option<f64>,
    pub ambient_temp_c:      Option<f64>,
}

#[derive(Debug, Error)]
pub enum DecodeError {
    #[error("JSON decode error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("unsupported protocol version {0}")]
    UnsupportedVersion(u8),
    #[error("device_uid is empty")]
    EmptyDeviceUid,
    #[error("plant_id is empty")]
    EmptyPlantId,
}

/// Decode a UDP payload into a [`UdpTelemetryMessage`].
pub fn decode(bytes: &[u8]) -> Result<UdpTelemetryMessage, DecodeError> {
    let msg: UdpTelemetryMessage = serde_json::from_slice(bytes)?;

    if msg.version != 1 {
        return Err(DecodeError::UnsupportedVersion(msg.version));
    }
    if msg.device_uid.trim().is_empty() {
        return Err(DecodeError::EmptyDeviceUid);
    }
    if msg.plant_id.trim().is_empty() {
        return Err(DecodeError::EmptyPlantId);
    }

    Ok(msg)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_payload() -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({
            "version": 1,
            "device_uid": "esp32-abc",
            "plant_id": "550e8400-e29b-41d4-a716-446655440000",
            "seq": 42,
            "timestamp_ns": 1_700_000_000_000_000_000_i64,
            "soil_moisture": 55.0,
            "ambient_temp_c": 22.5
        }))
        .unwrap()
    }

    #[test]
    fn decode_valid_payload() {
        let msg = decode(&valid_payload()).unwrap();
        assert_eq!(msg.device_uid, "esp32-abc");
        assert_eq!(msg.seq, 42);
        assert_eq!(msg.soil_moisture, Some(55.0));
        assert_eq!(msg.ambient_temp_c, Some(22.5));
        assert_eq!(msg.ambient_light_lux, None);
    }

    #[test]
    fn decode_invalid_json() {
        assert!(matches!(decode(b"not json"), Err(DecodeError::Json(_))));
    }

    #[test]
    fn decode_wrong_version() {
        let bytes = serde_json::to_vec(&serde_json::json!({
            "version": 99,
            "device_uid": "dev",
            "plant_id": "pid",
            "seq": 1,
            "timestamp_ns": 0
        }))
        .unwrap();
        assert!(matches!(decode(&bytes), Err(DecodeError::UnsupportedVersion(99))));
    }

    #[test]
    fn decode_empty_device_uid() {
        let bytes = serde_json::to_vec(&serde_json::json!({
            "version": 1,
            "device_uid": "",
            "plant_id": "pid",
            "seq": 1,
            "timestamp_ns": 0
        }))
        .unwrap();
        assert!(matches!(decode(&bytes), Err(DecodeError::EmptyDeviceUid)));
    }

    #[test]
    fn decode_empty_plant_id() {
        let bytes = serde_json::to_vec(&serde_json::json!({
            "version": 1,
            "device_uid": "dev",
            "plant_id": "",
            "seq": 1,
            "timestamp_ns": 0
        }))
        .unwrap();
        assert!(matches!(decode(&bytes), Err(DecodeError::EmptyPlantId)));
    }
}
