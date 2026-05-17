# Changelog

## 0.1.1 - 2026-05-17

- Fixed mode switching: fields now show only for the selected task.
- Split Base64 into separate Encode and Decode tabs.
- Simplified the copy path: "复制核心结果" copies the direct output string.
- Improved validation messages and normalized sidecar output to `{ summary, data.result }`.
- Added sidecar tests for default Hash mode, Base64 round-trip and JWT time formatting.

## 0.1.0 - 2026-05-17

- Initial release.
- Text hash: MD5, SHA-1, SHA-256, SHA-512.
- Base64 encode / decode with UTF-8 round-tripping (Chinese, emoji).
- HMAC-SHA256 with hex + base64 outputs.
- JWT header / payload decode with human-readable `iat`, `exp`, `nbf`.
