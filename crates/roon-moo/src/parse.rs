use std::collections::HashMap;

use bytes::Bytes;

use crate::error::MooError;
use crate::message::{MooBody, MooMessage, MooVerb};

const CONTENT_TYPE_JSON: &str = "application/json";

/// Parse a raw MOO message from a WebSocket frame.
///
/// The input is the complete binary content of a single WebSocket frame.
/// Returns a parsed `MooMessage` or an error describing why parsing failed.
pub fn parse(buf: &[u8]) -> Result<MooMessage, MooError> {
    if buf.is_empty() {
        return Err(MooError::Empty);
    }

    // Find the end of the header section (blank line = two consecutive newlines).
    let header_end = find_blank_line(buf);
    let header_bytes = &buf[..header_end];
    let body_bytes = if header_end < buf.len() {
        // Skip past the blank line delimiter (\n\n)
        let body_start = skip_blank_line(buf, header_end);
        &buf[body_start..]
    } else {
        &[]
    };

    let header_str = std::str::from_utf8(header_bytes).map_err(|_| MooError::InvalidHeaderUtf8)?;
    let mut lines = header_str.split('\n');

    // Parse first line: "MOO/1 VERB name"
    let first_line = lines.next().ok_or(MooError::Empty)?;
    let (verb, name) = parse_first_line(first_line)?;

    // Parse headers
    let mut request_id: Option<u32> = None;
    let mut content_length: Option<usize> = None;
    let mut content_type: Option<String> = None;
    let mut headers = HashMap::new();

    for line in lines {
        if line.is_empty() {
            break;
        }
        let (key, value) = parse_header_line(line)?;
        match key {
            "Request-Id" => {
                request_id = Some(
                    value
                        .parse::<u32>()
                        .map_err(|_| MooError::InvalidRequestId(value.to_string()))?,
                );
            }
            "Content-Length" => {
                content_length = Some(
                    value
                        .parse::<usize>()
                        .map_err(|_| MooError::InvalidContentLength(value.to_string()))?,
                );
            }
            "Content-Type" => {
                content_type = Some(value.to_string());
            }
            _ => {
                headers.insert(key.to_string(), value.to_string());
            }
        }
    }

    let request_id = request_id.ok_or(MooError::MissingRequestId)?;

    // Validate content headers and parse body
    let body = parse_body(body_bytes, content_length, content_type.as_deref())?;

    Ok(MooMessage {
        verb,
        name,
        request_id,
        headers,
        body,
    })
}

fn parse_first_line(line: &str) -> Result<(MooVerb, String), MooError> {
    let mut parts = line.splitn(3, ' ');

    let protocol = parts.next().ok_or(MooError::Empty)?;
    if protocol != "MOO/1" {
        return Err(MooError::InvalidProtocol(protocol.to_string()));
    }

    let verb_str = parts.next().ok_or(MooError::MissingVerb)?;
    let verb = match verb_str {
        "REQUEST" => MooVerb::Request,
        "CONTINUE" => MooVerb::Continue,
        "COMPLETE" => MooVerb::Complete,
        _ => return Err(MooError::UnknownVerb(verb_str.to_string())),
    };

    let name = parts.next().ok_or(MooError::MissingName)?;
    Ok((verb, name.to_string()))
}

fn parse_header_line(line: &str) -> Result<(&str, &str), MooError> {
    let colon_pos = line
        .find(": ")
        .ok_or_else(|| MooError::MalformedHeader(line.to_string()))?;
    Ok((&line[..colon_pos], &line[colon_pos + 2..]))
}

fn parse_body(
    body_bytes: &[u8],
    content_length: Option<usize>,
    content_type: Option<&str>,
) -> Result<Option<MooBody>, MooError> {
    match (content_length, content_type) {
        (None, None) => Ok(None),
        (Some(0), _) => Ok(None),
        (Some(len), None) => {
            debug_assert!(len > 0);
            Err(MooError::ContentLengthWithoutContentType)
        }
        (Some(len), Some(ct)) => {
            if body_bytes.len() < len {
                return Err(MooError::BodyLengthMismatch {
                    expected: len,
                    actual: body_bytes.len(),
                });
            }
            let body_slice = &body_bytes[..len];
            if ct == CONTENT_TYPE_JSON {
                let value: serde_json::Value = serde_json::from_slice(body_slice)?;
                Ok(Some(MooBody::Json(value)))
            } else {
                Ok(Some(MooBody::Binary(Bytes::copy_from_slice(body_slice))))
            }
        }
        // Content-Type without Content-Length: no body
        (None, Some(_)) => Ok(None),
    }
}

