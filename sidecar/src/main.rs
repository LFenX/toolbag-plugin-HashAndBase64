//! Toolbag plugin sidecar — Hash & Base64.
//!
//! Speaks the Toolbag NDJSON sidecar protocol over stdin/stdout. The host writes a single
//! `request` frame, this binary writes back one or more frames (we only emit `ready`,
//! `result`, and `error`) and exits.

use std::io::{self, BufRead, Write};

use base64::engine::general_purpose::STANDARD as B64_STD;
use base64::engine::general_purpose::URL_SAFE_NO_PAD as B64_URL;
use base64::Engine;
use digest::Digest;
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

type HmacSha256 = Hmac<sha2::Sha256>;

#[derive(Debug, Deserialize)]
struct Request {
    #[serde(rename = "type")]
    ty: String,
    #[serde(default)]
    id: String,
    #[serde(default)]
    command: String,
    #[serde(default)]
    params: Value,
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
enum Frame {
    Ready {
        protocol: u32,
        capabilities: Vec<String>,
    },
    Result {
        id: String,
        data: Value,
    },
    Error {
        id: String,
        code: String,
        message: String,
    },
}

fn write_frame(frame: &Frame) {
    let mut stdout = io::stdout().lock();
    if let Ok(line) = serde_json::to_string(frame) {
        let _ = stdout.write_all(line.as_bytes());
        let _ = stdout.write_all(b"\n");
        let _ = stdout.flush();
    }
}

fn main() {
    write_frame(&Frame::Ready {
        protocol: 1,
        capabilities: vec![],
    });

    let stdin = io::stdin();
    let line = match stdin.lock().lines().next() {
        Some(Ok(line)) => line,
        _ => {
            write_frame(&Frame::Error {
                id: String::new(),
                code: "E_PROTOCOL".to_string(),
                message: "no request received on stdin".to_string(),
            });
            return;
        }
    };

    let request: Request = match serde_json::from_str(&line) {
        Ok(req) => req,
        Err(err) => {
            write_frame(&Frame::Error {
                id: String::new(),
                code: "E_PROTOCOL".to_string(),
                message: format!("invalid request frame: {err}"),
            });
            return;
        }
    };

    if request.ty != "request" {
        write_frame(&Frame::Error {
            id: request.id,
            code: "E_PROTOCOL".to_string(),
            message: format!("unexpected frame type: {}", request.ty),
        });
        return;
    }

    let id = request.id.clone();
    match handle(&request) {
        Ok(data) => write_frame(&Frame::Result { id, data }),
        Err(err) => write_frame(&Frame::Error {
            id,
            code: err.code,
            message: err.message,
        }),
    }
}

struct PluginError {
    code: String,
    message: String,
}

impl PluginError {
    fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.to_string(),
            message: message.into(),
        }
    }
}

fn handle(req: &Request) -> Result<Value, PluginError> {
    match req.command.as_str() {
        "run" => dispatch_run(&req.params),
        // Allow direct per-mode commands too, useful if the UI wires them
        // individually later or other front-ends embed the sidecar.
        "hash.text" => hash_text(&req.params),
        "base64.encode" => base64_encode(&req.params),
        "base64.decode" => base64_decode(&req.params),
        "hmac.sha256" => hmac_sha256(&req.params),
        "jwt.decode" => jwt_decode(&req.params),
        other => Err(PluginError::new(
            "E_NOT_FOUND",
            format!("unknown command: {other}"),
        )),
    }
}

fn dispatch_run(params: &Value) -> Result<Value, PluginError> {
    let mode = string_field(params, "mode")?;
    match mode.as_str() {
        "hash" => hash_text(params),
        "base64" => {
            let op = string_field(params, "b64_op").unwrap_or_else(|_| "encode".to_string());
            if op == "decode" {
                base64_decode(params)
            } else {
                base64_encode(params)
            }
        }
        "hmac" => hmac_sha256(params),
        "jwt" => jwt_decode(params),
        other => Err(PluginError::new(
            "E_PROTOCOL",
            format!("unknown mode: {other}"),
        )),
    }
}

// ── Hash ─────────────────────────────────────────────────────────────────────

