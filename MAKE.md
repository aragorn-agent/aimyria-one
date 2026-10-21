# AIMYRIA GO · 产品仓库

这是 AIMYRIA GO 桌面端的唯一产品目录。所有与产品有关的源码、运维配套、发布工具都在这里。

## 目录

```
aimyria-go/
├── src/                  前端（React + TypeScript）
├── src-tauri/            Rust / Tauri 外壳
├── scripts/              安装、卸载、校验脚本（quickstart.ps1 / uninstall.ps1 等）
├── src/whats-new/        应用内「更新说明」（用户可见，措辞见下方规范）
├── ops/
│   ├── compat/           兼容层插件（服务端只读接口）
│   ├── nginx/            路由与站点配置补丁脚本
│   ├── site/             产品页 /go 的源文件
│   ├── tests/            配套自测
│   ├── tools/            构建环境（MSVC / cargo 包装）
│   └── build_changelog.py 由 version.json 生成公开更新日志
└── MAKE.md               本文件
```

## 构建

```bash
source ops/tools/build-env.sh
bash ops/tools/cargo-build.sh build --release
```

## 发版前必查

1. 三处版本号一致（`package.json` / `src-tauri/tauri.conf.json` / `src-tauri/Cargo.toml`）
2. `scripts/check_version_sync.ps1` 全 PASS
3. 产物在本机冒烟启动通过，且**确认没有遗留进程**
4. 清单里的 `sha256` 与服务端实际提供的字节一致

## 更新说明的写法（重要）

面向用户的更新说明**不是开发日志**。规则：

- **只说结论，不说机制**：写「优化了连接稳定性」，不写「修复了 X 协议在 Y 场景下的超时重试」。
- **不暴露内部标识**：不写注册表键名、协议 scheme、内部服务名、第三方依赖名、文件路径。
- **不提缺陷细节**：不写「修复了某某 bug 导致某某失效」。用户不需要知道曾经坏过。
- **默认一行**：一次更新一句话。真的有多项时，最多三条，且每条都是用户能感知的结果。
- **不承诺未来**，不写「计划中」。

推荐措辞（可直接用）：

- 优化了一些功能
- 提升了连接稳定性
- 改善了使用体验
- 修复了一些已知问题

反面例子（**不要**这样写）：

- ✗ 「余额改用 API Key 直查，不再一小时后显示令牌过期」
- ✗ 「安装器装完即写浏览器深链，程序没启动过也能点一键配置」
- ✗ 「卸载一并清除 ccswitch:// 与旧版自启项」