/// Find the position of the blank line separator (end of headers).
/// Returns buf.len() if no blank line is found.
fn find_blank_line(buf: &[u8]) -> usize {
    for i in 0..buf.len().saturating_sub(1) {
        if buf[i] == b'\n' && buf[i + 1] == b'\n' {
            return i;
        }
    }
    buf.len()
}

/// Skip past the blank line delimiter, returning the start of the body.
fn skip_blank_line(buf: &[u8], header_end: usize) -> usize {
    // header_end points to the first \n of \n\n
    let start = header_end + 2; // skip both \n\n
    start.min(buf.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn make_msg(s: &str) -> Vec<u8> {
        s.replace("\r\n", "\n").into_bytes()
    }

    #[test]
    fn test_parse_simple_request_no_body() {
        let raw = make_msg("MOO/1 REQUEST com.roonlabs.ping:1/ping\nRequest-Id: 5\n\n");
        let msg = parse(&raw).unwrap();
        assert_eq!(msg.verb, MooVerb::Request);
        assert_eq!(msg.name, "com.roonlabs.ping:1/ping");
        assert_eq!(msg.request_id, 5);
        assert!(msg.body.is_none());
        assert_eq!(msg.service(), Some("com.roonlabs.ping:1"));
        assert_eq!(msg.method(), Some("ping"));
    }

    #[test]
    fn test_parse_complete_no_body() {
        let raw = make_msg("MOO/1 COMPLETE Success\nRequest-Id: 0\n\n");
        let msg = parse(&raw).unwrap();
        assert_eq!(msg.verb, MooVerb::Complete);
        assert_eq!(msg.name, "Success");
        assert_eq!(msg.request_id, 0);
        assert!(msg.body.is_none());
        assert!(msg.service().is_none());
        assert!(msg.method().is_none());
    }

    #[test]
    fn test_parse_continue_with_json_body() {
        let body = r#"{"core_id":"abc-123","display_name":"My Core"}"#;
        let raw = make_msg(&format!(
            "MOO/1 CONTINUE Registered\nRequest-Id: 1\nContent-Length: {}\nContent-Type: application/json\n\n{}",
            body.len(),
            body
        ));
        let msg = parse(&raw).unwrap();
        assert_eq!(msg.verb, MooVerb::Continue);
        assert_eq!(msg.name, "Registered");
        assert_eq!(msg.request_id, 1);
        let json = msg.json_body().unwrap();
        assert_eq!(json["core_id"], json!("abc-123"));
        assert_eq!(json["display_name"], json!("My Core"));
    }

    #[test]
    fn test_parse_request_with_json_body() {
        let body = r#"{"zone_id":"z1","how":"absolute","seconds":30}"#;
        let raw = make_msg(&format!(
            "MOO/1 REQUEST com.roonlabs.transport:2/seek\nRequest-Id: 12\nContent-Length: {}\nContent-Type: application/json\n\n{}",
            body.len(),
            body
        ));
        let msg = parse(&raw).unwrap();
        assert_eq!(msg.verb, MooVerb::Request);
        assert_eq!(msg.name, "com.roonlabs.transport:2/seek");
        assert_eq!(msg.request_id, 12);
        assert_eq!(msg.service(), Some("com.roonlabs.transport:2"));
        assert_eq!(msg.method(), Some("seek"));
        let json = msg.json_body().unwrap();
        assert_eq!(json["seconds"], json!(30));
    }

    #[test]
    fn test_parse_binary_body() {
        let body_data = b"\x00\x01\x02\x03\xff";
        let header = format!(
            "MOO/1 REQUEST some/method\nRequest-Id: 7\nContent-Length: {}\nContent-Type: application/octet-stream\n\n",
            body_data.len()
        );
        let mut raw = header.into_bytes();
        raw.extend_from_slice(body_data);

        let msg = parse(&raw).unwrap();
        assert_eq!(msg.verb, MooVerb::Request);
        match &msg.body {
            Some(MooBody::Binary(b)) => assert_eq!(&b[..], body_data),
            other => panic!("expected Binary body, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_extra_headers_preserved() {
        let raw = make_msg(
            "MOO/1 COMPLETE Success\nRequest-Id: 0\nLogging: quiet\nX-Custom: value\n\n",
        );
        let msg = parse(&raw).unwrap();
        assert_eq!(msg.headers.get("Logging").unwrap(), "quiet");
        assert_eq!(msg.headers.get("X-Custom").unwrap(), "value");
    }

    #[test]
    fn test_error_empty() {
        assert!(matches!(parse(b""), Err(MooError::Empty)));
    }

    #[test]
    fn test_error_invalid_protocol() {
        let raw = make_msg("HTTP/1.1 GET /\nRequest-Id: 0\n\n");
        assert!(matches!(parse(&raw), Err(MooError::InvalidProtocol(_))));
    }

    #[test]
    fn test_error_unknown_verb() {
        let raw = make_msg("MOO/1 DELETE something\nRequest-Id: 0\n\n");
        assert!(matches!(parse(&raw), Err(MooError::UnknownVerb(_))));
    }

    #[test]
    fn test_error_missing_verb() {
        let raw = make_msg("MOO/1\nRequest-Id: 0\n\n");
        assert!(matches!(parse(&raw), Err(MooError::MissingVerb)));
    }

    #[test]
    fn test_error_missing_name() {
        let raw = make_msg("MOO/1 REQUEST\nRequest-Id: 0\n\n");
        assert!(matches!(parse(&raw), Err(MooError::MissingName)));
    }

    #[test]
    fn test_error_missing_request_id() {
        let raw = make_msg("MOO/1 REQUEST some/method\n\n");
        assert!(matches!(parse(&raw), Err(MooError::MissingRequestId)));
    }

    #[test]
    fn test_error_invalid_request_id() {
        let raw = make_msg("MOO/1 REQUEST some/method\nRequest-Id: abc\n\n");
        assert!(matches!(parse(&raw), Err(MooError::InvalidRequestId(_))));
    }

    #[test]
    fn test_error_content_length_without_content_type() {
        let raw = make_msg("MOO/1 REQUEST some/method\nRequest-Id: 0\nContent-Length: 5\n\nhello");
        assert!(matches!(
            parse(&raw),
            Err(MooError::ContentLengthWithoutContentType)
        ));
    }

    #[test]
    fn test_error_body_length_mismatch() {
        let raw = make_msg(
            "MOO/1 REQUEST some/method\nRequest-Id: 0\nContent-Length: 100\nContent-Type: application/json\n\n{}",
        );
        assert!(matches!(
            parse(&raw),
            Err(MooError::BodyLengthMismatch { .. })
        ));
    }

    #[test]
    fn test_error_invalid_json() {
        let body = "not json at all";
        let raw = make_msg(&format!(
            "MOO/1 REQUEST some/method\nRequest-Id: 0\nContent-Length: {}\nContent-Type: application/json\n\n{}",
            body.len(),
            body
        ));
        assert!(matches!(parse(&raw), Err(MooError::InvalidJson(_))));
    }

    #[test]
    fn test_parse_content_length_zero() {
        let raw =
            make_msg("MOO/1 COMPLETE Success\nRequest-Id: 3\nContent-Length: 0\n\n");
        let msg = parse(&raw).unwrap();
        assert!(msg.body.is_none());
    }

    #[test]
    fn test_parse_subscription_request() {
        let body = r#"{"subscription_key":0}"#;
        let raw = make_msg(&format!(
            "MOO/1 REQUEST com.roonlabs.transport:2/subscribe_zones\nRequest-Id: 3\nContent-Length: {}\nContent-Type: application/json\n\n{}",
            body.len(),
            body
        ));
        let msg = parse(&raw).unwrap();
        assert_eq!(
            msg.service(),
            Some("com.roonlabs.transport:2")
        );
        assert_eq!(msg.method(), Some("subscribe_zones"));
        assert_eq!(msg.json_body().unwrap()["subscription_key"], json!(0));
    }

    #[test]
    fn test_parse_registry_info_response() {
        let body = r#"{"core_id":"xxxx","display_name":"My Core","display_version":"2.0"}"#;
        let raw = make_msg(&format!(
            "MOO/1 COMPLETE Success\nRequest-Id: 0\nContent-Length: {}\nContent-Type: application/json\n\n{}",
            body.len(),
            body
        ));
        let msg = parse(&raw).unwrap();
        assert_eq!(msg.verb, MooVerb::Complete);
        assert_eq!(msg.name, "Success");
        assert_eq!(msg.json_body().unwrap()["core_id"], json!("xxxx"));
    }

    #[test]
    fn test_parse_no_trailing_blank_line() {
        // Message without the trailing \n\n — header section runs to end of buffer
        let raw = b"MOO/1 COMPLETE Success\nRequest-Id: 0";
        let msg = parse(raw).unwrap();
        assert_eq!(msg.verb, MooVerb::Complete);
        assert_eq!(msg.request_id, 0);
        assert!(msg.body.is_none());
    }
}