fn hash_text(params: &Value) -> Result<Value, PluginError> {
    let text = string_field_either(params, &["hash_input", "text"]).unwrap_or_default();
    let algo = string_field_either(params, &["hash_algo", "algo"])
        .unwrap_or_else(|_| "sha256".to_string())
        .to_lowercase();

    if text.is_empty() {
        return Err(PluginError::new(
            "E_PROTOCOL",
            "请输入要计算哈希的文本",
        ));
    }

    let bytes = text.as_bytes();
    let digest_hex = match algo.as_str() {
        "md5" => hex::encode(md5::Md5::digest(bytes)),
        "sha1" => hex::encode(sha1::Sha1::digest(bytes)),
        "sha256" => hex::encode(sha2::Sha256::digest(bytes)),
        "sha512" => hex::encode(sha2::Sha512::digest(bytes)),
        other => {
            return Err(PluginError::new(
                "E_PROTOCOL",
                format!("不支持的算法：{other}"),
            ))
        }
    };

    let algo_label = match algo.as_str() {
        "md5" => "MD5",
        "sha1" => "SHA-1",
        "sha256" => "SHA-256",
        "sha512" => "SHA-512",
        _ => algo.as_str(),
    };

    Ok(json!({
        "summary": {
            "模式": "Hash",
            "算法": algo_label,
            "输入长度": bytes.len(),
            "摘要长度": digest_hex.len() / 2,
        },
        "data": {
            "algo": algo,
            "hash": digest_hex,
        }
    }))
}

// ── Base64 ───────────────────────────────────────────────────────────────────

fn base64_encode(params: &Value) -> Result<Value, PluginError> {
    let text = string_field_either(params, &["b64_input", "text"]).unwrap_or_default();
    if text.is_empty() {
        return Err(PluginError::new(
            "E_PROTOCOL",
            "请输入要编码的文本",
        ));
    }
    let encoded = B64_STD.encode(text.as_bytes());
    Ok(json!({
        "summary": {
            "模式": "Base64",
            "操作": "编码",
            "输入字节": text.len(),
            "输出字符": encoded.len(),
        },
        "data": {
            "input": text,
            "output": encoded,
        }
    }))
}

fn base64_decode(params: &Value) -> Result<Value, PluginError> {
    let text = string_field_either(params, &["b64_input", "text"]).unwrap_or_default();
    if text.is_empty() {
        return Err(PluginError::new(
            "E_PROTOCOL",
            "请输入要解码的 Base64 字符串",
        ));
    }
    let trimmed = text.trim();
    let bytes = B64_STD
        .decode(trimmed)
        .or_else(|_| B64_URL.decode(trimmed))
        .map_err(|e| {
            PluginError::new(
                "E_PROTOCOL",
                format!("Base64 解码失败：{e}"),
            )
        })?;
    let utf8_text = match std::str::from_utf8(&bytes) {
        Ok(s) => Some(s.to_string()),
        Err(_) => None,
    };
    Ok(json!({
        "summary": {
            "模式": "Base64",
            "操作": "解码",
            "字节数": bytes.len(),
            "UTF-8 文本": utf8_text.is_some(),
        },
        "data": {
            "text_utf8": utf8_text,
            "bytes_hex": hex::encode(&bytes),
        }
    }))
}

// ── HMAC-SHA256 ──────────────────────────────────────────────────────────────

fn hmac_sha256(params: &Value) -> Result<Value, PluginError> {
    let secret = string_field_either(params, &["hmac_secret", "secret"]).unwrap_or_default();
    let payload = string_field_either(params, &["hmac_payload", "payload"]).unwrap_or_default();
    if secret.is_empty() {
        return Err(PluginError::new("E_PROTOCOL", "请输入 secret"));
    }
    if payload.is_empty() {
        return Err(PluginError::new("E_PROTOCOL", "请输入 payload"));
    }

    let mut mac = HmacSha256::new_from_slice(secret.as_bytes())
        .map_err(|e| PluginError::new("E_INTERNAL", format!("HMAC 初始化失败：{e}")))?;
    mac.update(payload.as_bytes());
    let bytes = mac.finalize().into_bytes();
    let hex_signature = hex::encode(&bytes);
    let b64_signature = B64_STD.encode(&bytes);

    Ok(json!({
        "summary": {
            "模式": "HMAC-SHA256",
            "Secret 长度": secret.len(),
            "Payload 长度": payload.len(),
        },
        "data": {
            "hex": hex_signature,
            "base64": b64_signature,
        }
    }))
}

