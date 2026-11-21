//! JSON-RPC 2.0 to matcha-anki-server (POST <server>/api/v1, or /migaku/api/v1 for the Migaku
//! bridge), with the server key as a bearer token, as matcha's AnkiClient.
use crate::{dsn, store};
use alloc::{format, string::String, vec::Vec};
use serde_json::{json, Value};

#[derive(Clone, PartialEq)]
pub enum Kind {
    Network,
    Http(u16),
    Unauthorized,
    Reply,
    Rpc,
}
#[derive(Clone)]
pub struct Error {
    pub kind: Kind,
    pub kind_name: String, // the server's error type for Rpc: "conflict", "notFound", ...
    pub message: String,
}
impl Error {
    fn new(kind: Kind, message: &str) -> Error { Error { kind, kind_name: String::new(), message: String::from(message) } }
}

pub struct Client {
    pub keep_buffer: bool, // a run of big replies: the grown buffer is kept between them
    pub url: String,
    pub key: String,
    next_id: u32,
    reply: Vec<u8>, // one reply buffer for every call of this client
}

// Replies are read into a buffer of this size; a bigger one (a media.get picture in base64) is asked
// for again into a buffer four times larger, up to REPLY_MAX, which is let go of afterwards.
const REPLY_BYTES: usize = 64 * 1024;
const REPLY_MAX: usize = 1024 * 1024;

impl Client {
    pub fn new(url: &str, key: &str) -> Client { Client { keep_buffer: false, url: String::from(url), key: String::from(key), next_id: 1, reply: Vec::new() } }

    /// Lets go of the reply buffer (it is made again by the next call).
    pub fn release(&mut self) {
        self.keep_buffer = false;
        self.reply = Vec::new();
    }

    /// A call whose reply is raw bytes (matcha-anki-server's device.pack), handed to `f` as they
    /// are in the reply buffer. A JSON reply is an error (as from a server without the method).
    pub fn call_raw<R>(&mut self, method: &str, params: Value, f: impl FnOnce(&[u8]) -> R) -> Result<R, Error> {
        let n = self.send(method, params)?;
        let body = &self.reply[4..4 + n];
        if body.first() == Some(&b'{') {
            let reply: Value = serde_json::from_slice(body).map_err(|_| Error::new(Kind::Reply, method))?;
            let e = reply.get("error").cloned().unwrap_or(Value::Null);
            return Err(Error {
                kind: Kind::Rpc,
                kind_name: String::from(e.pointer("/data/type").and_then(|t| t.as_str()).unwrap_or("")),
                message: String::from(e.get("message").and_then(|m| m.as_str()).unwrap_or("")),
            });
        }
        let r = f(body);
        if !self.keep_buffer && self.reply.len() > REPLY_BYTES {
            self.reply = Vec::new();
        }
        Ok(r)
    }

    /// Sends a request; the reply's body is in self.reply[4..4 + n].
    fn send(&mut self, method: &str, params: Value) -> Result<usize, Error> {
        let body = json!({"jsonrpc": "2.0", "id": self.next_id, "method": method, "params": params});
        self.next_id += 1;
        let body = serde_json::to_vec(&body).unwrap_or_default();
        let head = format!("POST {}\nContent-Type: application/json\nAuthorization: Bearer {}\n", self.url, self.key);
        if self.reply.len() < REPLY_BYTES {
            self.reply = Vec::new();
            self.reply = store::uninit(REPLY_BYTES);
        }
        let mut n = dsn::net_request(&head, &body, &mut self.reply);
        while n == dsn::ERR_TOO_BIG && self.reply.len() < REPLY_MAX {
            let size = self.reply.len() * 4;
            self.reply = Vec::new();
            self.reply = store::uninit(size);
            n = dsn::net_request(&head, &body, &mut self.reply);
        }
        drop(body);
        if n < 0 {
            return Err(Error::new(if n == dsn::ERR_TOO_BIG { Kind::Reply } else { Kind::Network }, method));
        }
        let resp = &self.reply;
        let status = u16::from_be_bytes([resp[0], resp[1]]);
        if status == 401 || status == 403 {
            return Err(Error::new(Kind::Unauthorized, method));
        }
        if status != 200 {
            return Err(Error::new(Kind::Http(status), method));
        }
        Ok(n as usize)
    }

    pub fn call(&mut self, method: &str, params: Value) -> Result<Value, Error> {
        let n = self.send(method, params)?;
        let resp = &self.reply;
        let reply: Value = serde_json::from_slice(&resp[4..4 + n]).map_err(|_| Error::new(Kind::Reply, method))?;
        if let Some(e) = reply.get("error") {
            return Err(Error {
                kind: Kind::Rpc,
                kind_name: String::from(e.pointer("/data/type").and_then(|t| t.as_str()).unwrap_or("")),
                message: String::from(e.get("message").and_then(|m| m.as_str()).unwrap_or("")),
            });
        }
        let result = match reply {
            Value::Object(mut m) => m.remove("result").unwrap_or(Value::Null),
            _ => Value::Null,
        };
        // A buffer grown for one big reply is let go of.
        if !self.keep_buffer && self.reply.len() > REPLY_BYTES {
            self.reply = Vec::new();
        }
        Ok(result)
    }

    /// A long operation: started, then polled with operations.get until it ends (120 s at most).
    pub fn call_operation(&mut self, method: &str, params: Value) -> Result<Value, Error> {
        let started = self.call(method, params)?;
        let id = started.get("operationId").cloned().unwrap_or(Value::Null);
        let deadline = store::now() + 120;
        loop {
            let op = self.call("operations.get", json!({"operationId": id}))?;
            match op.get("state").and_then(|s| s.as_str()).unwrap_or("") {
                "succeeded" => return Ok(op.get("result").cloned().unwrap_or(Value::Null)),
                "failed" => {
                    let e = op.get("error").cloned().unwrap_or(Value::Null);
                    return Err(Error {
                        kind: Kind::Rpc,
                        kind_name: String::from(e.pointer("/data/type").and_then(|t| t.as_str()).unwrap_or("")),
                        message: String::from(e.get("message").and_then(|m| m.as_str()).unwrap_or("")),
                    });
                }
                _ if store::now() > deadline && deadline > 120 => {
                    return Err(Error { kind: Kind::Rpc, kind_name: String::from("timeout"), message: String::from(method) })
                }
                _ => {}
            }
        }
    }
}
