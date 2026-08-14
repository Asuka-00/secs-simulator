# secs4rs

用 Rust **惯用实现**复刻 [Secs4Net](../Secs4Net/)（C#，自 `secs4java8` 100% 移植）的 SECS/GEM 协议栈。

## 硬约束：结果完全一致（Result Parity）

| 维度 | 要求 |
|---|---|
| **协议字节 / wire** | SECS-II / SECS-I / HSMS 位模式字节级一致 |
| **对外可观察行为** | 超时 T1–T8、状态机、错误条件、分块边界、消息字段语义 |
| **测试 oracle** | `Secs4Net.Tests` **65 用例语义全绿** |
| **关键保真点** | `empty().size() == -1`、BOOLEAN TRUE = `0xFF`、float 秒→毫秒 等 |

在此前提下，内部结构、API 形状、并发原语 **允许且鼓励** 惯用 Rust（`enum`、组合、`Result`、模块划分等）。

权威源：**`../Secs4Net/`**（歧义时对照 `../secs4java8/`）。

## 状态（摘要）

| Phase | 内容 | 状态 |
|---|---|---|
| 0 | 脚手架 / 文档 / harness | ✅ |
| 1 | property 反应式核（含 compute oracle） | ✅ |
| 2 | SECS-II 全类型 wire / get / 多块 | ✅ |
| 3 | OpenClose / SecsMessage / SecsTimeout | ✅ |
| 4–5 | HSMS + SS/GS 长连接 Select/DATA/Linktest | ✅ |
| 6 | SECS-I 分块 + OnTcpIp Open/重连 | ✅ |
| 7 | GEM S1/S2/S5/S6/S7/S9/S10 + Entity + SML | ✅ |
| 8 | 65 Case 对照审计 | ✅ |
| 9 | GEM session smoke；C# / Java HSMS-SS interop | ✅ |

后置（非阻塞）：LogicalCompution 全图、communicator 抽象 trait 组合。

## 工程布局

```
secs4rs/
├── Cargo.toml
├── CONVENTIONS.md
├── PORTING_LEDGER.md       # 行为切片进度
├── BEHAVIOR_MAP.md         # 源 → Rust 等价对照
├── crates/secs4rs/         # 主库（单元/集成测试在 #[cfg(test)]）
└── interop/
    ├── csharp_hsms_passive/   # Secs4Net Passive equip（dotnet）
    └── java_hsms_passive/     # secs4java8 Passive equip（javac + Export.jar）
```

### 主库模块

| 模块 | 说明 |
|---|---|
| `property` | Boolean/Integer/Float/Double/String/Object/List/Set/Map + compute |
| `secs2` | SECS-II enum 数据项 |
| `hsms` / `hsms_ss` / `hsms_gs` | HSMS 线帧与 SS/GS 通信器 |
| `secs1` / `secs1_on_tcp_ip` | SECS-I 分块与 TCP 封装 |
| `gem` | Clock、ACK、S1–S10/S13 辅助、动态事件报告 |
| `sml` | SML 解析 |
| `util` | EntityEventAdapter / EntityMessageSender / HsmsSsEntity |

## 构建与测试

```bash
cd secs4rs
cargo build
cargo test
cargo build --release
```

Phase 9 互操作用例会在测试中：

- **C#**：`dotnet build` + `dotnet run` `interop/csharp_hsms_passive`（依赖 `../Secs4Net`）
- **Java**：`javac -cp ../secs4java8/Export.jar` + `java` `interop/java_hsms_passive`

无 `dotnet` / `javac` 时对应用例会 panic 于工具链缺失（本机 CI 需具备）。

```bash
# 仅跑互操作
cargo test --package secs4rs phase9 -- --nocapture
```

## 许可

Apache-2.0（与上游一致）。

## 文档

- [CONVENTIONS.md](./CONVENTIONS.md) — 硬/软约束
- [PORTING_LEDGER.md](./PORTING_LEDGER.md) — 进度 ledger（行为切片）
- [BEHAVIOR_MAP.md](./BEHAVIOR_MAP.md) — API / 结构差异对照
