use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// IpcValue — a dynamically-typed value similar to JSON
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum IpcValue {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    String(String),
    Bytes(Vec<u8>),
    Array(Vec<IpcValue>),
    Map(Vec<(String, IpcValue)>),
}

impl IpcValue {
    pub fn as_str(&self) -> Option<&str> {
        match self {
            IpcValue::String(s) => Some(s.as_str()),
            _ => None,
        }
    }

    pub fn as_i64(&self) -> Option<i64> {
        match self {
            IpcValue::Int(v) => Some(*v),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            IpcValue::Bool(v) => Some(*v),
            _ => None,
        }
    }
}

impl From<String> for IpcValue {
    fn from(s: String) -> Self {
        IpcValue::String(s)
    }
}

impl From<&str> for IpcValue {
    fn from(s: &str) -> Self {
        IpcValue::String(s.to_string())
    }
}

impl From<i64> for IpcValue {
    fn from(v: i64) -> Self {
        IpcValue::Int(v)
    }
}

impl From<bool> for IpcValue {
    fn from(v: bool) -> Self {
        IpcValue::Bool(v)
    }
}

impl From<Vec<IpcValue>> for IpcValue {
    fn from(v: Vec<IpcValue>) -> Self {
        IpcValue::Array(v)
    }
}

// ---------------------------------------------------------------------------
// IpcError
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IpcError {
    pub code: u32,
    pub message: String,
}

impl IpcError {
    pub fn new(code: u32, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

impl std::fmt::Display for IpcError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "IPC error {}: {}", self.code, self.message)
    }
}

// Common IPC error codes
pub const ERROR_SERVICE_NOT_FOUND: u32 = 1;
pub const ERROR_METHOD_NOT_FOUND: u32 = 2;
pub const ERROR_INVALID_ARGS: u32 = 3;
pub const ERROR_INTERNAL: u32 = 4;
pub const ERROR_TIMEOUT: u32 = 5;

// ---------------------------------------------------------------------------
// IpcMessage
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum IpcMessage {
    MethodCall {
        id: u64,
        interface: String,
        method: String,
        args: Vec<IpcValue>,
    },
    MethodReturn {
        id: u64,
        result: Result<IpcValue, IpcError>,
    },
    Signal {
        interface: String,
        name: String,
        args: Vec<IpcValue>,
    },
    PropertyGet {
        id: u64,
        interface: String,
        name: String,
    },
    PropertySet {
        id: u64,
        interface: String,
        name: String,
        value: IpcValue,
    },
}

// ---------------------------------------------------------------------------
// Serialization helpers (length-prefixed binary format)
// ---------------------------------------------------------------------------

/// Serialize an `IpcMessage` to a length-prefixed byte vector.
///
/// Format: [4-byte LE length][postcard-encoded payload]
pub fn encode_message(msg: &IpcMessage) -> Result<Vec<u8>, String> {
    let payload = postcard::to_allocvec(msg).map_err(|e| format!("serialization failed: {e}"))?;
    let len = payload.len();
    let len_u32 = u32::try_from(len).map_err(|_| format!("message too large: {len} bytes"))?;
    let mut buf = Vec::with_capacity(4 + len);
    buf.extend_from_slice(&len_u32.to_le_bytes());
    buf.extend_from_slice(&payload);
    Ok(buf)
}

