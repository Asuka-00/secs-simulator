# SECS Simulator

[![License](https://img.shields.io/badge/License-Apache_2.0-blue.svg)](LICENSE)

Desktop **SECS/HSMS-SS** dual-role simulator (Host / Equipment).

桌面端 **SECS/HSMS-SS 双角色模拟器**（Host / Equipment），基于 **Tauri 2 + Vue 3 + TypeScript**，协议栈通过 **独立打包** 引用 [`secs4rs`](src-tauri/vendor/secs4rs)（类似 C# 引用 NuGet/DLL，而非源码工程强绑定）。

## 协议栈依赖（secs4rs）

| 项 | 说明 |
|---|---|
| 引用方式 | 项目内 vendored crate（`src-tauri/vendor/secs4rs`） |
| 包版本 | `0.1.0`（见 `src-tauri/Cargo.toml`） |
| 包归档 | `src-tauri/vendor/secs4rs-0.1.0.crate`（`cargo package` 产物） |

升级协议栈（从 secs4rs 源码重新打包并替换 vendor）：

```bash
./scripts/vendor-secs4rs.sh /path/to/secs4rs
cd src-tauri && cargo check
```

## 核心模型：预制消息库

行为完全由 **消息目录（Message Catalog）** 驱动（无独立 Rules/GEM 面板）：

1. 新建会话默认导入仓库内 [`default.SMD`](default.SMD)；也可 **Import SMD** 替换为其它 GWGEM 风格字典
2. 左侧 **按 Pair/SxFy 分组树**（蓝/黄箭头表示 H→E / H←E，绿点 = AutoReply）
3. **右键菜单**：Edit Property / Edit Body / Send / Copy / Delete / Toggle AR / New
4. **双击** 消息节点 → 属性弹窗；双击 body 行 → Body 弹窗
5. 收到带 AR 的入站 primary 时，按 `PairName` 自动回配对 secondary

## 语言与主题 / Language & Theme

- **语言**：中文 / English（顶栏下拉，`secs-sim-locale`）
- **主题**：浅色 / 深色（顶栏下拉，`secs-sim-theme`；默认跟随系统 `prefers-color-scheme`）

Language and theme are header switchers, persisted in `localStorage`.

## 开发

```bash
pnpm install
pnpm tauri dev
```

```bash
cd src-tauri && cargo test --lib
```

## 快速联调

1. 侧栏 **Pair** 创建 Equip(Passive) + Host(Active) 于 `:5000`（已带 `default.SMD`）
2. 先 Open **Equip**，再 Open **Host**，状态变为 Selected
3. 在 Host 选中 `S1F13` / `S1F1` 等消息点 **Send**，Equip 按 AR 自动回包
4. 在消息列表勾选/取消 **AR** 即可控制是否自动回复

## 布局

```
Sessions │ Connection bar
         ├ Messages list │ Message editor (body / AR / Send)
         └ Transaction log
```

## License

[Apache License 2.0](LICENSE).
