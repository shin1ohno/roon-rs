use std::collections::HashMap;

use bytes::Bytes;

/// MOO protocol verb indicating the message role in a request/response exchange.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MooVerb {
    /// Initiates a new request. Name is `<service>/<method>`.
    Request,
    /// Intermediate response — the request remains open.
    Continue,
    /// Final response — the request is closed.
    Complete,
}

impl MooVerb {
    pub fn as_str(&self) -> &'static str {
        match self {
            MooVerb::Request => "REQUEST",
            MooVerb::Continue => "CONTINUE",
            MooVerb::Complete => "COMPLETE",
        }
    }
}

/// The body of a MOO message, either parsed JSON or raw binary.
#[derive(Debug, Clone, PartialEq)]
pub enum MooBody {
    Json(serde_json::Value),
    Binary(Bytes),
}

/// A parsed MOO protocol message.
#[derive(Debug, Clone)]
pub struct MooMessage {
    pub verb: MooVerb,
    /// For REQUEST: `<service>/<method>`. For CONTINUE/COMPLETE: `<status>`.
    pub name: String,
    pub request_id: u32,
    /// Headers other than Request-Id, Content-Length, and Content-Type.
    pub headers: HashMap<String, String>,
    pub body: Option<MooBody>,
}

impl MooMessage {
    /// For REQUEST messages, extract the service name (everything before the last `/`).
    pub fn service(&self) -> Option<&str> {
        if self.verb != MooVerb::Request {
            return None;
        }
        self.name.rfind('/').map(|i| &self.name[..i])
    }

    /// For REQUEST messages, extract the method name (everything after the last `/`).
    pub fn method(&self) -> Option<&str> {
        if self.verb != MooVerb::Request {
            return None;
        }
        self.name.rfind('/').map(|i| &self.name[i + 1..])
    }

    /// Extract body as JSON value, if the body is JSON.
    pub fn json_body(&self) -> Option<&serde_json::Value> {
        match &self.body {
            Some(MooBody::Json(v)) => Some(v),
            _ => None,
        }
    }
}
