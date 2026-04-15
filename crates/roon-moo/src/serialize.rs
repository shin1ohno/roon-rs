use bytes::{BufMut, BytesMut};

use crate::message::{MooBody, MooMessage};

const CONTENT_TYPE_JSON: &str = "application/json";

/// Serialize a `MooMessage` into its wire format (binary WebSocket frame content).
pub fn serialize(msg: &MooMessage) -> Vec<u8> {
    let mut buf = BytesMut::with_capacity(256);

    // First line: "MOO/1 VERB name\n"
    buf.put_slice(b"MOO/1 ");
    buf.put_slice(msg.verb.as_str().as_bytes());
    buf.put_u8(b' ');
    buf.put_slice(msg.name.as_bytes());
    buf.put_u8(b'\n');

    // Request-Id header (always required)
    buf.put_slice(b"Request-Id: ");
    buf.put_slice(msg.request_id.to_string().as_bytes());
    buf.put_u8(b'\n');

    // Serialize body to get Content-Length/Content-Type
    let body_bytes = match &msg.body {
        Some(MooBody::Json(value)) => {
            let json_bytes = serde_json::to_vec(value).expect("JSON serialization should not fail");
            if json_bytes.is_empty() {
                None
            } else {
                Some((CONTENT_TYPE_JSON.to_string(), json_bytes))
            }
        }
        Some(MooBody::Binary(data)) => {
            if data.is_empty() {
                None
            } else {
                Some(("application/octet-stream".to_string(), data.to_vec()))
            }
        }
        None => None,
    };

    if let Some((ref content_type, ref data)) = body_bytes {
        buf.put_slice(b"Content-Length: ");
        buf.put_slice(data.len().to_string().as_bytes());
        buf.put_u8(b'\n');
        buf.put_slice(b"Content-Type: ");
        buf.put_slice(content_type.as_bytes());
        buf.put_u8(b'\n');
    }

    // Extra headers
    for (key, value) in &msg.headers {
        buf.put_slice(key.as_bytes());
        buf.put_slice(b": ");
        buf.put_slice(value.as_bytes());
        buf.put_u8(b'\n');
    }

    // Blank line terminates headers
    buf.put_u8(b'\n');

    // Body
    if let Some((_, data)) = body_bytes {
        buf.put_slice(&data);
    }

    buf.to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::{MooBody, MooMessage, MooVerb};
    use crate::parse::parse;
    use bytes::Bytes;
    use serde_json::json;
    use std::collections::HashMap;

    fn msg_no_body(verb: MooVerb, name: &str, request_id: u32) -> MooMessage {
        MooMessage {
            verb,
            name: name.to_string(),
            request_id,
            headers: HashMap::new(),
            body: None,
        }
    }

    #[test]
    fn test_serialize_simple_request() {
        let msg = msg_no_body(MooVerb::Request, "com.roonlabs.ping:1/ping", 5);
        let raw = serialize(&msg);
        let text = String::from_utf8_lossy(&raw);
        assert!(text.starts_with("MOO/1 REQUEST com.roonlabs.ping:1/ping\n"));
        assert!(text.contains("Request-Id: 5\n"));
    }

    #[test]
    fn test_serialize_complete_no_body() {
        let msg = msg_no_body(MooVerb::Complete, "Success", 0);
        let raw = serialize(&msg);
        let text = String::from_utf8_lossy(&raw);
        assert!(text.starts_with("MOO/1 COMPLETE Success\n"));
        assert!(!text.contains("Content-Length"));
    }

    #[test]
    fn test_serialize_with_json_body() {
        let msg = MooMessage {
            verb: MooVerb::Request,
            name: "com.roonlabs.transport:2/seek".to_string(),
            request_id: 12,
            headers: HashMap::new(),
            body: Some(MooBody::Json(
                json!({"zone_id": "z1", "how": "absolute", "seconds": 30}),
            )),
        };
        let raw = serialize(&msg);
        let text = String::from_utf8_lossy(&raw);
        assert!(text.contains("Content-Type: application/json\n"));
        assert!(text.contains("Content-Length: "));
    }

    #[test]
    fn test_serialize_with_binary_body() {
        let data = Bytes::from_static(b"\x00\x01\x02\xff");
        let msg = MooMessage {
            verb: MooVerb::Request,
            name: "some/method".to_string(),
            request_id: 7,
            headers: HashMap::new(),
            body: Some(MooBody::Binary(data.clone())),
        };
        let raw = serialize(&msg);
        assert!(raw.ends_with(b"\x00\x01\x02\xff"));
    }

    #[test]
    fn test_serialize_with_extra_headers() {
        let mut headers = HashMap::new();
        headers.insert("Logging".to_string(), "quiet".to_string());
        let msg = MooMessage {
            verb: MooVerb::Complete,
            name: "Success".to_string(),
            request_id: 0,
            headers,
            body: None,
        };
        let raw = serialize(&msg);
        let text = String::from_utf8_lossy(&raw);
        assert!(text.contains("Logging: quiet\n"));
    }

    #[test]
    fn test_roundtrip_no_body() {
        let msg = msg_no_body(MooVerb::Request, "com.roonlabs.ping:1/ping", 42);
        let raw = serialize(&msg);
        let parsed = parse(&raw).unwrap();
        assert_eq!(parsed.verb, msg.verb);
        assert_eq!(parsed.name, msg.name);
        assert_eq!(parsed.request_id, msg.request_id);
        assert!(parsed.body.is_none());
    }

    #[test]
    fn test_roundtrip_json_body() {
        let body = json!({"key": "value", "num": 42});
        let msg = MooMessage {
            verb: MooVerb::Continue,
            name: "Changed".to_string(),
            request_id: 3,
            headers: HashMap::new(),
            body: Some(MooBody::Json(body.clone())),
        };
        let raw = serialize(&msg);
        let parsed = parse(&raw).unwrap();
        assert_eq!(parsed.verb, MooVerb::Continue);
        assert_eq!(parsed.name, "Changed");
        assert_eq!(parsed.request_id, 3);
        assert_eq!(parsed.json_body().unwrap(), &body);
    }

    #[test]
    fn test_roundtrip_binary_body() {
        let data = Bytes::from_static(b"\x00\x01\x02\x03\x04");
        let msg = MooMessage {
            verb: MooVerb::Request,
            name: "svc/method".to_string(),
            request_id: 99,
            headers: HashMap::new(),
            body: Some(MooBody::Binary(data.clone())),
        };
        let raw = serialize(&msg);
        let parsed = parse(&raw).unwrap();
        match &parsed.body {
            Some(MooBody::Binary(b)) => assert_eq!(&b[..], &data[..]),
            other => panic!("expected Binary body, got {:?}", other),
        }
    }

    #[test]
    fn test_roundtrip_complete_verbs() {
        for verb in [MooVerb::Request, MooVerb::Continue, MooVerb::Complete] {
            let name = match verb {
                MooVerb::Request => "svc/method",
                MooVerb::Continue => "Changed",
                MooVerb::Complete => "Success",
            };
            let msg = msg_no_body(verb, name, 0);
            let raw = serialize(&msg);
            let parsed = parse(&raw).unwrap();
            assert_eq!(parsed.verb, verb);
            assert_eq!(parsed.name, name);
        }
    }
}

