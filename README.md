# Hash & Base64 - Toolbag 插件

一个面向日常开发调试的本地小工具，提供：

- 文本 Hash：MD5、SHA-1、SHA-256、SHA-512
- Base64 编码 / 解码：支持中文和 Emoji
- HMAC-SHA256：输出 hex 和 base64 签名
- JWT 解析：解析 header、payload、signature，并格式化 `iat`、`exp`、`nbf`

所有计算都在本机完成，不联网，不读写文件。

## 使用方式

在 Toolbag 应用市场安装后，打开 `Hash & Base64`：

1. 选择一个任务标签。
2. 粘贴文本或 token。
3. 点击 `执行`。
4. 用 `复制核心结果` 复制最常用的输出值。

## 结构

- `tool.json`：插件元数据、运行时、权限和命令定义。
- `ui.json`：Toolbag 声明式 UI。
- `sidecar/`：Rust sidecar，通过 Toolbag NDJSON 协议处理单次请求。

## 本地构建 sidecar

```powershell
cd sidecar
cargo build --release
```

产物：

```text
sidecar/target/release/sidecar-windows-x64.exe
```

## 发布

发布时推送和 `tool.json.version` 一致的 tag，例如：

```powershell
git tag v0.1.1
git push origin v0.1.1
```

Release workflow 会自动构建 `.tbpkg`、生成 SHA-256、使用 minisign 签名，并向 `LFenX/Toolbag-Registry` 开 PR。
