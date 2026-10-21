# AIMYRIA GO

把主流 AI 编程客户端一键接入 [Aimyria](https://aimyria.com)。

常驻系统托盘、静默运行、不弹窗。你只需要做一件事：点一下「一键配置」。

---

## 安装

**免安装，单个文件，双击即用。**

1. 下载 <https://aimyria.com/go/>
2. 双击运行。程序**不弹窗**，直接进系统托盘；默认开机自启。
3. 回到控制台的密钥页，点「一键配置」，选择要配置的软件，确认一次即可。

> **Windows 提示「已保护你的电脑」？** 我们没有购买商业代码签名证书。
> 点 **更多信息 → 仍要运行** 即可。

**系统要求**：Windows 10 / 11 x64。不需要安装运行库。

卸载：设置 → 应用 → AIMYRIA GO。

---

## 支持的客户端

Claude Code · Claude Desktop · Codex · Gemini CLI · WorkBuddy ·
DeepSeek Harness · OpenCode · OpenClaw · Hermes

写入原则：

- **只覆盖本工具自己的配置**，你原有的模型、注释和其它设置原样保留。
- **先确认后写入**：配置内容是外部输入，必须在确认框里点过才会落盘。
- **原子写入**：写到一半中断不会留下半个配置文件。
- **重复执行结果一致**：按多少次「一键配置」，文件都不变。

---

## 更新

程序会定期检查新版本，发现后静默下载、校验、替换，然后重启到新版本。
不弹窗、不打断正在进行的操作。也可以在 <https://aimyria.com/go/> 手动下载覆盖。

---

## 隐私

- API Key 只保存在你自己的机器上。
- 没有遥测、没有分析上报、没有崩溃自动上传。
- 唯一的联网行为是检查更新，以及你主动查看余额时向 Aimyria 查询。

---

## 开发与构建

```bash
pnpm install
pnpm build             # 前端
cargo build --release  # 桌面端（需 MSVC 工具链）
```

Windows 构建要求 MSVC + 静态 CRT，否则目标机器需要额外安装运行库。

### 测试

```bash
cargo test
pnpm typecheck
```

---

## 许可

本项目以 MIT 许可证分发，包含来自上游开源项目的代码，
其版权声明依许可证要求予以保留。完整文本见 [LICENSE](LICENSE)。

---

## 报告安全问题

**请不要开公开 issue。** 详见 [SECURITY.md](SECURITY.md)。
