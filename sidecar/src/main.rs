//! Toolbag plugin sidecar: Hash, Base64, HMAC-SHA256 and JWT decode.
//!
//! The host sends one NDJSON request frame on stdin. The sidecar writes
//! ready/result/error frames to stdout and exits.

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

#[derive(Debug)]
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
                message: "没有收到 Toolbag 请求。".to_string(),
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
                message: format!("请求格式无效：{err}"),
            });
            return;
        }
    };

    if request.ty != "request" {
        write_frame(&Frame::Error {
            id: request.id,
            code: "E_PROTOCOL".to_string(),
            message: format!("未知请求类型：{}", request.ty),
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

fn write_frame(frame: &Frame) {
    let mut stdout = io::stdout().lock();
    if let Ok(line) = serde_json::to_string(frame) {
        let _ = stdout.write_all(line.as_bytes());
        let _ = stdout.write_all(b"\n");
        let _ = stdout.flush();
    }
}

fn handle(req: &Request) -> Result<Value, PluginError> {
    match req.command.as_str() {
        "run" => dispatch_run(&req.params),
        "hash.text" => hash_text(&req.params),
        "base64.encode" => base64_encode(&req.params),
        "base64.decode" => base64_decode(&req.params),
        "hmac.sha256" => hmac_sha256(&req.params),
        "jwt.decode" => jwt_decode(&req.params),
        other => Err(PluginError::new(
            "E_NOT_FOUND",
            format!("未知命令：{other}"),
        )),
    }
}

fn dispatch_run(params: &Value) -> Result<Value, PluginError> {
    let mode = string_field(params, "mode").unwrap_or_else(|_| "hash".to_string());
    match mode.as_str() {
        "hash" => hash_text(params),
        "base64" | "base64_encode" => base64_encode(params),
        "base64_decode" => base64_decode(params),
        "hmac" => hmac_sha256(params),
        "jwt" => jwt_decode(params),
        other => Err(PluginError::new("E_PROTOCOL", format!("未知模式：{other}"))),
    }
}

fn hash_text(params: &Value) -> Result<Value, PluginError> {
    let text = string_field_either(params, &["hash_input", "text"])?;
    let algo = string_field_either(params, &["hash_algo", "algo"])
        .unwrap_or_else(|_| "sha256".to_string())
        .to_lowercase();
    if text.is_empty() {
        return Err(PluginError::new("E_INPUT", "请输入要计算 Hash 的文本。"));
    }

    let bytes = text.as_bytes();
    let digest_hex = match algo.as_str() {
        "md5" => hex::encode(md5::Md5::digest(bytes)),
        "sha1" => hex::encode(sha1::Sha1::digest(bytes)),
        "sha256" => hex::encode(sha2::Sha256::digest(bytes)),
        "sha512" => hex::encode(sha2::Sha512::digest(bytes)),
        other => {
            return Err(PluginError::new(
                "E_INPUT",
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
            "任务": "Hash",
            "算法": algo_label,
            "输入字节": bytes.len(),
            "输出字符": digest_hex.len()
        },
        "data": {
            "result": digest_hex,
            "details": {
                "algorithm": algo,
                "inputBytes": bytes.len()
            }
        }
    }))
}

fn base64_encode(params: &Value) -> Result<Value, PluginError> {
    let text = string_field_either(params, &["b64_plain", "b64_input", "text"])?;
    if text.is_empty() {
        return Err(PluginError::new("E_INPUT", "请输入要编码的文本。"));
    }
    let encoded = B64_STD.encode(text.as_bytes());
    Ok(json!({
        "summary": {
            "任务": "Base64 编码",
            "输入字节": text.len(),
            "输出字符": encoded.len()
        },
        "data": {
            "result": encoded,
            "details": {
                "input": text
            }
        }
    }))
}

fn base64_decode(params: &Value) -> Result<Value, PluginError> {
    let text = string_field_either(params, &["b64_encoded", "b64_input", "text"])?;
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Err(PluginError::new("E_INPUT", "请输入要解码的 Base64。"));
    }
    let bytes = B64_STD
        .decode(trimmed)
        .or_else(|_| B64_URL.decode(trimmed))
        .map_err(|e| PluginError::new("E_INPUT", format!("Base64 解码失败：{e}")))?;
    let utf8_text = std::str::from_utf8(&bytes).map(str::to_string).ok();
    let bytes_hex = hex::encode(&bytes);
    let result = utf8_text.clone().unwrap_or_else(|| bytes_hex.clone());

    Ok(json!({
        "summary": {
            "任务": "Base64 解码",
            "输出字节": bytes.len(),
            "UTF-8 文本": utf8_text.is_some()
        },
        "data": {
            "result": result,
            "details": {
                "textUtf8": utf8_text,
                "bytesHex": bytes_hex
            }
        }
    }))
}

fn hmac_sha256(params: &Value) -> Result<Value, PluginError> {
    let secret = string_field_either(params, &["hmac_secret", "secret"])?;
    let payload = string_field_either(params, &["hmac_payload", "payload"])?;
    if secret.is_empty() {
        return Err(PluginError::new("E_INPUT", "请输入 HMAC 密钥。"));
    }
    if payload.is_empty() {
        return Err(PluginError::new("E_INPUT", "请输入要签名的正文。"));
    }

    let mut mac = HmacSha256::new_from_slice(secret.as_bytes())
        .map_err(|e| PluginError::new("E_INTERNAL", format!("HMAC 初始化失败：{e}")))?;
    mac.update(payload.as_bytes());
    let bytes = mac.finalize().into_bytes();
    let hex_signature = hex::encode(bytes);
    let b64_signature = B64_STD.encode(bytes);

    Ok(json!({
        "summary": {
            "任务": "HMAC-SHA256",
            "密钥长度": secret.len(),
            "正文长度": payload.len()
        },
        "data": {
            "result": hex_signature,
            "details": {
                "hex": hex_signature,
                "base64": b64_signature
            }
        }
    }))
}

fn jwt_decode(params: &Value) -> Result<Value, PluginError> {
    let token = string_field_either(params, &["jwt_token", "token"])?;
    let token = token.trim();
    if token.is_empty() {
        return Err(PluginError::new("E_INPUT", "请输入 JWT。"));
    }
    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() != 3 {
        return Err(PluginError::new(
            "E_INPUT",
            format!(
                "JWT 必须包含 header.payload.signature 三段，当前是 {} 段。",
                parts.len()
            ),
        ));
    }

    let header_bytes = B64_URL
        .decode(parts[0])
        .map_err(|e| PluginError::new("E_INPUT", format!("header 解码失败：{e}")))?;
    let payload_bytes = B64_URL
        .decode(parts[1])
        .map_err(|e| PluginError::new("E_INPUT", format!("payload 解码失败：{e}")))?;
    let header: Value = serde_json::from_slice(&header_bytes)
        .map_err(|e| PluginError::new("E_INPUT", format!("header JSON 无效：{e}")))?;
    let payload: Value = serde_json::from_slice(&payload_bytes)
        .map_err(|e| PluginError::new("E_INPUT", format!("payload JSON 无效：{e}")))?;

    let alg = header
        .get("alg")
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .to_string();
    let typ = header
        .get("typ")
        .and_then(Value::as_str)
        .unwrap_or("JWT")
        .to_string();
    let exp = payload.get("exp").and_then(Value::as_i64);
    let iat = payload.get("iat").and_then(Value::as_i64);
    let nbf = payload.get("nbf").and_then(Value::as_i64);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);

    let mut summary = serde_json::Map::new();
    summary.insert("任务".into(), json!("JWT 解析"));
    summary.insert("alg".into(), json!(alg));
    summary.insert("typ".into(), json!(typ));
    if let Some(iat) = iat {
        summary.insert("iat".into(), json!(format_unix(iat)));
    }
    if let Some(nbf) = nbf {
        summary.insert("nbf".into(), json!(format_unix(nbf)));
    }
    if let Some(exp) = exp {
        summary.insert("exp".into(), json!(format_unix(exp)));
        summary.insert("已过期".into(), json!(now > exp));
    }

    let details = json!({
        "header": header,
        "payload": payload,
        "signature": parts[2]
    });
    let result = serde_json::to_string_pretty(&details)
        .map_err(|e| PluginError::new("E_INTERNAL", format!("结果序列化失败：{e}")))?;

    Ok(json!({
        "summary": Value::Object(summary),
        "data": {
            "result": result,
            "details": details
        }
    }))
}

fn format_unix(seconds: i64) -> String {
    let days_from_epoch = seconds.div_euclid(86_400);
    let secs_in_day = seconds.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days_from_epoch);
    let hour = secs_in_day / 3600;
    let minute = (secs_in_day % 3600) / 60;
    let second = secs_in_day % 60;
    format!("{year:04}-{month:02}-{day:02} {hour:02}:{minute:02}:{second:02} UTC")
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

fn string_field(params: &Value, key: &str) -> Result<String, PluginError> {
    params
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| PluginError::new("E_INPUT", format!("缺少字段：{key}")))
}

