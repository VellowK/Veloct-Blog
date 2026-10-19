# 下一位 AI：从这里接手

> 最新轮次：2026-10-19，用户授权公开源码并选择 GPL-3.0。当前源码分支为 `master`；旧默认 `main` 只有初始说明/AGPL 许可，保留但不作为产品入口。完善面向用户与贡献者的 [README](<README.md>)、[GPL-3.0-only 许可](<LICENSE>) 及 manifest 元数据，公开前审查全部远端分支与标签历史。本轮不改产品逻辑、不替换已发布二进制或标签。1.0.1 的 UI274/22files、九套浏览器及 Rust1576/0failed/17ignored 等为前次发布证据；未安装、未写用户博客或部署云站点。

**前次产品轮次为 1.0.1 发版，包含已完成的 UI 重构与此前修复；本次仅源码公开与许可整理。** 后台标题、卡片、表单、空态、设置导航和滚动层级统一；业务确认、单栏编辑器与真实站点预览保持原语义。桌面/Tauri/Rust/npm根版本同步，第三方依赖保持不变。最新产物信息见第6节，旧轮次数据仅作历史。

**下一个 AI 的第一步**：读本文件与[当前产品要求](<PROCESSING.md>)，检查工作树；向用户确认下一项具体任务。不要继续旧任务、自动修 ISSUE、上传、安装、提交或发布。

## 1. 现在做到哪里

- **最新后台统一**：作品、备份、部署、发布、日志、网络图片、菜单、站点设置与外链知识库统一共享 PageHeader / Card / StateBlock；设置内嵌标题为h2，统一暖暗卡片/12px圆角/40px控件/间距和分组导航。去掉普通面板嵌套滚动，保留编辑器/预览/站点信息双栏滚动边界。仪表盘同一层级表面；没有改网站前台或领域业务。新增 [全后台生产浏览器检查](<scripts/verify-admin-dark.mjs>)，覆盖12个导航页和4个设置分区；连同既有8套回归两宽度通过，UI271/21files通过。
- 最新首页图片：头像512px裁切与头图均先入附件池，再上传一个首选云目标；已配置同步时用第一个目标，不可用不回退，未配置时用可见云目标最高上传优先级/保存顺序。S3/R2无有效公开域名时入池后拒绝网络写入。失败不改旧地址，成功只填表单仍须保存，保留上传中编辑的作者字段并忽略导航晚到结果；站点图片引用随保存/重开重建，换图不删文章引用/远程对象，不迁移既有本地文件。入口为[site_images](<crates/veloct-app/src/site_images.rs>)、[桌面图片命令](<src-tauri/src/site_images.rs>)、[设置面板](<src/SiteSettingsPanel.tsx>)和[浏览器回归](<scripts/verify-site-images.mjs>)。代码/回归及签名新包完成，相关job已收集，不重复已完成工作。
- 前轮五项：附件目标从已配置策略选择；文件点击/原生与浏览器拖放/悬停或聚焦后粘贴，先预览、确认后入池，不自动上传/改正文；右上分类通知五秒进度并可查看完整详情，不清空结构状态；面包屑返回前页保留编辑缓冲；AI建页导航、设置、调用和桌面命令移除，普通页面不受影响。一次20文件、单个64MiB；只内联<=8MiB按文件头识别的位图，其他只显示信息，不放开file://。回归与签名交付均已完成；构建/后验job均已收集，不重复已通过工作。
- 前轮功能是 S3/R2 Halo 风格策略：端点/CDN 分离协议与主机、Path/Virtual风格、目录模板/命名/同名防覆盖、扩展名首匹配后缀、预览缩略图、上传隐藏/优先级和显式只读验证。新策略默认UUID+随机冲突后缀；旧策略缺省不迁移，普通编辑仍保留旧命名/去重。验证只读ListObjectsV2，不保存或上传，不代表PUT/CDN权限。Secret仍只OS凭据库；未进行真实桶联调或用户博客写入。当前编码与回归完成，最终签名包信息见第6节。
- 前轮 EdgeOne 国内/国际程序直传保留：**仅用户点击部署才上传**，全部启用目标及主站/作品站完整发布产物，两个根使用独立项目；发布/构建/保存不上传，Pages Token只存OS凭据库，等云端Success。1Panel仍手动；未获授权真实云端部署，不自动索取聊天密钥或扩展发布。
- 首页是个人资料布局：头图、头像/名字、站点描述、独立个人介绍；原全站右栏不在首页挂载，其他页保留。不要恢复旧 Hero/文章列表、临时空首页或双图方案。
- 活动区只显示**一张近 365 天热力图**，标题旁的紧凑按钮切换博客/GitHub；没有底部按钮行。只开启一种来源时没有按钮；全关闭显示空态。不要改回 90 天或两张图。
- 联系方式仅显示有效已填项目，最多 16 项、每行最多 4 项；全空时整区移除。旧配置数组不删减，ID-only 按钮可复制。
- 写作保留**一份所见即所得 / MD 源码切换**，文章右侧有可收起的 240–300px「本文资源」小栏（附件池 / 图片池），收起即恢复全宽；没有重复右侧预览或分隔条，普通页面/知识库不挂此栏。新加文件按文章稳定 ID namespace 归属，旧资源按真实正文引用归属；添加和上传不自动改正文，显式插入走光标/选区与原保存逻辑。独立「页面预览」和外观预览才运行真实 Astro。
- 前面已完成：页面菜单选择器、工作区附件管理/设置拆分（历史AI内联已被新删除要求取代）、多主机按优先级同步且后续地址仅备份、网络图片归入设置、原位删除确认、语雀/飞书仅目录链接、新建私有草稿预览、Windows 大小窗口图标、Astro 工具栏关闭。不要因历史文档或保留的旧组件又恢复旧 UI。

