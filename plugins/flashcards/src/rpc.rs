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
    pub url: String,
    pub key: String,
    next_id: u32,
    reply: Vec<u8>, // one reply buffer for every call of this client
}

const REPLY_BYTES: usize = 1024 * 1024; // a media.get reply holds a whole image in base64

impl Client {
    pub fn new(url: &str, key: &str) -> Client { Client { url: String::from(url), key: String::from(key), next_id: 1, reply: Vec::new() } }

    pub fn call(&mut self, method: &str, params: Value) -> Result<Value, Error> {
        let body = json!({"jsonrpc": "2.0", "id": self.next_id, "method": method, "params": params});
        self.next_id += 1;
        let body = serde_json::to_vec(&body).unwrap_or_default();
        let head = format!("POST {}\nContent-Type: application/json\nAuthorization: Bearer {}\n", self.url, self.key);
        if self.reply.len() < REPLY_BYTES {
            self.reply = store::uninit(REPLY_BYTES);
        }
        let resp = &mut self.reply;
        let n = dsn::net_request(&head, &body, resp);
        if n < 0 {
            return Err(Error::new(if n == dsn::ERR_TOO_BIG { Kind::Reply } else { Kind::Network }, method));
        }
        let status = u16::from_be_bytes([resp[0], resp[1]]);
        if status == 401 || status == 403 {
            return Err(Error::new(Kind::Unauthorized, method));
        }
        if status != 200 {
            return Err(Error::new(Kind::Http(status), method));
        }
        let reply: Value = serde_json::from_slice(&resp[4..4 + n as usize]).map_err(|_| Error::new(Kind::Reply, method))?;
        if let Some(e) = reply.get("error") {
            return Err(Error {
                kind: Kind::Rpc,
                kind_name: String::from(e.pointer("/data/type").and_then(|t| t.as_str()).unwrap_or("")),
                message: String::from(e.get("message").and_then(|m| m.as_str()).unwrap_or("")),
            });
        }
        Ok(reply.get("result").cloned().unwrap_or(Value::Null))
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