fn string_field_either(params: &Value, keys: &[&str]) -> Result<String, PluginError> {
    for key in keys {
        if let Some(s) = params.get(*key).and_then(Value::as_str) {
            return Ok(s.to_string());
        }
    }
    Err(PluginError::new(
        "E_INPUT",
        format!("缺少字段：{}", keys.join(" 或 ")),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_run_mode_is_hash() {
        let result = dispatch_run(&json!({
            "hash_input": "hello",
            "hash_algo": "sha256"
        }))
        .expect("hash");
        assert_eq!(
            result.pointer("/data/result").and_then(Value::as_str),
            Some("2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824")
        );
    }

    #[test]
    fn supports_separate_base64_modes() {
        let encoded = dispatch_run(&json!({
            "mode": "base64_encode",
            "b64_plain": "你好"
        }))
        .expect("encode");
        assert_eq!(
            encoded.pointer("/data/result").and_then(Value::as_str),
            Some("5L2g5aW9")
        );

        let decoded = dispatch_run(&json!({
            "mode": "base64_decode",
            "b64_encoded": "5L2g5aW9"
        }))
        .expect("decode");
        assert_eq!(
            decoded.pointer("/data/result").and_then(Value::as_str),
            Some("你好")
        );
    }

    #[test]
    fn formats_unix_time() {
        assert_eq!(format_unix(1_516_239_022), "2018-01-18 01:30:22 UTC");
    }
}