## 2. 先读什么

1. 本文件：交接断点与安全边界。
2. [PROCESSING.md](<PROCESSING.md>)：当前产品要求、接线与构建注意事项。
3. [UPDATE.md](<UPDATE.md>)：最新修改、失败记录、最终签名产物。
4. [ISSUE.md](<ISSUE.md>)：当前验证缺口和仍需复核的历史问题。
5. [handoff.md](<handoff.md>) 与 [测试矩阵](<docs/spec/02-test-matrix.md>)：正式契约及证据；最新首页图片见 D246–D250，五项界面 D239–D245，S3 D233–D238，EdgeOne D228–D232，资源栏 D224–D227。旧主页位置变更见 D220–D221。

权威顺序：**用户最新明确要求 > 锁定契约 > 规格 > 交接状态**。有明确新需求时按新需求处理，不擅改锁定契约。测试矩阵的旧双图、90 天、底部按钮、右侧写作预览条目已有后续覆盖，不是当前目标。

## 3. 工作树和用户数据

- 应用仓库：`D:\Veloct-Blog`；分支 `master`，发布标签 `v1.0.1`。重构前完整快照 `3c51661` / `backup/admin-dark-before-20261018` 保留；发布包含后台统一与版本变更，不重写备份/发布标签。用户已授权公开仓库，当前默认源码分支为 `master`，GPL-3.0-only 许可见 [LICENSE](<LICENSE>)；凭据文件忽略，不进入提交、日志或发布资产。
- 接手先运行 `git status --short` / `git diff --stat`。禁止 reset/clean/无关回滚，不要 `git add -A`，不要清理未跟踪的新组件、基线和验证脚本。要提交/推送时先得到用户指示并逐路径核对。
- 应用源码与用户自己的博客项目是两个目录；真实项目绝不是仓库根目录。其本机位置见 [工作树与真实用户项目](<PROCESSING.md#2-工作树与真实用户项目>)；不要把机器私人目录固化进产品逻辑或复制用户内容进模板。
- **2026-10-16历史证据**：用户实际项目的三份匹配官方模板曾备份升级，实际构建和双尺寸浏览器验证通过，8个内容/配置/资源文件SHA-256不变；当时核验实际项目、staged模板与源码一致。首页图片轮和本次文档整理均未重查真实项目，不能写成当前文件重新核验。
- 旧项目只按**完整官方正文 / 完整规范化哈希**识别，备份后升级；自定义文件保留并记日志。不能按片段猜测“官方”，不能强制覆盖未知用户模板。若用户说“怎么没变”，先查模板升级日志和实际文件，不能只说缓存。

## 4. 修改相关功能时从哪些文件追

| 功能 | 主要入口 |
|---|---|
| 个人首页、按钮、日历与联系区 | [ProfileHome.astro](<site/src/components/ProfileHome.astro>)、[ActivityHeatmap.astro](<site/src/components/ActivityHeatmap.astro>)、[profile-home.css](<site/src/styles/profile-home.css>)、[profile-home.mjs](<site/src/lib/profile-home.mjs>) |
| 外观/首页设置及快照 | [SiteSettingsPanel.tsx](<src/SiteSettingsPanel.tsx>)、[site_settings.rs](<crates/veloct-app/src/site_settings.rs>)、[github_activity.rs](<crates/veloct-app/src/github_activity.rs>) |
| 头像/头图附件化与引用保护 | [界面](<src/SiteSettingsPanel.tsx>) → [桌面图片命令](<src-tauri/src/site_images.rs>) / [保存接线](<src-tauri/src/commands.rs>) → [首选与上传](<crates/veloct-app/src/site_images.rs>) / [附件服务](<crates/veloct-app/src/attachments.rs>)；[离线测试](<crates/veloct-app/src/site_images/tests.rs>) / [浏览器](<scripts/verify-site-images.mjs>) |
| 真项目模板升级 | [project.rs](<crates/veloct-app/src/project.rs>)、[完整历史基线目录](<crates/veloct-app/template-baselines/>)、[prepare-template.mjs](<scripts/prepare-template.mjs>) |
| 编辑与本文资源栏 / 预览入口 | [App.tsx](<src/App.tsx>)、[Editor.tsx](<src/Editor.tsx>)、[DocumentResourcePanel.tsx](<src/DocumentResourcePanel.tsx>)、[documentResources.ts](<src/lib/documentResources.ts>)、[PreviewPanel.tsx](<src/PreviewPanel.tsx>) |
| 文件选择/通知/上一页 | [AttachmentFilePicker.tsx](<src/AttachmentFilePicker.tsx>) / [Notifications.tsx](<src/components/Notifications.tsx>) / [navigationHistory.ts](<src/lib/navigationHistory.ts>)；[五项浏览器回归](<scripts/verify-five-ui.mjs>)；原生预览/字节导入在[附件命令](<src-tauri/src/attachments.rs>) |
| S3/R2 策略 | [S3ConfigFields.tsx](<src/S3ConfigFields.tsx>) / [s3Form.ts](<src/lib/s3Form.ts>) → [附件命令](<src-tauri/src/attachments.rs>) → [s3_policy.rs](<crates/veloct-attach/src/s3_policy.rs>) / [S3 adapter](<crates/veloct-attach/src/s3.rs>) / [事务](<crates/veloct-attach/src/transaction.rs>)；共享 [S3 client](<crates/veloct-s3/src/client.rs>) 只新增显式条件PUT，备份旧路径不变 |
| EdgeOne 显式直传 | [DeployPanel.tsx](<src/DeployPanel.tsx>)、[deployForm.ts](<src/lib/deployForm.ts>) → [部署命令](<src-tauri/src/deploy.rs>) → [app 部署](<crates/veloct-app/src/deploy.rs>) / [EdgeOne adapter](<crates/veloct-deploy/src/edgeone.rs>)；官方协议来源见 [upstream](<docs/spec/03-upstream-facts.md#edgeone-direct-upload-protocol-verified-2026-10-17>) |
| 接线核验 | [api.ts](<src/lib/api.ts>) → [Tauri 命令](<src-tauri/src/commands.rs>) / [注册入口](<src-tauri/src/lib.rs>) → app/domain → 真相源 |

**容易回归的细节：**

- 不活动的日历/统计用 `aria-hidden` + `inert` 和不可见 grid 叠放保留最大几何尺寸。不要改回 `[hidden]` 占位：Tailwind 的重要隐藏规则会将其折叠，导致窄屏切换时卡片跳高。不要全局覆盖正常隐藏规则。
- 公开博客日历排除私有草稿与普通页面；GitHub 是**活跃度等级**而非提交数。GitHub 显式同步到 `data/github-activity.json`，失败保留旧结果，换账号不复用；生产按冻结时间构建，不在静态页面偷偷联网。HTTP 500 已定位为 reqwest 缺少 `system-proxy`，workspace 现显式启用；不硬编码本机代理，不修改系统设置、不放松 TLS。新增可选真实联网测试，见测试矩阵 D222–D223。
- 多主机附件选择初次成功的优先地址后，不因重试、排序或可用性探测自动改 Markdown；后续地址只作备份。密钥仅 OS 凭据库。S3上传隐藏/优先级只影响下拉，不停止已配置同步。新策略UUID/日期/随机使用持久附件标识与添加时UTC保证重试稳定；409/412走有上限的条件PUT更名或报错，不覆盖。不要把只读验证说成真实PUT/CDN可用，也不要让缩略图参数进入正文链接。
- 保留但不挂载的 [MarkdownPreview.tsx](<src/MarkdownPreview.tsx>)、[DocumentPreview.tsx](<src/DocumentPreview.tsx>) 不表示当前编辑器仍有第二栏，不应直接重新接入。

## 5. 已验证与未验证

本轮（2026-10-18）与保留历史：

| 范围 | 真实记录 |
|---|---|
| UI（1.0.1发布复核） | 274 tests / 22files，TS/Vite218modules通过；新版本/依赖锁完整性测试3项，保留既有大包警告 |
| 真实浏览器（本轮） | 全后台、五项界面、首页图片、本文资源、编辑器、站点设置、菜单选择器、S3、EdgeOne共九套均1440/1024通过，无JS错误；IPC/云端/文件为隔离fixture，不是云账户或装机联调 |
| Rust（1.0.1发布复核） | 关闭增量缓存完整串行workspace1576 passed / 0 failed / 17 ignored / 47targets；clippy退出0，仅既有site-settings警告。进程树和安装取消本次均通过；前轮1575/1failed及单项失败保留为历史，未改终止代码，不宣称不稳定性已根治 |
| 打包（1.0.1发布复核） | 修正模板锁文件后重新prepare并重打，最终installer退出0/release1分40秒；独立EXE补签后两项Valid，版本均1.0.1，sidecar与fresh build一致；无安装/卸载验收 |
| GitHub 真实网络（前一轮） | 同一公开账号的生产 Rust 客户端修复前HTTP500、修复后371天；显式运行ignored测试通过，不写用户项目，不是新安装包UI装机证明 |
| 模板网站（1.0.1发布复核） | prepare/test:template通过，Astro21页构建、64/64产物检查；仅模板版本元数据同步，不改网站前台视觉，未构建用户真实项目 |
| 真实浏览器（10-16历史） | 首页 Edge 1440×900 / 1024×640 / 390×844；设置两种桌面尺寸；实际用户项目 8 页构建及两尺寸检查；本轮不把这些计作新视觉验收 |

边界：本轮文件/通知/导航的隔离浏览器fixture及前轮S3/R2、EdgeOne的官方协议核实与隔离 IPC/HTTP fixture 不等于真实云账号联调；GitHub公开日历真实网络证据属于前轮，语雀/飞书、云存储和 EdgeOne 仍不能宣称真实账户联调。本轮发布应用安装包，不代表安装/卸载或博客部署验收；没有真实用户项目/云账户操作。保留 Vite 大包、既有 clippy 字段默认赋值和实际空知识库提示。

历史失败不能抹掉：10-16主页轮曾修复隐藏规则导致的窄屏跳高；同轮一条追加无障碍检查在三尺寸后达到120秒上限，不能算其剩余构建通过。同轮打包曾被用户中断并bundle-only续做。这些不是首页图片轮的新失败或当前待办，详细过程保留在[更新记录](<UPDATE.md>)及[归档](<docs/archive/PROCESSING-2026-10-18-before-handover.md>)。

## 6. 最终交付物（1.0.1）

**本表为2026-10-18的1.0.1最终签名产物，旧1.0.0测试包哈希仅保留在更新历史。** 发布入口：[Veloct 1.0.1](<https://github.com/VellowK/Veloct-Blog/releases/tag/v1.0.1>)；用户已授权公开仓库与下载，默认源码分支为 `master`；本次公开不替换此表安装器。

| 文件 | 字节 | SHA-256 |
|---|---:|---|
| [本地测试程序](<target/release/veloct-desktop.exe>) | 17,373,112 | `440884AEB3DE93A1A08FA68E754D08584CFA343527B613906C7AFD99053B20D0` |
| [NSIS 安装包](<target/release/bundle/nsis/Veloct_1.0.1_x64-setup.exe>) | 29,905,448 | `DE5AB40E2914015F67B29D6DD800ED374E025425E5C66DEA8460E62AA9B3B8B8` |

随包提供[SHA-256校验文件](<target/release/SHA256SUMS-1.0.1.txt>)；仅发布安装包与校验文件，不将依赖资源的独立EXE当便携版发布。最终构建退出0（release1分40秒），EXE18:43:30、安装器18:42:57（+08:00），版本资源均1.0.1。首检独立EXE为NotSigned，补签后 `sign-artifacts.ps1 -Installer -Check` 均Valid；`build-sidecar.ps1 -Check` 确认源sidecar与fresh build一致。证书 `117C9922717A3A7D240B9340917781EA5B9F577F` 仅本机自签信任，不保证其他电脑免SmartScreen。没有本轮安装/卸载或真实云账户测试，发布不代表用户博客部署。之后若重建，此表就成为历史，须重新核验。

## 7. 下次真正修改后的验证与打包

按改动范围选测试，不要为了接手无条件重跑所有历史任务：

```powershell
npm.cmd run test:ui -- --run
npm.cmd run build:ui
npm.cmd run prepare:template
npm.cmd run test:template
npm.cmd --prefix site run build
npm.cmd --prefix site run verify
powershell -ExecutionPolicy Bypass -File scripts/cargo.ps1 test -p veloct-app --lib
powershell -ExecutionPolicy Bypass -File scripts/cargo.ps1 clippy -p veloct-app --all-targets
node scripts/verify-profile-home.mjs
node scripts/verify-profile-settings.mjs
node scripts/verify-admin-dark.mjs
node scripts/verify-document-resources.mjs
node scripts/verify-editor-preview.mjs
node scripts/verify-edgeone-deploy.mjs
node scripts/verify-s3-config.mjs
node scripts/verify-five-ui.mjs
node scripts/verify-site-images.mjs
# 真项目验证需显式传入用户选中的项目根目录：
# node scripts/verify-existing-home.mjs <project-root>
```

改到跨crate行为再跑Rust workspace。1.0.1完整检查使用 `$env:CARGO_INCREMENTAL='0'` 和 `-- --test-threads=1`，避免沿用已知旧增量缓存错误；本次全量通过，但串行不能证明历史安装取消测试不稳定性已消除。如再运行应如实记录，不自动清理整个target或扩大到进程终止修复。浏览器脚本依赖本机 `target/browser-validation/` 下的 Playwright 与 Edge，不是新增的产品依赖；迁移环境先检查工具是否存在，不擅自改变依赖。

每轮产品修复按用户既有要求重建测试程序和 NSIS，纯文档整理不重建。构建前确认没有运行的 `veloct-desktop.exe`；遇锁文件请用户关闭，**禁止杀用户应用**。使用 [构建脚本](<scripts/build-installer.ps1>)，最终再用 [签名脚本](<scripts/sign-artifacts.ps1>) 的 `-Installer -Check` 和 [sidecar 检查](<scripts/build-sidecar.ps1>) 的 `-Check`。独立 EXE 若打包后为 NotSigned，补签后再独立复核，不凭先前的日志判断。

`Successfully signed: ...nst*.tmp` 只是 NSIS 临时文件，不代表最终包完成：必须看到 bundler 退出 0、新安装器时间戳/哈希和最终 Valid。若中断发生在编译已完成之后，先查产物和进程，再考虑 bundle-only 续做；不要重复十几分钟编译。构建脚本通常还原就地签名的源码 sidecar，中断后需确认还原/freshness，不能跳过。打包完成≠本包已装机，部署/发布仍需用户明确确认。

## 8. 可直接交给下一位 AI 的开场说明

```text
项目在 D:\Veloct-Blog。请先读 NEXT_AI.md、PROCESSING.md、ISSUE.md，按需查 UPDATE.md、README.md 和规格。
先核对当前工作树，保护所有已修改和未跟踪文件。
上一轮按用户授权发布1.0.1，包含全后台Claude暗色统一；标签v1.0.1，重构前3c51661和回退标签保留。最新产物与验证见第5/6节，不要重复已完成工作或自动修历史ISSUE。
发布复核完整Rust1576/0failed/17ignored；历史Windows进程终止测试仍有不稳定记录，真实云账户和新包装机也未验证。
读完简述当前状态与风险，等我给下一项任务。未经明确指示不安装、真实上传、部署、提交、推送或发布。
```