/// Deserialize an `IpcMessage` from a length-prefixed byte slice.
///
/// Returns the message and the number of bytes consumed.
pub fn decode_message(bytes: &[u8]) -> Result<(IpcMessage, usize), String> {
    if bytes.len() < 4 {
        return Err(format!(
            "too short: expected at least 4 bytes for length prefix, got {}",
            bytes.len()
        ));
    }
    let len_bytes = &bytes[..4];
    let len = u32::from_le_bytes([len_bytes[0], len_bytes[1], len_bytes[2], len_bytes[3]]) as usize;
    if bytes.len() < 4 + len {
        return Err(format!(
            "truncated: expected {} bytes of payload, got {}",
            len,
            bytes.len() - 4
        ));
    }
    let payload = &bytes[4..4 + len];
    let msg: IpcMessage =
        postcard::from_bytes(payload).map_err(|e| format!("deserialization failed: {e}"))?;
    Ok((msg, 4 + len))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip(msg: &IpcMessage) {
        let encoded = encode_message(msg).expect("encoding should succeed");
        let (decoded, consumed) = decode_message(&encoded).expect("decoding should succeed");
        assert_eq!(*msg, decoded, "round-trip must produce identical message");
        assert_eq!(consumed, encoded.len(), "must consume exactly the encoded bytes");
    }

    #[test]
    fn test_method_call_round_trip() {
        round_trip(&IpcMessage::MethodCall {
            id: 1,
            interface: "com.turnix.Service".into(),
            method: "hello".into(),
            args: vec![
                IpcValue::String("world".into()),
                IpcValue::Int(42),
            ],
        });
    }

    #[test]
    fn test_method_return_ok_round_trip() {
        round_trip(&IpcMessage::MethodReturn {
            id: 1,
            result: Ok(IpcValue::String("done".into())),
        });
    }

    #[test]
    fn test_method_return_err_round_trip() {
        round_trip(&IpcMessage::MethodReturn {
            id: 2,
            result: Err(IpcError::new(ERROR_SERVICE_NOT_FOUND, "service not found")),
        });
    }

    #[test]
    fn test_signal_round_trip() {
        round_trip(&IpcMessage::Signal {
            interface: "com.turnix.DeviceManager".into(),
            name: "DeviceAdded".into(),
            args: vec![IpcValue::Map(vec![
                ("vendor".into(), IpcValue::Int(0x8086)),
                ("device".into(), IpcValue::Int(0x1234)),
            ])],
        });
    }

    #[test]
    fn test_property_get_round_trip() {
        round_trip(&IpcMessage::PropertyGet {
            id: 3,
            interface: "com.turnix.NetworkManager".into(),
            name: "Status".into(),
        });
    }

    #[test]
    fn test_property_set_round_trip() {
        round_trip(&IpcMessage::PropertySet {
            id: 4,
            interface: "com.turnix.NetworkManager".into(),
            name: "Hostname".into(),
            value: IpcValue::String("myhost".into()),
        });
    }

    #[test]
    fn test_all_ipc_value_variants_round_trip_in_array() {
        let values = vec![
            IpcValue::Null,
            IpcValue::Bool(true),
            IpcValue::Int(-42),
            IpcValue::Float(core::f64::consts::PI),
            IpcValue::String("test".into()),
            IpcValue::Bytes(vec![0x00, 0xFF, 0xAB]),
            IpcValue::Array(vec![IpcValue::Int(1), IpcValue::Int(2)]),
            IpcValue::Map(vec![
                ("key".into(), IpcValue::String("val".into())),
            ]),
        ];
        let msg = IpcMessage::MethodCall {
            id: 99,
            interface: "test".into(),
            method: "echo".into(),
            args: values,
        };
        round_trip(&msg);
    }

    #[test]
    fn test_decode_too_short() {
        let result = decode_message(&[0x00]);
        assert!(result.is_err(), "should reject <4 bytes");
        assert!(result.unwrap_err().contains("too short"));
    }

    #[test]
    fn test_decode_truncated() {
        // length = 1000, but no payload
        let mut buf = Vec::from(&0xE8u32.to_le_bytes()[..]);
        buf.push(0x03); // only 1 byte of payload instead of 1000
        let result = decode_message(&buf);
        assert!(result.is_err(), "should reject truncated payload");
        assert!(result.unwrap_err().contains("truncated"));
    }

    #[test]
    fn test_encode_message_too_large() {
        // postcard doesn't have a max size issue for reasonable values,
        // but we verify the encoding works for a large message
        let large_args: Vec<IpcValue> = (0..1000).map(IpcValue::Int).collect();
        let msg = IpcMessage::MethodCall {
            id: 1,
            interface: "test".into(),
            method: "bulk".into(),
            args: large_args,
        };
        let encoded = encode_message(&msg).expect("large message should encode");
        assert!(encoded.len() > 4, "large message should have payload");
        let (decoded, _) = decode_message(&encoded).expect("large message should decode");
        assert_eq!(msg, decoded);
    }

    #[test]
    fn test_ipc_value_from_conversions() {
        let v: IpcValue = "hello".into();
        assert_eq!(v, IpcValue::String("hello".into()));

        let v: IpcValue = 42i64.into();
        assert_eq!(v, IpcValue::Int(42));

        let v: IpcValue = true.into();
        assert_eq!(v, IpcValue::Bool(true));

        let v: IpcValue = vec![IpcValue::Int(1), IpcValue::Int(2)].into();
        assert_eq!(v, IpcValue::Array(vec![IpcValue::Int(1), IpcValue::Int(2)]));
    }

    #[test]
    fn test_ipc_value_accessors() {
        assert_eq!(IpcValue::String("hi".into()).as_str(), Some("hi"));
        assert_eq!(IpcValue::Int(10).as_i64(), Some(10));
        assert_eq!(IpcValue::Bool(false).as_bool(), Some(false));
        assert_eq!(IpcValue::Null.as_str(), None);
        assert_eq!(IpcValue::Null.as_i64(), None);
    }

    #[test]
    fn test_ipc_error_display() {
        let err = IpcError::new(1, "not found");
        let msg = format!("{err}");
        assert!(msg.contains("1"));
        assert!(msg.contains("not found"));
    }

    #[test]
    fn test_multiple_messages_in_stream() {
        let msgs = vec![
            IpcMessage::MethodCall {
                id: 1,
                interface: "com.foo".into(),
                method: "bar".into(),
                args: vec![],
            },
            IpcMessage::Signal {
                interface: "com.foo".into(),
                name: "event".into(),
                args: vec![IpcValue::Int(42)],
            },
            IpcMessage::MethodReturn {
                id: 1,
                result: Ok(IpcValue::Null),
            },
        ];

        let mut stream = Vec::new();
        for msg in &msgs {
            let encoded = encode_message(msg).unwrap();
            stream.extend_from_slice(&encoded);
        }

        let mut offset = 0;
        for expected in &msgs {
            let (decoded, consumed) = decode_message(&stream[offset..]).unwrap();
            assert_eq!(*expected, decoded);
            offset += consumed;
        }
        assert_eq!(offset, stream.len(), "must consume entire stream");
    }
}
