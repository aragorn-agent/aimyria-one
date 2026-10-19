# AimyriA go

把 Claude Code、Claude Desktop、Codex、WorkBuddy、DeepSeek Harness（`dsh`）、Hermes
一键配置为使用 [Aimyria](https://aimyria.com) 中转；OpenCode、OpenClaw、Gemini CLI
作为兼容入口保留。

工具常驻系统托盘、静默运行、不弹窗。它只做一件事：**让你的 AI 编程命令行指向 Aimyria**。

---

## 安装

**免安装，单个文件，双击即用。**

1. 下载 <https://aimyria.com/releases/latest/download/AimyriaOne-x64.exe>
   （约 34.9 MB，可用同目录的 `.sha256` 校验）
2. 双击运行。程序**不弹窗**，直接进系统托盘；默认开机自启。
3. 打开 <https://aimyria.com/keys>，在要用的一把密钥右侧点「导入到CCS」。
   浏览器会通过 `ccswitch://` / `aimyria://` 唤起本工具，弹出确认框；
   **确认一次**即完成：只写入你选中的那个应用，并按该密钥所属分组写入全部可用模型。

> **Windows SmartScreen 提示？** 我们不做代码签名，所以 Windows 会提示
> 「已保护你的电脑」。点 **更多信息 → 仍要运行** 即可。
> 原因与校验方式见 [为什么不做代码签名](#为什么不做代码签名)。

**系统要求**：Windows 10 / 11 x64。**不需要安装 VC++ 运行库**——运行库已静态链接进
exe，导入表里只有 Windows 自带的 DLL。

卸载：设置 → 应用 → AimyriA go；或直接删掉 `%LOCALAPPDATA%\Aimyria` 目录
（托盘菜单里也能退出）。

---

## 它改哪些文件

| 客户端 | 文件 |
|---|---|
| Claude Code | `~/.claude/settings.json` |
| Claude Desktop | Claude Desktop 的第三方供应商配置 |
| Codex | `~/.codex/auth.json`、`~/.codex/config.toml` |
| Gemini CLI | `~/.gemini/.env` |
| OpenCode | `~/.config/opencode/opencode.json` |
| OpenClaw | `~/.openclaw/openclaw.json` |
| WorkBuddy | `~/.workbuddy/models.json` |
| DeepSeek Harness | `$DSH_HOME/profiles/<profile>/cordis.patch.yml` |

写入规则：

- **只覆盖本工具自己的字段**，你在同一文件里的其它设置（钩子、权限、模型偏好）原样保留。
- **Key 还是占位符时拒绝写入**，并报错。覆盖掉一个能用的 Key 比失败更糟。
- **原子写入**（临时文件 + rename），写到一半崩溃不会留下半个配置文件。
- **重复执行结果一致**，按多少次「导入到CCS」文件都不变。
- **先确认后写入**：深链来自网页，属于不受信任的输入，所以必须由你在确认框里点「导入」才会落盘。

---

## 自更新

程序每隔数小时检查一次
<https://aimyria.com/releases/latest/download/version.json>，若发现新版本则：

1. 下载到 exe 同目录；
2. **校验 sha256**（与清单比对，不一致立即放弃并删除临时文件——失败关闭）；
3. 校验通过才替换（`rename` 当前 exe → `.old` → 放入新的 → 重启），失败自动回滚；
4. 新版本成功启动后才删除 `.old`。

正在导入时不会打断你：存在待处理的导入请求会推迟本轮更新。

---

## 开发与构建

需要 Rust 1.95、Node.js 20+、MSVC 生成工具（见下）。

```bash
pnpm install
pnpm run build:renderer                 # 前端
cd src-tauri && cargo build --release   # 产物在 target/release/aimyria-one.exe
```

### 为什么必须同时满足 MSVC + 静态 CRT

这两件事都是为了让产物是**真正的单文件**，缺一个就会在有用户的机器上双击没反应：

| 开关 | 不加会怎样 |
|---|---|
| MSVC 工具链（而非 GNU） | GNU 只能**动态**链接 `WebView2Loader.dll`，该 DLL 不在同目录就启动失败 |
| `-C target-feature=+crt-static` | 会动态依赖 `VCRUNTIME140.dll`；没装过 VC++ 运行库的干净 Windows 上启动失败 |

本仓库提供 `scripts/build_msvc.ps1`，用 `-StaticCrt` 一次带上两者：

```powershell
& scripts\build_msvc.ps1 `
    -Vcvars 'C:\path\to\vcvars64.bat' `
    -ManifestDir "$PWD\src-tauri" `
    -TargetDir 'C:\build\aimyria' `
    -StaticCrt
```

构建脚本会在结束时打印 `RESULT=OK exit=0`，并检查产物导入表**不含**
`WebView2Loader` 与 `VCRUNTIME*` / `api-ms-win-crt-*`。

### 测试

```bash
pnpm run typecheck       # 前端类型
pnpm run test:unit       # 前端单测
pnpm run check:brand     # 品牌/合规门禁
cd src-tauri && cargo test --lib
```

---

## 深链格式（给控制台侧用）

```
aimyria://v1/import?resource=provider
  &app=claude
  &name=Aimyria
  &endpoint=https://api.aimyria.com
  &apiKey=sk-...
  &homepage=https://aimyria.com
  &models=gpt-6.1-sol,claude-sonnet-5-5
  &groupName=GPT%20Pro
  &balanceUrl=https://aimyria.com/api/v1/aimyria/balance
  &balanceToken=<短期只读令牌>
```

桌面端同时接受 `ccswitch://v1/import?...`：控制台现有的「导入到CCS」按钮发的就是它，
所以站点不用改也能唤起本工具，不会落到「需要安装 CC Switch」的兜底页。

| 参数 | 必填 | 说明 |
|---|---|---|
| `resource` | 是 | 固定 `provider`。其它值一律拒绝。 |
| `app` | 是 | `claude` / `claude-desktop` / `codex` / `gemini` / `opencode` / `openclaw` / `hermes` / `workbuddy` / `dsh` |
| `name` | 是 | 显示名 |
| `apiKey` | 是¹ | 用户的 API Key |
| `endpoint` | 是¹ | 中转地址，多个用逗号分隔，第一个为主 |
| `homepage` | 否 | 留空则从 endpoint 推断 |
| `model` | 否 | 主模型。省略时按分组自动选最新 |
| `models` | 否 | 该分组的全部模型，逗号分隔 |
| `groupName` | 否 | 分组名，显示在确认框里 |
| `balanceUrl` / `balanceToken` | 否 | 只读余额桥接（控制台现签的短期令牌） |

¹ 也可以用 `config` 参数传一个 base64 的配置文件地址，由工具自行取回解析。

**同一 relay 的重复导入会替换而非新增一行**，所以轮换 Key 只要重新发一次链接即可。

---

## 隐私

- API Key 存在你自己的机器上：`%USERPROFILE%\.aimyria\aimyria-one.db`，私有权限。
- Key 只会发往 `api.aimyria.com`——查余额、跑请求。没有第三方。
- 没有遥测、没有分析上报、没有崩溃自动上传。日志在 `%USERPROFILE%\.aimyria\logs\`。

---

## 为什么不做代码签名

签名证书年费不低，而签名只能证明「二进制来自签发者」，不能证明它安全；
把这份预算花在审计和响应上更有用。替代的信任来源：

1. **源码公开**：本仓库，MIT 许可，可自行构建。
2. **校验和**：`releases/latest/download/AimyriaOne-x64.exe.sha256` 与二进制同源发布。
3. **自更新走 sha256 校验**，且失败即放弃。

---

## 许可

本项目基于 [CC Switch](https://github.com/farion1231/cc-switch) 修改，
原作者 Jason Young 以 MIT 许可证发布。

**本项目同样以 MIT 许可证分发，并保留原始版权声明。**
完整许可文本见 [LICENSE](LICENSE)。

我们是下游使用者，不是原作者。上游项目的全部功能与本项目无关，
请勿把上游的文档、赞助商列表或功能说明当作本项目的内容。

---

## 报告安全问题

**请不要开公开 issue。** 走私密渠道：<https://aimyria.com/security>
或邮件 <security@aimyria.com>。详见 [SECURITY.md](SECURITY.md)。