#[cfg(test)]
mod proptests {
    use super::*;
    use crate::message::{MooBody, MooMessage, MooVerb};
    use crate::parse::parse;
    use proptest::prelude::*;
    use std::collections::HashMap;

    fn arb_verb() -> impl Strategy<Value = MooVerb> {
        prop_oneof![
            Just(MooVerb::Request),
            Just(MooVerb::Continue),
            Just(MooVerb::Complete),
        ]
    }

    proptest! {
        #[test]
        fn roundtrip_proptest(
            verb in arb_verb(),
            request_id in 0u32..10000,
        ) {
            // Generate name based on verb
            let name = match verb {
                MooVerb::Request => "com.test:1/method".to_string(),
                _ => "Status".to_string(),
            };

            let msg = MooMessage {
                verb,
                name: name.clone(),
                request_id,
                headers: HashMap::new(),
                body: None,
            };
            let raw = serialize(&msg);
            let parsed = parse(&raw).unwrap();
            prop_assert_eq!(parsed.verb, msg.verb);
            prop_assert_eq!(&parsed.name, &msg.name);
            prop_assert_eq!(parsed.request_id, msg.request_id);
        }

        #[test]
        fn roundtrip_with_json_proptest(
            request_id in 0u32..10000,
            val in -1000i64..1000,
        ) {
            let body = serde_json::json!({"value": val});
            let msg = MooMessage {
                verb: MooVerb::Continue,
                name: "Changed".to_string(),
                request_id,
                headers: HashMap::new(),
                body: Some(MooBody::Json(body.clone())),
            };
            let raw = serialize(&msg);
            let parsed = parse(&raw).unwrap();
            prop_assert_eq!(parsed.json_body().unwrap(), &body);
        }
    }
}
