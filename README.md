# Veloct

**把写作、个人主页和静态网站发布，放进一个本地优先的桌面工作台。**

Veloct 面向个人站长：在本地管理 Markdown、知识库与作品，预览真实 Astro 网站，再按需发布、部署和备份。桌面使用 Tauri 2、Rust 与 React，网站模板使用 Astro。

[下载 Windows 版](<https://github.com/VellowK/Veloct-Blog/releases/latest>) · [浏览源码](<https://github.com/VellowK/Veloct-Blog/tree/master>) · [下载最新源码 ZIP](<https://github.com/VellowK/Veloct-Blog/archive/refs/heads/master.zip>) · [提交问题](<https://github.com/VellowK/Veloct-Blog/issues>) · [GPL-3.0 许可证](<LICENSE>)

## 功能

- **本地内容管理**：文章、普通页面与知识库以 Markdown 为正文真源；SQLite 仅作可重建索引。
- **一份编辑内容**：所见即所得 / Markdown 切换，保留未保存缓冲；文章配有可收起的「本文资源」栏。
- **个人主页**：头像裁切、头图、个人介绍、联系方式及近 365 天活动热力图；GitHub 活动使用显式同步的离线快照。
- **附件与图片**：本地附件池、明确确认后入池、引用追踪及多存储目标；支持 S3 兼容存储等可配置服务。
- **作品与链接知识库**：导入独立静态作品；语雀 / 飞书知识库仅关联目录和原始链接，不复制私有正文。
- **真实网站预览**：桌面预览与正式构建共用 Astro 模板，不使用另一套近似渲染器。
- **发布与部署分离**：发布冻结构建输入；只有明确点击部署才上传。支持 EdgeOne 直接上传，1Panel 为手动导出流程。
- **备份与恢复**：本地及可配置云备份，恢复先生成计划，再确认执行；凭据保留在操作系统凭据库。
- **统一暗色后台**：暖炭灰层级、统一标题、卡片和表单，以及可查看完整详情的操作通知。

Veloct 当前以 **单用户、单站点** 为边界，不是多人协作 CMS，也不包含已经移除的 AI 建页功能。

## 下载与使用

当前稳定版：**[1.0.1](<https://github.com/VellowK/Veloct-Blog/releases/tag/v1.0.1>)**，提供 Windows x64 NSIS 安装包。

1. 在 [Release 页面](<https://github.com/VellowK/Veloct-Blog/releases/tag/v1.0.1>) 下载 `Veloct_1.0.1_x64-setup.exe` 和 `SHA256SUMS-1.0.1.txt`。
2. 用 PowerShell 校验安装包 SHA-256，并与校验文件比对：

   ```powershell
   Get-FileHash .\Veloct_1.0.1_x64-setup.exe -Algorithm SHA256
   ```

3. 安装后新建项目，或打开已有的 Veloct 项目；随后在工作台编辑内容、配置外观并预览。
4. 云存储、备份与部署是可选操作，需要自行配置对应服务。发布或保存设置不会自动部署。

**签名说明**：1.0.1 发布文件的 Authenticode 校验通过，但使用的是维护者本机自签证书，并非公共受信代码签名。其他电脑可能出现未知发布者或 SmartScreen 提示；请核验下载来源和哈希后自行判断，不必为运行软件盲目信任证书或关闭系统防护。

**验证边界**：1.0.1 发布前通过了 UI 274 项、串行 Rust 1576 项及九套生产浏览器回归（1440 / 1024 两种宽度）；Rust 另有 17 项忽略。浏览器使用隔离 fixture，不代表真实云账户、安装后 WebView 或此次安装 / 卸载均已验证。Windows 进程树取消测试有历史波动，详见 [已知问题](<ISSUE.md>)。目前没有提供 macOS / Linux 的已验证安装包。

## 源码与项目目录

**默认开发分支是 `master`**。旧 `main` 是只有初始说明和许可文件的独立历史分支，不是当前产品源码。发布版本使用 `v1.0.1` 等标签；[最新源码 ZIP](<https://github.com/VellowK/Veloct-Blog/archive/refs/heads/master.zip>) 与 Release 自动生成的标签源码用途不同，不要混用来核对二进制版本。

```powershell
git clone https://github.com/VellowK/Veloct-Blog.git
cd Veloct-Blog
# 如需查看原始 1.0.1 发布快照：git switch --detach v1.0.1
```

应用源码仓库与用户的网站项目是两个目录。新建网站项目时，Veloct 将模板复制进去；用户的文章、凭据和站点配置不应被提交到应用仓库。

| 目录 | 用途 |
|---|---|
| [src](<src/>) | React / TypeScript 桌面工作台 |
| [src-tauri](<src-tauri/>) | Tauri 薄适配层、系统凭据与桌面集成 |
| [crates](<crates/>) | 内容、附件、发布、部署、备份等 Rust 领域逻辑 |
| [site](<site/>) | Astro 网站模板，供预览和正式构建共用 |
| [scripts](<scripts/>) | 模板准备、测试、Node 获取、构建与签名脚本 |
| [tests](<tests/>) | 前端逻辑及源码约束回归 |
| [docs/spec](<docs/spec/>) | 架构、数据模型、测试矩阵与上游事实 |

## 本地开发（Windows）

### 环境

- Windows x64、Git。
- Node.js **22.12.0 或以上**和 npm。
- Rust **1.90 或以上**，使用 MSVC 工具链；安装 Visual Studio C++ Build Tools / Windows SDK。
- Microsoft Edge WebView2 Runtime；系统依赖详见 [Tauri 官方环境准备](<https://v2.tauri.app/start/prerequisites/>)。

安装开发依赖：

```powershell
npm.cmd ci
npm.cmd --prefix site ci
```

构建脚本使用仓库隔离的 Cargo home，避免继承维护机器的全局镜像设置。首次克隆时创建其本地配置（目录不入 Git；已有文件请保留）：

```powershell
if (!(Test-Path .cargo-home/config.toml)) {
    New-Item -ItemType Directory -Force .cargo-home | Out-Null
    @'
[source.crates-io]
registry = "sparse+https://index.crates.io/"

[http]
multiplexing = false

[net]
retry = 5
'@ | Set-Content -Encoding ascii .cargo-home/config.toml
}
```

[Cargo 包装脚本](<scripts/cargo.ps1>) 从 Windows 配置读取代理；必要时可通过环境变量 `VELOCT_CARGO_PROXY` 显式指定自己的代理，不要照搬其他机器的端口。

准备模板和随包运行时，再启动完整桌面工作台：

```powershell
npm.cmd run prepare:template
powershell -ExecutionPolicy Bypass -File scripts/fetch-node.ps1
powershell -ExecutionPolicy Bypass -File scripts/build-sidecar.ps1

# npm 提供 Tauri CLI；使用与 Cargo 包装脚本相同的隔离环境。
$env:CARGO_HOME = Join-Path $PWD '.cargo-home'
$env:Path = "$env:USERPROFILE\.cargo\bin;$env:Path"
if ($env:VELOCT_CARGO_PROXY) { $env:CARGO_HTTP_PROXY = $env:VELOCT_CARGO_PROXY }
npx.cmd tauri dev
```

只看桌面 UI 可运行 `npm.cmd run dev:ui`，但普通浏览器没有 Tauri IPC，不能据此验证项目读写、凭据、预览服务或云操作。网站模板依赖用户项目结构，不能把应用仓库与真实博客目录混作一个项目。

### 测试

```powershell
npm.cmd run test:ui -- --run
npm.cmd run build:ui
npm.cmd run test:template

# 跨 crate 全量回归按串行执行，以降低 Windows 进程测试相互影响。
$env:CARGO_INCREMENTAL = '0'
powershell -ExecutionPolicy Bypass -File scripts/cargo.ps1 test --workspace --no-fail-fast -- --test-threads=1
powershell -ExecutionPolicy Bypass -File scripts/cargo.ps1 clippy --workspace --all-targets

npm.cmd --prefix site run build
npm.cmd --prefix site run verify
```

生产 UI 的浏览器脚本使用本机 Playwright 与 Edge 验证环境，不是产品依赖；所需工具准备及确切验证范围见 [接手与验证说明](<NEXT_AI.md>)。不要将隔离 IPC / 云服务 fixture 的通过等同于真实账户上线。

### 打包

```powershell
# 自行构建未签名安装包，不需要维护者的证书或私钥。
powershell -ExecutionPolicy Bypass -File scripts/build-installer.ps1 -NoSign
```

安装包输出至 `target/release/bundle/nsis/`。模板、Node 运行时及 sidecar 必须先按上文准备。自行签名需自有证书，参考 [构建脚本](<scripts/build-installer.ps1>) 和 [签名检查脚本](<scripts/sign-artifacts.ps1>)；不要在仓库提交私钥、访问令牌或真实用户数据。当前签名校验不表示自行构建可以与官方包逐字节一致。

## 反馈与贡献

欢迎通过 [Issues](<https://github.com/VellowK/Veloct-Blog/issues>) 提交问题。请注明版本、操作系统、复现步骤和期望结果；日志与截图先删除 Token、密码、私人文章和账号信息。发现可能的密钥泄露时，不要直接把凭据贴进公开 Issue。

提交修改前请运行相关测试。涉及用户内容的变更应保持 Markdown 真源、恢复前确认和凭据隔离；不得为了通过测试删除数据或绕过权限。贡献者提交的原创修改按本项目的 GPL-3.0-only 条款提供；第三方内容需保留其原有许可和声明。

## 文档

- [1.0.1 发布说明](<RELEASE-1.0.1.md>)：此次版本改动及分发边界。
- [更新记录](<UPDATE.md>) / [已知问题](<ISSUE.md>)：历史证据、当前限制与待验证项。
- [架构](<docs/spec/00-architecture.md>) / [数据模型](<docs/spec/01-data-model.md>) / [测试矩阵](<docs/spec/02-test-matrix.md>)：实现约束与验证记录。
- [开发交接](<NEXT_AI.md>) / [当前产品规则](<PROCESSING.md>) / [产品契约](<handoff.md>)：维护工作入口；历史条款以明确的后续需求修订为准。

## 许可证

Copyright (C) 2026 Vellow and Veloct contributors.

Veloct 自本次开源许可整理起采用 **GNU General Public License v3.0 only（SPDX：`GPL-3.0-only`）**。完整条款见 [LICENSE](<LICENSE>)。允许使用、修改与商业用途；向他人分发程序或衍生版本时，须遵守 GPL-3.0 的许可证、对应源码和声明要求。本程序不提供任何担保。

第三方依赖保留各自许可证；本许可不改变用户自行撰写的文章、上传的图片或其他个人内容的权利归属。历史分支 / 标签保留当时的许可记录，本次不会重写已发布标签或替换 1.0.1 安装包。
