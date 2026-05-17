# Hash & Base64 — Toolbag 插件

[Toolbag](https://github.com/LFenX/Toolbag-Windows) 的一个本地工具插件，提供四类常用的小功能：

- **文本哈希**：MD5 / SHA-1 / SHA-256 / SHA-512，一键计算
- **Base64**：文本编码 / 解码，正确处理中文与 Emoji
- **HMAC-SHA256**：输入 secret + payload，输出 hex 与 base64 签名（Webhook / API 调试常用）
- **JWT 解析**：解析 header 与 payload，自动展开 `iat`/`exp`/`nbf`

所有计算都在本机执行，不联网，不读写文件，不读取剪贴板之外的任何外部数据。

## 截图

打开 Toolbag → 应用市场 → 找到 "Hash & Base64" → 安装。安装后从左侧 sidebar 进入即可。

## 架构

- `tool.json`：插件元数据（id、版本、命令、风险等级、最低 Toolbag 版本）。
- `ui.json`：声明式 UI Schema —— 左侧 SchemaForm 表单，右侧 ResultRenderer 结果区。
- `sidecar/`：原生子进程（Rust），通过 stdin/stdout 的 NDJSON 协议接收单次请求、返回结果后退出。

支持的命令：

| command | 说明 |
|---|---|
| `run` | 根据 `params.mode` 分发：`hash` / `base64` / `hmac` / `jwt` |
| `hash.text` | 直接计算哈希，参数 `{ text, algo }` |
| `base64.encode` | `{ text }` → Base64 字符串 |
| `base64.decode` | `{ text }` → UTF-8 文本 + hex 字节 |
| `hmac.sha256` | `{ secret, payload }` → hex + base64 签名 |
| `jwt.decode` | `{ token }` → header + payload + signature |

所有命令都返回 `{ summary, data }` 形式的结果：`summary` 走 keyValue 渲染（人眼速读），`data` 走 code 渲染（可复制 / 可处理）。

## 从源码构建

```powershell
git clone https://github.com/LFenX/toolbag-plugin-HashAndBase64.git
cd toolbag-plugin-HashAndBase64\sidecar
cargo build --release
```

产物在 `sidecar/target/release/sidecar-windows-x64.exe`。

## 打包成 `.tbpkg`

`.tbpkg` 实质是 zip。手工打包流程：

```powershell
$plugin = "toolbag-plugin-hash-and-base64-0.1.0"
$dist = "dist\$plugin"
Remove-Item -Recurse -Force dist -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force -Path "$dist\bin" | Out-Null
Copy-Item tool.json ui.json icon.svg changelog.md "$dist\"
Copy-Item sidecar\target\release\sidecar-windows-x64.exe "$dist\bin\"
Compress-Archive -Path "$dist\*" -DestinationPath "dist\$plugin.tbpkg"
```

接着用 minisign 签名（密钥由插件作者持有，公钥被 Toolbag 客户端硬编码）：

```powershell
minisign -Sm "dist\$plugin.tbpkg" -s $env:TOOLBAG_PLUGIN_PRIVKEY -t "toolbag-plugin signature"
```

会生成同名 `.tbpkg.sig`。再生成 `.sha256`：

```powershell
(Get-FileHash "dist\$plugin.tbpkg" -Algorithm SHA256).Hash.ToLower() | `
  Out-File -NoNewline -Encoding ascii "dist\$plugin.tbpkg.sha256"
```

CI（`.github/workflows/release.yml`）会在打 tag 时自动完成全部步骤并发到 GitHub Releases。

## 发布

```powershell
git tag v0.1.0
git push origin v0.1.0
```

Release Workflow 会：

1. 在 Windows runner 上 `cargo build --release` 编译 sidecar。
2. 打包 `.tbpkg` 并生成 `.sha256`。
3. 用 GitHub Secret `TOOLBAG_PLUGIN_PRIVKEY` 做 minisign 签名。
4. 创建 GitHub Release，上传 `.tbpkg` / `.tbpkg.sig` / `.tbpkg.sha256`。
5. （可选）向 [Toolbag-Registry](https://github.com/LFenX/Toolbag-Registry) 提 PR 更新 `plugins/com.lfen.toolbag.hash-and-base64.json`。

## 协议

MIT。详见 [LICENSE](./LICENSE)。