// ── JWT decode ───────────────────────────────────────────────────────────────

fn jwt_decode(params: &Value) -> Result<Value, PluginError> {
    let token = string_field_either(params, &["jwt_token", "token"]).unwrap_or_default();
    let token = token.trim();
    if token.is_empty() {
        return Err(PluginError::new("E_PROTOCOL", "请输入 JWT"));
    }
    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() != 3 {
        return Err(PluginError::new(
            "E_PROTOCOL",
            format!(
                "JWT 必须包含三段（header.payload.signature），实际 {}",
                parts.len()
            ),
        ));
    }

    let header_bytes = B64_URL.decode(parts[0]).map_err(|e| {
        PluginError::new(
            "E_PROTOCOL",
            format!("header 解码失败：{e}"),
        )
    })?;
    let payload_bytes = B64_URL.decode(parts[1]).map_err(|e| {
        PluginError::new(
            "E_PROTOCOL",
            format!("payload 解码失败：{e}"),
        )
    })?;

    let header: Value = serde_json::from_slice(&header_bytes).map_err(|e| {
        PluginError::new(
            "E_PROTOCOL",
            format!("header JSON 无效：{e}"),
        )
    })?;
    let payload: Value = serde_json::from_slice(&payload_bytes).map_err(|e| {
        PluginError::new(
            "E_PROTOCOL",
            format!("payload JSON 无效：{e}"),
        )
    })?;

    let alg = header
        .get("alg")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .to_string();
    let typ = header
        .get("typ")
        .and_then(|v| v.as_str())
        .unwrap_or("JWT")
        .to_string();

    let exp = payload.get("exp").and_then(|v| v.as_i64());
    let iat = payload.get("iat").and_then(|v| v.as_i64());
    let nbf = payload.get("nbf").and_then(|v| v.as_i64());
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let expired = exp.map(|exp| now_ms > exp);

    let mut summary = serde_json::Map::new();
    summary.insert("模式".into(), json!("JWT"));
    summary.insert("alg".into(), json!(alg));
    summary.insert("typ".into(), json!(typ));
    if let Some(exp) = exp {
        summary.insert("exp".into(), json!(format_unix(exp)));
    }
    if let Some(iat) = iat {
        summary.insert("iat".into(), json!(format_unix(iat)));
    }
    if let Some(nbf) = nbf {
        summary.insert("nbf".into(), json!(format_unix(nbf)));
    }
    if let Some(exp) = expired {
        summary.insert("已过期".into(), json!(exp));
    }

    Ok(json!({
        "summary": Value::Object(summary),
        "data": {
            "header": header,
            "payload": payload,
            "signature": parts[2],
        }
    }))
}

fn format_unix(seconds: i64) -> String {
    // Format as ISO-ish UTC without an external chrono dep. Best effort.
    let days_from_epoch = seconds.div_euclid(86_400);
    let secs_in_day = seconds.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days_from_epoch);
    let hour = secs_in_day / 3600;
    let minute = (secs_in_day % 3600) / 60;
    let second = secs_in_day % 60;
    format!(
        "{year:04}-{month:02}-{day:02} {hour:02}:{minute:02}:{second:02} UTC"
    )
}

/// Howard Hinnant's days-from-civil algorithm in reverse.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = if m <= 2 { y + 1 } else { y };
    (year, m, d)
}

// ── helpers ──────────────────────────────────────────────────────────────────

fn string_field(params: &Value, key: &str) -> Result<String, PluginError> {
    params
        .get(key)
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| {
            PluginError::new(
                "E_PROTOCOL",
                format!("缺少字段：{key}"),
            )
        })
}

fn string_field_either(params: &Value, keys: &[&str]) -> Result<String, PluginError> {
    for key in keys {
        if let Some(s) = params.get(*key).and_then(|v| v.as_str()) {
            return Ok(s.to_string());
        }
    }
    Err(PluginError::new(
        "E_PROTOCOL",
        format!("缺少字段：{}", keys.join(" 或 ")),
    ))
}
