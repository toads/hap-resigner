---
name: ohos-hap-resigner
description: 傻瓜式 HAP 重签名桌面工具（hap-resigner）：拖入 hap 即自动重签名并安装到真机。跨平台（macOS/Windows）。触发词：重签名、hap 重签、分发 hap、安装 hap、hap-resigner。
---

# HAP Resigner

纯 Rust 实现的 HarmonyOS / OpenHarmony HAP 重签名与自动安装工具。接收方拖入 HAP 后自动完成：识别包名 → 检测 UDID → 登录 AGC → 申请/复用证书与 Profile → 覆盖重签 → hdc 安装并启动。

## 项目结构

```
ohos-hap-resigner/
├── src/            # egui GUI、HAP signer、AGC 客户端、材料管理、HDC 设备
├── vendor/         # 固定版本的 OpenHarmony HDC / ylong 源码
├── tests/          # 密码学向量与集成测试
├── build_mac.sh    # macOS .app + ZIP/SHA-256 打包
└── .github/workflows/windows.yml
```

## 核心能力

- 纯 Rust HAP v3 signer：APK-v2 chunk digest、PKCS#7 CMS、ECDSA P-256、fs-verity Merkle tree、native `.so` code-sign。
- 覆盖重签：替换 Profile 与外层 signer block，保留已有 property 或按需生成。
- 内嵌 HDC：vendored 官方 HDC host，自动复用系统 `8710` server 或启动自带 server。
- AGC 自动化：DevEco 公共 OAuth、设备/证书/Profile API；严格 TLS；错误脱敏；P12 密码与 token 存入系统密钥库。
- 原生 GUI：egui 单进程渲染，跟随系统明暗主题，顶栏设备状态胶囊下拉选择。

## 构建

```bash
git clone https://github.com/toads/hap-resigner.git && cd hap-resigner

# 测试
cargo test --locked --features app

# CLI 自检
cargo run --bin hap-resigner-cli -- --selftest

# macOS 打包
./build_mac.sh
```

Windows 正式构建由 `v*` 标签触发 `.github/workflows/windows.yml`。

## CLI 用法

```bash
# 重签
cargo run --bin hap-resigner-cli -- sign \
  --input app.hap --output app-resigned.hap \
  --p12 debug.p12 --certificate debug.cer --profile profile.p7b

# GUI
cargo run --bin hap-resigner --features app
```

预编译包见 <https://github.com/toads/hap-resigner/releases>。

## 许可证

MIT 或 Apache-2.0。
