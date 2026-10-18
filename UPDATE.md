# 最近更新与验证记录

> 面向测试者和下一位 AI。这里记录当前工作树中最近完成的用户可见调整；不是产品契约，也不替代 `docs/spec/02-test-matrix.md`。
>
> 当前工作树包含大量未提交代码和文档改动。下面的安装包是本工作树功能改动生成的本地测试包，不代表已发布版本。

## 交接文档整理（2026-10-18，仅文档）

- 用户要求整理Markdown后交给下一位AI。当前所有产品需求及签名包交付已结束，**没有待续编码任务**；下一位先读 [NEXT_AI.md](<NEXT_AI.md>)，检查工作树，再等用户指定下一项需求，不自动修历史ISSUE或发版。
- [PROCESSING.md](<PROCESSING.md>)收敛为仍有效的产品规则、数据边界与工作方式；整理前长记录保存在[归档](<docs/archive/PROCESSING-2026-10-18-before-handover.md>)，历史原文不丢失。接手入口补齐首页图片代码/回归入口、测试矩阵索引、浏览器命令，并修正过时锚点及真实项目历史核验的时间边界。
- [ISSUE.md](<ISSUE.md>)先列当前确认失败、未验证、交付和未提交工作风险，旧功能轮次/旧优先级明确标为历史。[README.md](<README.md>)修正部署/附件配置的JSON真相源及已移除AI桌面功能描述，文档导航职责统一。
- 文档检查：6份文档共151处本地链接/锚点均有效，当前两项产物哈希一致，已知workspace失败仍保留，定向 `git diff --check` 通过。**没有重跑产品测试、重新构建、安装、真实用户项目/云账户操作、提交或推送**。以下首页图片轮是最新功能证据，测试数、失败和EXE/NSIS哈希均沿用其已完成结果；旧轮次记录继续保留，不以旧全量通过覆盖当前失败。

## 首页头像 / 头图进入附件池并上传首选目标（2026-10-18，最新功能轮次）

- 首页头像保留512×512裁切；头像和头图现在都经原有附件缓存/哈希去重/上传事务，成功后返回云目标地址，不再为新上传写固定本地头像/头图文件。已有本地图片与手填地址不迁移、不删除。
- 首选规则：已配置图片同步时严格取其第一个目标，即使隐藏；缺失、仅本地或失效不会悄悄换桶。未配置时，在可见 S3/R2、GitHub、兰空图床中按上传优先级降序、同分按保存顺序选择。明确上传按钮只传此一个首选目标，不额外扇出其他目标；Secret仍在OS凭据库。
- 先入池再上传，失败保留待重试资源，不覆盖当前地址、不自动保存站点信息或改正文。S3/R2缺少有效公开域名时入池后拒绝网络写入；上传成功只更新当前表单，用户仍需保存。上传时禁用站点保存，保留同时编辑的作者字段，导航晚到响应不污染重开面板。
- 保存的头像/头图计入附件引用；重新打开项目仍识别，换图仅更新这两个站点引用，不删除文章引用或远程对象。上传/保存捕获原项目并在阻塞工作线程执行，后续项目切换不会把操作投到新项目。
- 验证：UI **271 tests / 21 files**，TS/Vite218modules；新增首页图片及五项界面/资源栏的生产Edge1440/1024回归通过，无JS错误。PNG夹具首次被ImageBitmap拒绝，换用浏览器canvas生成的有效PNG后通过，不放宽产品解码。
- 最终相关Rust：应用 **204 passed / 1 ignored**、desktop **155 passed**，clippy退出0，仅既有site-settings字段赋值警告。完整串行workspace已执行：**1575 passed / 1 failed / 17 ignored / 47 targets**，失败仅既有 `cancelling_the_install_kills_the_tree_it_started`（安装取消后子进程仍在运行）；单项复核也失败，未改进程终止代码，不宣称全量通过。新增首选规则、失败保留/去重/引用重建、无公开域名零上传等回归通过；最终小范围线程/域名防护变动后重新跑了上述应用/desktop与clippy。
- **最终交付完成**：无运行中桌面进程，sidecar→installer退出0（release5分47秒，NSIS完成）；独立EXE打包后需补签，补签及 `sign-artifacts.ps1 -Installer -Check` 两项均Valid，`build-sidecar.ps1 -Check` 与fresh build一致。沿用证书 `117C9922717A3A7D240B9340917781EA5B9F577F`，仅本机自签信任；版本仍1.0.0。没有写真实用户博客、真实上传到云账户、安装、部署或提交；浏览器与HTTP/存储使用隔离fixture，不是真实R2/CDN联调。
  - [测试程序](<target/release/veloct-desktop.exe>)：17,375,672字节，2026-10-18 14:09:41 +08:00，SHA-256 `D50FF290933CED2A767AF98611B65B74B5380AF9D9B3158B79B9D7CD2208746D`。
  - [NSIS安装包](<target/release/bundle/nsis/Veloct_1.0.0_x64-setup.exe>)：29,903,808字节，2026-10-18 14:09:25 +08:00，SHA-256 `C1FFF69401D33FB952436CBFB01F12E7E04C6CF6D86ABF8855E3FFEB7130EAD6`。

## 附件选择、通知、返回导航与移除 AI 建页（2026-10-18，前一轮）

- 附件管理的存储目标改为已配置策略下拉；沿用上传隐藏/优先级过滤，管理列表与后台同步规则不变。文件区可点击选择、拖拽、悬停或聚焦后 Ctrl+V 粘贴，先显示待加入清单/图片预览，再明确点击加入附件池；不自动上传、不自动插入或改写正文。
- 原生文件选择/拖拽只读预览走受限后端命令，不开放任意 file:// 权限。按文件头识别 PNG/JPEG/GIF/WebP/BMP，最多8MiB内联位图预览；其他文件只显示信息，不执行 SVG/HTML。一次最多20个文件、单个64MiB；浏览器/剪贴板字节在显式确认后才进原有缓存与哈希去重事务，拒绝非法文件名/编码/超限，导航晚到结果丢弃。
- 操作消息统一为右上角成功/失败/警告/信息通知，五秒倒计时进度、关闭按钮、点击查看完整详情；详情支持 Escape 和焦点循环。自动消失不清空加载错误/对象状态；冲突、逐目标结果、附件行错误等业务状态保留，读取重试仍有常驻入口。重复操作仍可再次通知，最多同时五条。UI临时ID不依赖非安全HTTP环境中的crypto.randomUUID。
- 面包屑左侧新增应用内上一页，保留前一个文章及未保存编辑缓冲；项目切换清空历史，未启动的站点预览不因返回隐式启动。
- 移除 AI 建页导航、页面创建器、设置及前端调用，删除桌面 AI 命令/注册/运行状态接线；普通页面编辑保留，不自动清理用户历史配置或系统凭据。共享领域库不等于仍提供桌面功能。
- 验证：UI **271 tests / 21 files**，TypeScript/Vite生产构建通过；最终 `CARGO_INCREMENTAL=0` 串行workspace **1569 passed / 0 failed / 17 ignored / 47 targets**（应用197+1ignored、desktop155，数量降低主要因删除AI桌面测试；另增字节导入集成测试），clippy退出0，仅既有site-settings字段赋值警告。五项界面、编辑器、本文资源、EdgeOne、S3生产bundle在1440/1024宽度均通过，无JS错误，IPC/原生文件事件使用隔离fixture，不是真实WebView剪贴板或云账户联调。
- 中途修正了共享通知TypeScript可选属性/ID类型、旧AI导航测试与旧浏览器页面内提示定位；旧S3脚本需允许五秒内重复同文通知。首张截图遇页面入场动画未结束，改为等待动画后复核，不是更改主题颜色。新增Rust字节命令集成测试后完整重跑成功。
- **最终交付完成**：首次预检查发现用户桌面仍运行，未结束用户进程；用户确认关闭后预检通过，sidecar检查及 `build-installer.ps1` 退出0（release 3分19秒，NSIS完成）。独立EXE打包后NotSigned已补签，`sign-artifacts.ps1 -Installer -Check` 两项均Valid，`build-sidecar.ps1 -Check` 确认bundler还原后的源launcher与fresh build一致。沿用证书 `117C9922717A3A7D240B9340917781EA5B9F577F`，仅本机自签信任，不保证别的电脑免SmartScreen。版本仍1.0.0；未写用户博客、真实上传、安装、部署或提交。
  - [测试程序](<target/release/veloct-desktop.exe>)：17,327,032字节，2026-10-18 12:57:24 +08:00，SHA-256 `8864FC52C6124795C8018CEFDCD3F50B178AB4B520CDE21001BA6797A5410F7B`。
  - [NSIS安装包](<target/release/bundle/nsis/Veloct_1.0.0_x64-setup.exe>)：29,900,672字节，2026-10-18 12:57:04 +08:00，SHA-256 `DBC04E1F2C911EAB2A981098B2B31D6A9292BB4E3B177A362265FCA409CA1DE9`。

## S3 / Cloudflare R2 存储策略配置（2026-10-17，前一轮）

- 按用户提供的策略表单拆分：名称、上传界面隐藏/显示优先级、桶、Endpoint 协议/Path 或 Virtual Hosted 风格/无协议主机、遮蔽的 Access Key ID/Secret、Region、上传目录、文件命名/同名处理、CDN 协议/域名、按扩展名首个匹配的网址后缀，以及无/自定义缩略图参数。不会预填用户账户端点或域名；R2 使用账户 Endpoint、Path Style、小写 `auto`，域名需事先由用户绑定。
- 核实 [Halo S3 文档](<https://github.com/halo-dev/plugin-s3#上传目录>)（网络受限时读取同源项目 README 的 jsDelivr 镜像），支持其日期、UUID、时间戳、随机字符串目录/命名占位符与原文件名。为保证附件重试路径稳定，时间来自**附件添加时 UTC**，UUID/随机名按持久附件标识确定；不是每次重试重新随机。空目录上传根目录。默认 UUID、新同名追加随机字母数字；另有原名、日期/时间/随机/自定义及同名报错选项。
- 新策略同名处理使用签名 `If-None-Match: *` 条件 PUT，发生 409/412 时更名或明确拒绝，不覆盖旧对象；冲突重试有上限，最终实际 key/URL 进入原事务记录。同内容已上传记录继续去重复用；已有配置缺少新策略时沿用旧路径/同名复用语义，编辑名称等不会暗中切换命名策略。命名/域名/后缀变化不自动重写既有 Markdown、对象记录或备份 S3 行为。
- 隐藏仅作用于上传下拉，优先级高者靠前；管理列表仍可编辑，既有多主机同步不会被悄悄停用。缩略图参数只用于资源预览，默认无；R2 自身并不提供图片缩放服务，需域名服务实际支持。网址后缀按扩展名顺序取首项，允许 `?` / `!` / `@` 服务商形式，缩略图不进入正文链接。
- 「验证对象存储」使用当前未保存配置与输入 Secret，留空可使用已存凭据；沿用签名只读 `ListObjectsV2(max-keys=1)`，后台执行，不写文件、不测试上传、不保存草稿/Secret，也不能证明 PUT 权限或 CDN 可用。Secret 仍只存系统凭据库；ID 在配置中保存、界面遮蔽。前后端拒绝非法地址、路径穿越、未知占位符与非法参数。
- 已完成 UI **267 tests / 20 files**、TypeScript/Vite 生产构建，以及真实 Edge 加载生产 bundle 的 1440/1024 双宽度隔离 IPC 检查：R2 表单字段、协议拼接、隐藏/优先级、旧配置编辑、Secret 清空/不回显、未保存只读验证、URL/缩略图规则、无 JS 错误。附件库 **278 passed**；最终 `CARGO_INCREMENTAL=0` 完整串行 workspace **1595 passed / 0 failed / 17 ignored / 47 targets**，应用195/1 ignored、desktop184，workspace clippy退出0，仅保留既有站点设置字段默认赋值警告。曾因两个命令集成测试未补新增字段、字段默认值类型不符导致编译失败，修正后完整重跑通过；首轮去重测试因夹具把不同字节误标为相同内容已修正，不改产品去重规则。
- 追加复核本文资源栏及 EdgeOne 部署的双宽度生产 bundle 浏览器脚本均通过，无 JS 错误；没有改动网站模板，不将历史网站/装机检查冒充本轮。
- 无真实云账户上传/删除、真实桶验证或 CDN 联调；不写用户博客、不安装、不提交或发布。前轮 EdgeOne 显式部署和本文资源隔离保留。
- 最终构建：sidecar 构建/检查通过，`build-installer.ps1` 退出0（release 6分26秒，完成NSIS）。独立EXE打包后 `NotSigned` 已补签，`sign-artifacts.ps1 -Installer -Check` 两项均 `Valid`，`build-sidecar.ps1 -Check` 再次确认bundler还原后的源launcher与fresh build一致。沿用本机自签证书 `117C9922717A3A7D240B9340917781EA5B9F577F`，不保证其他电脑免SmartScreen；版本仍1.0.0，未运行安装器。
  - [测试程序](<target/release/veloct-desktop.exe>)：17,502,136字节，2026-10-17 23:25:11 +08:00，SHA-256 `0FC9B9E229013F1A08ED248819BBAFF31256D4BB945C432B0152BE7821D36BC9`。
  - [NSIS安装包](<target/release/bundle/nsis/Veloct_1.0.0_x64-setup.exe>)：29,950,648字节，2026-10-17 23:25:00 +08:00，SHA-256 `1934969EFDFC3D1E778C43CB2EA4E17211F7C208A51D793BD3F50993138FC861`。

## EdgeOne 国内 / 国际显式直传部署（2026-10-17，前一轮）

- 按最新要求：**只有点击「部署当前构建」才上传**。发布/构建、打开设置、保存目标或 Token 都不上传；范围是全部已启用目标与现有发布产物的完整 `dist/site` / `dist/works`，不是整个项目目录。主站和作品站使用不同项目，缺少作品项目仍明确跳过，不混入主站。
- 以官方 [Makers SDK 0.1.0](<https://unpkg.com/@edgeone/makers-sdk@0.1.0/dist/client.js>) / contract 0.1.39 核实协议，在 Rust 内实现，未安装 CLI/新增 Node 进程。国内 `pages-api.cloud.tencent.com/v1`、国际 `pages-api.edgeone.ai/v1`；Pages API Token 仅发给对应区域，沿用系统凭据库存储、配置仅留 handle。不是腾讯云 SecretId/SecretKey，不向用户索取聊天明文密钥，不自动切换区域或创建项目。
- 只接受现有 `Provider=Upload` 的直接上传项目：检查项目 → 获取临时 COS 凭据 → ZIP 完整产物并按 COS HMAC-SHA1 签名上传 → 创建 Production 部署 → 查询对应部署状态。仅 `Success` 算成功，创建已接收不算上线；失败/未知状态/约 15 分钟等待超时均明确提示，不自动重传。COS 只接收临时凭据，不携带 Pages Token，HTTP 客户端禁止跟随重定向，错误不回显云端原文/令牌。
- 发布目录读取和打包均拒绝路径穿越、符号链接、`.env`、`.git`、`.veloct`、附件/工具缓存及密钥文件；保留合法 `.well-known` 和全部静态资源。主站/作品站隔离、某个子目标失败不抹去已成功站点，也不阻塞后续目标。原有 Cloudflare 与 1Panel 导出保留。部署命令改为后台阻塞任务，避免打包/上传/轮询阻塞 WebView。
- 设置面板改为「自动直传」，区分地区、项目 ID 与 Token，提供编辑已有目标（不会回显凭据），前后端均阻止主站/作品站使用相同项目。界面展示全部目标与子目标真实结果，失败、部分成功和空运行不再统一提示“部署成功”。
- 验证：应用 **195 passed / 1 ignored**，部署 crate **105 passed**；UI **259 tests / 19 files**、TypeScript/Vite 生产构建通过。真实 Edge 加载生产 bundle，1440/1024 宽度的隔离 IPC fixture 验证区域切换、配置保存零部署、Token 清空/不回显、重叠项目拦截、显式点击一次才部署、异步忙状态和部分失败提示，无 JS 错误。COS 签名测试与官方 `cos-nodejs-sdk-v5@2.15.4` 固定向量一致，ZIP 解包校验 Unicode 名称与原始字节；双区域/双根集成测试验证全部文件、禁用目标零请求、缺凭据零请求和失败隔离。
- 首次新增测试编译因本地变量遮蔽测试工厂名失败，重命名后通过；首次 clippy 提示新文件 tests 后还有 helper，调整顺序后只剩站点设置测试既有警告。完整串行 workspace 首轮已退出 0（1580 passed / 0 failed / 17 ignored / 47 targets，含此前不稳定 taskkill 测试）；不会抹去前轮失败，也没有修改进程终止代码。
- 交付前复核补充了误放入发布目录的附件缓存、`.npmrc`、`.git-credentials`、`.pem` 拦截测试，为保证包对应最终源码，主动取消了第一次 release 构建并重新验证。最终 `CARGO_INCREMENTAL=0` 完整串行 workspace 再次通过（1580 passed / 0 failed / 17 ignored / 47 targets），随后 workspace clippy 退出0，仅既有站点设置默认字段赋值警告。没有写实际博客、真实云端部署、安装、提交或发布；官网接口/fixture 验证**不是用户真实账户联调**，首次真实上线仍需用户确认。
- 最终交付：重新运行 `build-installer.ps1` 退出0（release 6分11秒，NSIS 完成）；独立 EXE 打包后 `NotSigned` 已补签，`sign-artifacts.ps1 -Installer -Check` 两项均 `Valid`，`build-sidecar.ps1 -Check` 确认还原后的源 launcher 与 fresh build 一致。沿用本机自签证书 `117C9922717A3A7D240B9340917781EA5B9F577F`，不保证别的电脑免 SmartScreen。
  - [测试程序](<target/release/veloct-desktop.exe>)：17,388,472 字节，2026-10-17 17:12:32 +08:00，SHA-256 `3245BEE5783E12AC723BE624B36850A8F7D9CC46EACBAA1A794C46AE5F7FEFEA`。
  - [NSIS 安装包](<target/release/bundle/nsis/Veloct_1.0.0_x64-setup.exe>)：29,911,696 字节，2026-10-17 17:12:17 +08:00，SHA-256 `93CA2BDB19223A2E253DEEF6CB85B179DC5DD76EFC85AEFE80F12538A415C398`。
  - 版本仍 `1.0.0`，未安装、发布或真实部署；Vite 大包/MSVC 链接提示保留。

## 文章编辑右侧资源栏（2026-10-17，前一轮）

- 在文章编辑器右侧增加紧凑「本文资源」栏，提供「附件池 / 图片池」标签、数量、搜索、刷新和收起/展开。默认宽 240–300px，各自滚动；只有一个正文编辑器，未恢复旧右侧预览或分隔条，页面/知识库和网站首页不增加此栏。窄窗口文章标题与操作分行，避免标题被挤没。
- 新加文件使用现有附件接口及 `document-article-<稳定ID>` namespace 持久保存，未插入也能留在本文池；旧附件按正文中的真实 Markdown 链接/图片引用归入本文，不把整个项目资源直接冒充本文资源。另有「从项目资源池选用」，只有明确选用插入后才计入本文。使用同一套 Markdown grammar 读取正文，排除 frontmatter、行内/围栏代码中的假链接。
- 支持本地多选添加、选择存储目标、沿用既有单目标/图片多主机同步上传、失败提示与重试，以及插入当前编辑位置。添加/上传均不自动修改正文；插入走富文本或 MD 光标/选区和原自动保存，frontmatter 原样保留。切换文章/项目后到达的异步插入与文件选择结果不落入新文章。
- 图片池区分本地附件图和正文已有的远程/站点相对图片；本地未上传图片显示占位，HTTPS 远程图可显示缩略图，失效/防外链时有回退提示。可直接插入 HTTPS 网络图片，不下载、不重传。未开放任意本地文件协议/目录权限；移除资源、删除远程对象仍经已有全局管理，不在本文小栏内提供容易误删共享资源的动作。
- 验证：UI **254 tests / 18 files**（新增 10 项本文资源归属/分类/Markdown/URL 安全回归）及 TypeScript/Vite 生产构建通过。真实 Edge 加载生产 bundle，IPC/文件选择/云存储使用隔离 fixture，1440×900 与 1024×720 均验证标签键盘、本文隔离、添加不插入、上传失败重试且不改正文、富文本/MD 插入、frontmatter、远程图不下载、晚到操作隔离、文章切回仍保留池与收起展开，无 JS 错误、无水平溢出；右栏分别 300/240px。旧编辑器脚本同步验证 1440×900 / 1024×640，保存状态稳定、磁盘冲突保留、MD 切换及独立真实预览入口不回归。
- 首轮新增单测暴露共享 Markdown parser 会保留图片 alt 的反斜线表示，断言改为分别验证规范 Markdown 转义与图片 URL；浏览器首轮失败来自精确文本定位及懒加载尚未进入视口，改为滚动到图片并检查占位节点后通过。人工查看窄窗口截图另发现标题被按钮挤压，已修布局并新增标题宽度断言后重跑通过。
- 本轮仅改桌面前端，不改 Rust 业务/网站模板，未重跑上一轮有已知 Windows taskkill 失败的全 Rust workspace；不把此前全量失败改写成通过。云端上传是 fixture 验证，不冒充真实账户联调；未写用户实际博客，未安装、部署、提交或发布。
- 最终交付：`build-installer.ps1` 退出 0（release 编译 12分15秒，完成 NSIS）；第一次独立签名 Check 如预期发现桌面 EXE `NotSigned`、安装器 `Valid`，补签独立程序后 `sign-artifacts.ps1 -Installer -Check` 两项均 `Valid`。`build-sidecar.ps1 -Check` 通过，bundler 还原源 sidecar 后再次与 fresh debug build 校验 SHA-256 一致（`9E4562662643C0F4DAA1F96E9BDD3CD741CE0E8A350AD4BCA23AD307FDF08E76`）。本机自签证书沿用，不能保证其他电脑免 SmartScreen。
  - [测试程序](<target/release/veloct-desktop.exe>)：17,278,392 字节，2026-10-17 16:34:13 +08:00，SHA-256 `32835959BCDA1263A7A8ACDAEA396D0FD8AA936269689A583456E44CF1F1E996`。
  - [NSIS 安装包](<target/release/bundle/nsis/Veloct_1.0.0_x64-setup.exe>)：29,893,784 字节，2026-10-17 16:33:53 +08:00，SHA-256 `2B89608B45E779F58EDBCD9F4743A1DBF070FAB496B3E46185E00A11FF18FF9C`。
  - 没有运行安装器、发布或部署；版本仍为 `1.0.0`。保留 Vite 大包及 MSVC 链接消息提示。

## GitHub 热力图同步 HTTP 500 修复（2026-10-17，前一轮）

- 根因已用真实网络请求复现：机器已经启用 Windows 系统代理，但 workspace 的 reqwest 使用 `default-features = false`，没有显式启用 `system-proxy`，导致生产 Rust 客户端绕过系统代理。对同一公开账号，直连/修复前 Rust 请求返回 HTTP 500，通过系统代理/修复后 Rust 请求成功。
- 在 [workspace 依赖](<Cargo.toml>) 显式恢复 `system-proxy`；[锁文件](<Cargo.lock>) 仅新增其四个传递依赖。由 reqwest 读取操作系统代理与绕过规则，保留环境代理优先级，不写死本机代理地址，不修改系统代理/hosts，不使用第三方 GitHub API、不增加令牌或自动重试。公开日历来源、校验和失败保留快照逻辑未改。
- 新增可选真实联网测试 `github_activity::tests::live_calendar_uses_production_client`，使用与 Tauri 同款的 `ReqwestHttpClient::without_redirects(20s)` + `fetch_calendar`，不打开或写入用户项目。`VELOCT_GITHUB_TEST_USERNAME` 可选择公开账号，未设时使用 `octocat`。以目标账号运行 `scripts/cargo.ps1 test -p veloct-app --lib live_calendar_uses_production_client -- --ignored --nocapture`：修复前退出 101 / HTTP 500；修复后退出 0，解析 **371 天（2025-10-12 至 2026-10-17）** 官方日历。该测试默认 ignored，普通回归不依赖公网；本轮不是只有 fixture 的成功证据，也不是新安装包 UI 装机验证。
- 扩展离线回归：有效完整日历、官方 URL/无 Cookie 与 Authorization、无效用户名零请求；HTTP 500/502/503/504/403/404/429，以及 HTTP 200 登录页/缺天/断日/重复日期/非法等级/超大正文均失败并保持旧快照及成功时间的完整字节。
- 已验证：UI **244 tests / 17 files**；`clippy --workspace --all-targets` 退出 0（保留站点设置测试既有默认字段赋值警告）；应用库 **192 passed / 0 failed / 1 ignored**，新增联网测试已另外显式通过。
- 全量失败如实记录：首轮 `test --workspace --no-fail-fast` 在 desktop 测试链接时报 ``crate `reqwest` required to be available in rlib format, but was not found in this form``。仅为复核在子进程设置 `CARGO_INCREMENTAL=0`，没有清理用户文件或更改产品构建配置；重跑编译成功，但 Windows 进程终止两项测试失败：`state::tests::a_real_spawner_starts_a_process_keeps_its_output_and_kills_its_tree`（[state.rs:1692](<src-tauri/src/state.rs#L1692>)）和 `cancelling_the_install_kills_the_tree_it_started`（[site_install.rs:247](<src-tauri/tests/site_install.rs#L247>)），均为 `taskkill` 报 `There is no running instance of the task.`。再以 `-- --test-threads=1` 复核，安装取消通过（该 target 3 passed / 1 ignored），desktop 库仍为 183 passed / 1 failed；最终全量退出 101。此次未修改进程终止代码，也没有跳过测试冒充全绿。
- 构建与签名：`build-sidecar.ps1` → `build-installer.ps1` 完整退出 0（release 编译 18m48s、NSIS 产物完成）；bundler 后独立 EXE 仍需补签，已用既有本地证书补签，并以 `sign-artifacts.ps1 -Installer -Check` 核验两项均 **Valid**。`build-sidecar.ps1 -Check` 退出 0，确认打包脚本还原的 launcher 与 fresh build 字节一致。签名为本地自签，不保证其他电脑免 SmartScreen。
- 最终新包（2026-10-17，本轮新哈希）：

  | 文件 | 字节 | 最后修改时间（+08:00） | SHA-256 |
  |---|---:|---|---|
  | [测试程序](<target/release/veloct-desktop.exe>) | 17,274,808 | 15:46:24 | `F4D08811DC5B3C53225368F7F1F3C2F83517E22B3642A29D69C6D8BEB246F747` |
  | [NSIS 安装包](<target/release/bundle/nsis/Veloct_1.0.0_x64-setup.exe>) | 29,882,928 | 15:46:10 | `244B78EF3FAA8E336DDEDBA69319C992573024E4B39CF4149F1478A40299FEAF` |

- UI / 网站模板未修改，没有改写实际博客配置、快照或文章，未安装、部署或发布；这不是新安装包已装机验收的声明。

## 下一位 AI 交接文档整理（2026-10-16，仅文档）

- 新增 [NEXT_AI.md](<NEXT_AI.md>) 作为接手入口，明确当前已完成、暂无未完成编码任务；下位 AI 先了解用户下一项需求，不默认重做历史测试或发布。
- 同步 README 的阅读顺序、设置真相源、签名边界和契约优先级；PROCESSING 新增顶部检查点；ISSUE 区分当前验证缺口和旧轮次风险，避免误读旧右侧预览、旧包哈希或“无人做过像素验证”。
- 记录实际博客与应用源码分离、精确官方模板升级及备份、用户内容保护、大量尚未提交的工作，以及单栏写作 / 单张365天图 / 标题旁切换的最终形态。
- **本次不改业务代码，不重跑业务测试、不重建、不安装或发布。** 以下最近功能轮次的产物和证据保持不变，文档核对不冒充新业务验收。
- 文档一致性检查通过：5 份文档、49 个本地链接可定位，现有程序/安装器字节与 SHA-256 均与交接表一致；文档范围 `git diff --check` 通过。这些不是重跑业务测试或新装机证据。

## 热力图切换按钮移至标题旁（2026-10-16，最近功能轮次）

- 将切换按钮改为「博客创作 / GitHub 贡献」标题旁的紧凑图标按钮，保留完整可访问名称、目标提示、鼠标和 Enter/空格操作；移除原底部按钮行，不再为它留白。标题、统计和日历跟随同一来源切换，仍只显示一张近 365 天图；原数据范围、隐私规则、联系方式隐藏与单/空来源设置不变。
- 三尺寸真实 Edge 验证按钮与标题同行、无底部操作行、全年日期格、键盘与焦点、无切换请求、路由返回无重复绑定及卡片尺寸稳定。移动端回归发现 Tailwind 的重要 `[hidden]` 重置会折叠原本预留尺寸的面板/统计，改用 `aria-hidden` + `inert` 和不可见叠放，兼顾稳定尺寸与禁止隐藏内容交互；仅提高普通 CSS 优先级不能覆盖重要层规则，没有全局覆盖隐藏行为。
- 已补三份上一版完整官方基线及精确哈希，保留 LF/CRLF、备份、幂等和自定义代码保护。实际博客三份匹配模板先备份再更新，8 个内容/配置/资源文件哈希完全相同；冻结时间构建 8 页，双尺寸实际产物验证通过，并检查原个人资料保留。
- UI 构建、**244 tests / 17 files**、模板测试、应用 **190 tests** / clippy、网站 **21 页 / 64 检查**通过；主页 Edge 1440×900 / 1024×640 / 390×844、设置双尺寸和实际博客双尺寸均通过，无页面 JS 错误。本轮未重跑 Rust 全 workspace，既有大包/clippy/空知识库提示保留。
- release 编译已成功（4分53秒）。首轮在 NSIS 临时文件签名后被用户中断，主程序已是新版，但最终安装器时间戳仍是上一轮；因此只用 Tauri `bundle` 续做 NSIS，不重新编译或重跑已通过检查。临时文件的 `Successfully signed` 不是最终安装器完成的证据。续打包退出 0；新程序和安装器独立 Check 均 **Valid**，sidecar 与新构建一致，三份 staged 模板与最终源文件一致。仍仅本机自签信任，未安装或部署。
  - [测试程序](<target/release/veloct-desktop.exe>)：17,266,624 bytes，SHA-256 `64EA05E3D117D695DB9E9A8162CA2438D4C24E2D927C2F419AA121C1A472E682`。
  - [安装包](<target/release/bundle/nsis/Veloct_1.0.0_x64-setup.exe>)：29,883,352 bytes，SHA-256 `DE09567588B961D75C3D1B4C2168F8BEDA29E5D9F768A17A4BB3E10FDEFAC6ED`。
- 同版本主页完整脚本先前已通过；后来补充隐藏来源不可交互断言的重跑在三种尺寸均通过后触及 120 秒命令时限，未继续声称该次剩余单/空来源构建阶段完成。用户要求继续后不重复已完成测试，仅续打包并核验最终文件。

## 首页热力图恢复年度范围（2026-10-16）

- 按最新要求，恢复原来的**近 365 天**范围，不再只显示 90 天。仍只显示一张图，保留右下角「博客创作 / GitHub 贡献」切换、固定按钮和稳定占位，不恢复上下两张图。
- 同步恢复年度图的宽度与可横向滚动容器，小窗口可查看完整全年，不压缩为难读的小格；月份、日期提示、图例、公开创作统计和 GitHub 活跃度含义保持。空联系方式/全空区隐藏、最多 16 项及原配置不动。
- 官方模板升级新增上一版三份完整文件精确哈希，测试覆盖 LF/CRLF、原件备份、幂等、自定义修改保护。实际博客项目三份匹配文件已备份更新，8 个内容/配置/资源文件哈希不变；冻结本轮时间构建 8 页及双尺寸实际产物验证通过。
- UI 构建 / **244 tests / 17 files**、模板测试、应用 **190 tests** / clippy、网站 **21 页 / 64 检查**通过；主页 Edge 的 1440×900 / 1024×640 / 390×844 验证 365 个日期格、单图切换、键盘、无远程请求、无横向页面溢出及联系人行为；设置 Edge 双尺寸通过。本轮未重跑 Rust 全 workspace。首个合并命令在主页验证完成后达到 120 秒上限，剩余设置浏览器检查单独重跑通过，不计作编译或断言失败。既有大包/clippy/空知识库提示保留。
- NSIS 重建成功（release 11分37秒）。独立测试程序打包后补签，两项独立签名 Check 均 **Valid**；sidecar 与新构建字节一致。证书仍是本机自签，不代表其他电脑的 SmartScreen 信任；没有安装或发布。最终产物：
  - [测试程序](<target/release/veloct-desktop.exe>)：17,266,616 bytes，SHA-256 `5763EC056F3A638921E780124B4AF72379FADAC2E68FB6DC1CF46F53C4958C79`。
  - [安装包](<target/release/bundle/nsis/Veloct_1.0.0_x64-setup.exe>)：29,862,584 bytes，SHA-256 `41361EFB2C5301631E2A12DF03ED230C0A9F5D433CABDDAD776706D42A9B4EAC`。

## 首页单图切换与空联系方式隐藏（2026-10-16）

- 用户随后修订方案：取消双列竖排，首页只显示**一张近 90 天热力图**，右下角按钮在「博客创作 / GitHub 贡献」之间切换。两个原设置开关继续有效：只开一项时不显示切换按钮，两项关闭显示原空态。切换只使用已经生成的本地数据，不请求远程、不重新加载页面；GitHub 仍为活跃等级，不冒充提交数量。
- 日历改为滚动 90 天，保留 UTC 星期对齐、日期提示、日期范围和颜色图例；只统计窗口内的公开创作。GitHub 原完整年度快照及校验不删减，仅裁剪显示。开发预览按当天定位，生产按冻结构建时间定位，保持发布可重现。两个数据面板重叠占位但同一时刻只有一张可见/可交互，切换按钮固定宽度、底边位置不跳动。
- 右侧只生成有效已填联系方式，空链接和空 ID（含纯空格）自动隐藏，不再生成加号/虚线空位；全空时整个联系方式区隐藏，日历利用剩余空间。最多显示 16 个、每行最多 4 个，原保存数组、排序、未知字段和账号数据不删改；桌面说明同步更新。
- 补齐四份上一版完整组件/帮助模块/CSS 基线，旧官方项目自动备份升级，自定义文本仍保留。实际用户项目四份匹配基线模板已备份并更新；8 个内容/配置/静态资源文件 SHA-256 不变。实际项目以本轮显式冻结时间构建（8 页），双尺寸 Edge 验证单图/切换/空位隐藏与个人资料保留。
- 验证：UI **244 tests / 17 files**、模板测试与 staging、应用 Rust **190 tests** / clippy、网站 **21 页 / 64 检查**通过；主页 Edge 在 1440×900 / 1024×640 / 390×844 验证鼠标、Enter、空格切换、按钮几何稳定、无新增请求、ClientRouter 返回后不重复触发、90 天边界、私有草稿不泄露、ID 复制、空/单来源和空联系人；设置 Edge 双尺寸通过。保留既有 Vite 大包、应用 clippy 默认赋值与实际项目空知识库提示，本轮未重跑全 Rust workspace。
- 浏览器首轮发现按钮文案长短导致宽度变化，补固定宽度后回归；下一轮坐标差来自点击自动滚动，先滚动按钮入视口再比较后全尺寸通过。实际项目截图路径已改为固定仓库测试目录，不跟随调用工作目录；清理了本轮误写到项目构建子目录的两张测试截图，没有删除用户资源。
- 新测试程序与 NSIS 安装包重建成功（release 编译 10 分 28 秒）。打包后独立 EXE 为 NotSigned，已补签；程序和安装包独立复核均为 **Valid**，sidecar 与新构建相符，四份 staged 模板与最终源文件逐字节一致。新 release 实窗回归 ICON_BIG 64×64（alpha 2897 / chromatic 2655）、ICON_SMALL 32×32（alpha 837 / chromatic 762），隔离测试窗正常退出。未运行安装器或部署；签名仍为本地自签，不代表通用 SmartScreen 信任。打包器签名前的 `No signature found` 之后已成功签署，不是本轮构建失败。

| 本轮产物 | 字节 | SHA-256 |
|---|---:|---|
| `target/release/veloct-desktop.exe` | 17,266,104 | `89CEF154BFD473FEE51428DC48D6382BE95CF650301682720DFF213A379C5625` |
| `target/release/bundle/nsis/Veloct_1.0.0_x64-setup.exe` | 29,858,152 | `81E7CC988E637ADB4A80F6DF76074AAEC3641AD87455E37B1F8864A957752941` |

## 实际旧项目首页未更新：补齐模板识别（2026-10-16）

- 用户提供实际项目后核实：不是浏览器缓存。升级日志显示三份旧模板未匹配已知基线，被自定义保护跳过；新组件存在但旧首页仍渲染 Hero/文章列表，旧布局无条件挂载右栏，旧配置读取器也没有主页默认值。上轮隔离 fixture 通过不等于此真实旧项目已经更新。
- 用户明确同意备份后替换后，只更新三份模板代码。原件在用户项目 `.veloct/template-backups/profile-home-manual-20261016-164136/`；内容、知识库、作品、数据配置及静态资源范围内 8 个已有文件替换前后 SHA-256 全部相同，没有清空个人资料、头像或文章。
- 将这批完整旧文本及其规范化 SHA-256 纳入[升级精确基线](crates/veloct-app/src/project.rs)，不是按片段猜模板。扩展同一升级回归，验证 LF/CRLF、原件备份、幂等、追加用户修改仍拒绝覆盖、用户内容不变。Rust 应用 **190 tests** 与应用 clippy 通过，保留既有默认值赋值警告；本轮未重跑全 workspace，不把上轮全量结果当成本轮证据。
- 实际项目网站构建成功（8 页，知识库为空的既有提示）；[实际项目 Edge 检查](scripts/verify-existing-home.mjs)在 1440×900 / 1024×640 验证读取原个人信息、双日历、16 联系格、无原右栏/横溢出和页面 JS 错误。首次检查使用了不存在的 `.profile-heatmap` 选择器，修正为组件实际的 `data-activity` 后双尺寸通过，没有修改正确的页面实现。
- UI **242 tests / 17 files** 和模板测试通过；桌面 UI 生产构建通过，保留既有 Vite 大包提示。已重建[桌面程序](target/release/veloct-desktop.exe)（17,246,648 字节）与[安装包](target/release/bundle/nsis/Veloct_1.0.0_x64-setup.exe)（29,842,280 字节），打包退出 0。独立程序补签后两份文件签名 Check 均 Valid（仅本机自签信任）；sidecar Check 通过。构建前无用户应用进程，未结束用户进程，未安装或发布。
- 最终 SHA-256：应用 `EBC1A7409EF2AD63EAAE201557A6E891C7C5C645C4ECF8494BCA8CDFEBAAA97D`；安装器 `0003AB48D5C2026315902ACEE76769A76310FDA2AF24A44580391BDE34DE16EB`。

## 个人主页布局与双热力图（2026-10-16）

- 按用户新要求，以 osu! 个人页主区域的结构为参考，替换上一轮临时留白：首页保留头图、叠放头像、右侧用户名和站点描述；中部左侧为博客创作/GitHub 双热力图、右侧为固定 4×4 联系图标区；底部为独立个人介绍。保留全站左导航与页脚，首页不再挂载原右侧栏。共享布局也识别规范首页路径，避免旧官方首页遗漏开关后再次插入原右栏，其他页面保留右栏。
- 外观 → 站点信息：头图地址/上传、头像裁切、名字、站点描述、**独立个人介绍**、联系方式/ID 与上移下移排序。新增联系人最多 16 个，空位保留；超过 16 个的旧配置不截断存储，只限制主页显示。图标链接打开目标，只有 ID 的项目点击复制；危险 URL 不作为链接。上传头图转为受限尺寸 PNG，存入项目静态目录，随项目备份/发布，不引用应用临时地址。
- 设置 → 站点设置 → 首页与热力图：两个图分别开关，关联 GitHub 用户名并显式同步。GitHub 公开日历按日期及 0–4 活跃等级验证，**等级不冒充贡献/提交数量**；无需令牌，不调用第三方代理，不存远程 HTML。成功后原子写入项目的 `data/github-activity.json`，失败保留上次快照和成功时间；换账号不复用旧账号数据。网站只离线渲染快照，需手动再同步和发布；首次未同步显示真实空态。
- 公开博客热力图按文章/知识条目创建日期计数，排除普通页面、未公开草稿，避免泄露私有创作元数据；以冻结构建时间和数据日期确定日历，不引入构建时墙钟随机性。首页介绍和站点描述不再共用一个字段。
- 仅匹配完整旧官方文本/哈希的首页、布局、配置读取器才升级，并先备份；自定义模板保留，写入日志提醒。新增[留白首页精确基线](crates/veloct-app/template-baselines/src/pages/index.blank.astro)，扩展升级测试覆盖原首页及上轮留白首页、LF/CRLF、幂等、备份、用户内容不变。新增布局、组件和帮助模块也通过干净模板 staging 带入新旧项目；没有新增依赖或改写用户配置/文章。
- 验证：UI **242 tests / 17 files**、模板 staging/测试、网站生产构建 **21 页 / 62 检查**、Rust workspace **1561 passed / 0 failed / 16 ignored（47 targets）**及 clippy 通过（保留既有 `site_settings` 测试的默认值赋值警告、Vite 大包提示）。[主页 Edge 回归](scripts/verify-profile-home.mjs)覆盖 1440×900 / 1024×640 / 390×844，头像头图、独立字段、双日历、隐藏私有草稿、16 格/4 列、ID 复制、无原右栏/无横溢出、生产开关/账号隔离；[设置 Edge 回归](scripts/verify-profile-settings.mjs)覆盖双桌面尺寸的上传、排序、16 项上限、保存、同步错误提示，无 JS 错误。均用隔离 fixture，不操作用户项目；GitHub 测试使用 HTML/HTTP fixture，未声称本轮真实账号联网同步已验证，公开 HTML 变化会给出失败提示而非虚构数据。
- 写作单栏、菜单页面选择、原位删除/链接知识库浏览器回归仍通过。预览缩放采样首次在 1024px/移动 125% 出现一次不稳定，补充失败样本输出后原生产代码不变重跑完整矩阵通过；保留这次偶发现象，不将复跑通过当作首轮未失败。网站检查首次仍断言临时空白主页，已随新产品要求更新为真实个人主页契约后 62/62 通过。
- 已重建[桌面程序](target/release/veloct-desktop.exe)（17,246,648 字节）与[安装包](target/release/bundle/nsis/Veloct_1.0.0_x64-setup.exe)（29,849,408 字节，28.47 MiB），打包退出 0。打包中签名成功，独立程序打包后仍为 NotSigned，已补签；两份交付文件独立签名 Check 均 Valid（仅本机自签信任，不保证其他机器免 SmartScreen），sidecar Check 通过。构建前无用户应用进程，未结束用户进程、未运行安装器或发布。下节是已被本轮替代的留白中间状态。
- 最终 SHA-256：应用 `F02CDE3CA6871692B79634FBDD93EEF1DCBD48BDE71150B8FB7661C21AAD000A`；安装器 `75FD169765F9DB678EC0450EEC9F0ACB325B59EC4579784A4B954E25E68D34A7`。

## 首页留白（2026-10-16，历史，已被个人主页替代）

- 按用户后续收窄要求，**本轮不修联系方式 / 账号 ID 的展示**。网站首页清空 Hero、首页摘要/正文、最新文章、标签等主内容，且完全不挂载首页右侧栏（不是隐藏空栏）；保留左侧全站导航、页脚、SEO 和首页地址。其他页面继续使用原右侧栏。
- 仅调整[首页模板](site/src/pages/index.astro)和[共享布局](site/src/layouts/BaseLayout.astro)的可选右栏开关；没有删除首页 Markdown、文章、知识库或改写站点配置。原首页正文/摘要、Hero 配置仍保存但暂不在首页展示，供后续重设计。
- 旧项目打开时仅升级完整内容/已知哈希匹配的官方首页与布局，原件先备份；自定义模板继续保留并记录需合并的日志。新增[精确旧首页基线](crates/veloct-app/template-baselines/src/pages/index.before-clear.astro)及[升级回归](crates/veloct-app/src/project.rs)，覆盖 LF/CRLF、幂等、备份、自定义保护和用户文件字节不变。
- 验证：网站生产构建 21 页、57/57 产物检查；UI 236 tests / 16 files、模板测试与 staging 通过；Rust 应用全部 186 tests、定向项目 17 tests、应用 clippy 通过，未重跑整个 Rust workspace。[真实 Edge 首页回归](scripts/verify-empty-home.mjs)在 1440×900 / 1024×640 验证空主区域、无右侧栏、导航页脚保留、文章列表仍有右栏以及 ClientRouter 返回首页依然留白，无页面 JS 错误。无新增依赖，未操作用户项目。
- 首次重建中 UI 生产构建、release 编译和应用/安装器签名成功，但最终 bundler 退出 1：`另一个程序正在使用此文件，进程无法访问。 (os error 32)`。构建前无运行应用；结束时检测到测试程序在 13:16:10 启动（PID 9732，当前 `target/release/veloct-desktop.exe`），属于构建过程中打开应用。未结束用户进程；用户确认关闭后重新打包退出 0，保留首次失败记录，不以旧文件签名代替成功构建。
- 最终[桌面程序](target/release/veloct-desktop.exe) 17,163,704 字节，[安装包](target/release/bundle/nsis/Veloct_1.0.0_x64-setup.exe) 29,820,784 字节（28.44 MiB）。独立主程序补签后两份交付文件独立签名 Check 均 Valid（仅本机自签信任，不保证其他机器消除 SmartScreen），sidecar Check 通过；未安装或发布。
- 最终 SHA-256：应用 `C215D041B6CC880DCD08C79B0084558AF879413738D3195C2D46558C38DC4698`；安装器 `ACEBEB014F976770105263D773F1BE621F14D62EDD939746DE316B635966163A`。

## 编辑区只保留所见即所得（2026-10-16）

- 按用户最新要求移除文章/页面/知识库共用编辑区右侧的重复 Markdown 预览及拖动分隔条，编辑器占满内容区宽度，不留下空白列。所见即所得与 MD 源码切换、摘要、附件、WIP、自动保存和冲突处理继续保留。
- 独立「页面预览」与外观预览仍使用真实站点；原右侧的「打开站点预览」按钮移到编辑区标题操作行，不再在写作时挂载第二个阅读视图。更新空态和按钮提示，移除不再使用的分栏状态/拖动逻辑。
- UI 生产构建和 236 项测试（16 文件）通过；[Edge 写作回归](scripts/verify-editor-preview.mjs)在 1440×900 / 1024×640 验证单栏等宽、无预览/分隔条/iframe、MD 与富文本内容一致、保存状态不抖动、摘要/未知 frontmatter 保留、打开零写入、冲突保留缓冲区、文档切换及独立站点入口。首次回归因测试未考虑摘要以 JSON 字符串写入 YAML 而超时，修正断言后双尺寸通过，无页面 JS 错误；未修改正确的摘要保存逻辑。
- 菜单页面选择弹窗与外观双尺寸/双设备/五档缩放回归均通过。均使用隔离 IPC，不操作用户项目。本轮只改桌面 UI，没有新增依赖或修改 Rust/网站模板；未重跑 Rust 全量/clippy/网站构建，保留既有 Vite 大包提示。
- 已重建[桌面程序](target/release/veloct-desktop.exe)（17,163,192 字节）与[安装包](target/release/bundle/nsis/Veloct_1.0.0_x64-setup.exe)（29,807,968 字节，28.43 MiB）。打包退出 0，独立主程序补签后两者独立签名 Check 均 Valid（仅本机自签信任，不保证其他机器消除 SmartScreen），sidecar Check 通过；未安装或发布。
- 最终 SHA-256：应用 `AFFEC584DF93AA0FCDCCE2F453CAB1CB09996DB9C50BF8F013FB462150B85E7A`；安装器 `05203D583AD4B7AB655702C814BF6B8F2555B392CF7D0E229543CD90E66F5AE7`。

## 新增菜单选择页面链接（2026-10-16）

- 「新增菜单」字段改为「链接地址/页面」，输入框右侧新增小型「选择页面」按钮；仍可手动填写站内路径或外部网址。
- 弹窗复用当前项目内容列表，只显示可生成的页面（含首页与自定义页），不混入文章、知识库或校验失败的页面。按名称/路径搜索，直接使用内容服务返回的规范路由，首页仍为 `/`，不从 slug 猜地址。
- 选择只填入地址，不直接新增菜单；名称留空时填入页面标题，已有名称不覆盖。取消、关闭或 Escape 不改变草稿。弹窗支持键盘选择、Tab 循环、恢复触发按钮焦点、长列表滚动、空态，切换项目重置表单/弹窗。
- UI 生产构建与全量 236 项测试（16 文件，新增 [4 项页面链接测试](tests/menuPages.test.ts)）通过。[真实 Edge 回归](scripts/verify-menu-page-picker.mjs)在 1440×900 / 1024×640 验证筛选、搜索、首页/自定义页路由、取消、选择不提交、手动地址、键盘焦点、滚动与空态；写作预览双尺寸回归仍通过，无页面 JS 错误。IPC 使用隔离 fixture，未操作用户项目。测试发现并修正搜索初始焦点、Tab 边界、搜索框 Escape 默认清空和卸载时焦点恢复问题。
- 本轮只改桌面 UI，没有新增依赖或修改 Rust/网站模板；未重跑 Rust 全量/clippy/网站构建，仍保留既有 Vite 大包提示。写作预览及外观预览双尺寸/双设备/五档缩放回归通过。
- 已重建[桌面程序](target/release/veloct-desktop.exe)（17,164,216 字节）与[安装包](target/release/bundle/nsis/Veloct_1.0.0_x64-setup.exe)（29,819,096 字节，28.44 MiB）。打包退出 0；独立主程序打包后为 NotSigned，已补签。两份交付文件独立签名 Check 均 Valid（仅本机自签信任，不保证其他机器消除 SmartScreen），sidecar Check 通过；未安装或发布。
- 最终 SHA-256：应用 `3D02C67A28E7C14757A970FFC7D2A99C0A7990BF50E1B43BE1BA60B9707C2A29`；安装器 `0714CE967DCDDABED58893B48F7BE2B5C0BBA0B9841FA4DF4414573DCA21C606`。

## 写作工具栏稳定与 Markdown 即时预览（2026-10-16）

- 顶部保存提示原来从「已同步」变成「有未保存的改动」时变宽，两个窗口尺寸下都复现预览按钮左移 44px。现在保存提示预留固定宽度，独立站点预览按钮同样固定宽度，路径在剩余空间内截断；已同步、未保存、保存中三态的工具栏和按钮坐标一致。
- 按本轮用户明确要求，编辑器右侧改为读取**当前未保存缓冲区**的 Markdown 即时预览，不再等待自动保存、IPC、Astro 热更新或文档路由。左侧 MD 模式、所见即所得、摘要编辑都会同步；右侧只读，切换文档不残留旧内容，保存冲突也保留当前预览。独立「页面预览」与外观预览继续使用真实 Astro，编辑区提供「打开站点预览」入口。
- 两侧复用 [Markdown 扩展](src/lib/markdownExtensions.ts)，没有新增运行时依赖。显式声明 Markdown 输入类型，修复外部正文更新未按 Markdown 解析、MD 切回富文本后显示旧正文的问题；禁用 StarterKit 自带的重复 Link。`setContent` 和 `setEditable` 的程序更新不发写入事件，避免仅打开文档就规范化正文并触发保存。Frontmatter 原始字段（含未知键与永久 ID）继续保留。
- 此处是**正文阅读视图，不是网站最终样式**：主题、站点自定义组件/指令及站点相对图片请在独立站点预览核对。相对图片显示占位，不误请求桌面应用源；不执行原始脚本，不接受危险链接，普通链接也不会把工作台导航走。
- 验证：`npm.cmd run build:ui`、`npm.cmd run test:ui -- --run`（232 tests / 15 files）、`npm.cmd run test:template` 通过。新增 [3 项语法测试](tests/markdownExtensions.test.ts) 与 [真实 Edge UI 回归](scripts/verify-editor-preview.mjs)：1440×900 / 1024×640 下验证三态几何稳定、未保存正文/表格/代码/富文本/摘要即时显示、打开文章零写入、无 Astro 依赖、冲突不丢缓冲区、文档切换和安全边界，无页面 JS 错误。既有预览双尺寸/双设备/五档缩放和原位删除/链接知识库 Edge 回归也通过。
- 首次全量 UI 回归仍要求旧的 DocumentPreview 接线，按新要求更新该断言；首次打包被新增测试的严格 TypeScript 类型检查拦下，已补可选索引与联合类型安全断言，重新 UI 构建和 232 项测试通过后再打包。仍保留既有 Vite 大包提示。本轮未改 Rust/网站模板，未重跑 Rust 全量测试、clippy 或网站生产构建，不沿用上轮结果声称本轮已通过。
- 已重建[桌面程序](target/release/veloct-desktop.exe)（17,163,192 字节）与[安装包](target/release/bundle/nsis/Veloct_1.0.0_x64-setup.exe)（29,824,816 字节，28.44 MiB）；打包退出 0，打包中签名应用/sidecar/安装器。独立 release 程序打包后仍为 NotSigned，已补签；最终两个交付文件独立 Check 均 Valid（仅本机自签信任，不保证其他机器消除 SmartScreen）。`build-sidecar.ps1 -Check` 通过；未运行安装器、未发布。
- 最终 SHA-256：应用 `53A4F5BF2D33EF43A339316836667C794B65304753309EF718287B9716EAEA1A`；安装器 `B3C57A0F2BA2BE2B2B028EB07D1EE3490456DDE45FB5CF12589FF50D0F0DD39B`。

## 新建文章预览与 Windows 任务栏图标修复（2026-10-15）

- 新文章默认是未公开草稿，原 WIP 页面只生成已公开 WIP 的路由，导致草稿在开发预览中持续 404。现已允许开发预览生成私有草稿的永久 ID 地址；生产构建仍排除私有草稿，不因修复而公开内容。
- Astro 热加载识别新文件存在短暂延迟（真实隔离验证曾连续四次 404 后返回 200）。编辑器改为检查当前文档地址就绪后再挂载 iframe，不再把临时 404 当最终结果；切换文档/项目取消旧等待，超时或命令异常显示错误及重试，不无限空转。后端只检查当前预览服务与已存文档路由，不接受任意网络地址。
- Windows 任务栏需要独立的 `ICON_BIG`。旧程序实测大图标缺失，小图标为 32×32；仅调用 Tauri `set_icon` 不能补齐大图标。现在小图标使用编译期嵌入的 PNG，大图标从应用 ICO 资源加载并显式设置，Windows 句柄边界限制在一个私有模块。不会清理 Explorer 缓存或修改用户的固定快捷方式。
- 旧官方 WIP 模板纳入备份后安全升级，自定义模板仍保留；打包模板已重新暂存。[真实 Astro 回归](scripts/verify-new-article-preview.mjs)验证已有/热创建草稿、永久 ID 不随 slug 改变、私有草稿不进入生产及公开 WIP 正常输出。[浏览器回归](scripts/verify-document-readiness.mjs)验证等待时无 iframe、正确路由、切换取消及错误重试，无 JS 错误。
- 本轮重新验证：UI 229 tests / 14 files（新增 6 项等待/取消/超时测试），Rust workspace 1556 passed / 0 failed / 16 ignored（47 目标），UI 构建、模板测试、workspace clippy、站点 21 页及 52/52 产物检查通过。保留既有 site_settings clippy、Windows linker 和 Vite 大包提示。
- 新 release 程序已实际启动并用原生探针验证：`ICON_BIG` 为 64×64（2897 个非透明像素、2655 个彩色像素），`ICON_SMALL` 为 32×32（837 / 762），不再缺失或空白；使用独立 WebView2 配置，测试窗口已正常关闭，未运行安装器。该验证不代表清除了旧的固定快捷方式缓存。
- 已重建[桌面程序](target/release/veloct-desktop.exe)（17,163,192 字节）与[安装包](target/release/bundle/nsis/Veloct_1.0.0_x64-setup.exe)（29,817,016 字节，28.44 MiB）。打包中签名，独立程序补签后两者 Check 均 Valid（本机自签信任，不代表其他机器消除 SmartScreen）；sidecar 与全新构建匹配，未安装或发布。串联签名脚本首次因调用方读取陈旧的 LASTEXITCODE 误报失败，签名本身已成功；改用 PowerShell 成功状态并独立重查后全部通过。
- 最终 SHA-256：应用 `246BD3A747BEFC5255236839149A49722B1424CCCDF2EA9A2A600CCAA302753C`；安装器 `2E4B3E2795B624FFADBEC9C95139C7FFC919FBAA5D9D93A317B29A00EC4FA8D0`。

## 预览工具栏、原位删除确认与链接知识库（2026-10-15）

- Astro 预览启动前由 sidecar 写入项目级工具栏关闭偏好，保留其他偏好，不改用户模板。旧项目即使配置中明确开启工具栏也不再显示；干净模板同样默认关闭。已用真正的打包 sidecar、Astro dev 和 Edge 在隔离旧项目验证，不只是 CSS 隐藏。
- 文章、页面、知识库的删除按钮在原位置切换为「确认删除」，带 160ms 切换动画及减少动态效果适配；取消或 Esc 恢复，不请求删除。确认失败的错误就近保留，不再使用顶部确认条。
- 「知识库管理」新增「链接知识库」：支持语雀知识库、飞书 wiki 根节点，获取分组与子目录，工作区及网站知识库页都只显示大纲，点击打开源文档；不抓取正文，不混入本地 Markdown 或引用索引。
- 语雀使用个人 Token；飞书使用企业自建应用 App ID / Secret，需要知识库只读权限及目标空间查看权限。授权只进入 OS 凭据库，不写入目录、网站或备份；恢复到其他设备后重新授权。目录标题和链接在发布后可见，私有源文档仍需访问者自行取得源站权限。
- 目录快照位于 `data/linked-knowledge.json`；支持更新目录、修改名称/授权、解除链接。失败保留旧快照，外部编辑冲突拒绝覆盖，畸形数据报错而非清空。固定官方 API、禁止 HTTP 重定向、限制目录深度/节点数并校验源链接；解除链接不删除源站或本地内容。
- 站点支持目录快照变化时刷新；旧官方知识库入口及新增组件纳入安全模板升级，自定义文件保留并记录警告。站点模板已重新暂存。顺带将失配的 `test:template` 命令改为执行其实际使用的 Vitest 测试。
- 验证：UI 223 项（13 文件），Rust workspace 1556 passed / 0 failed / 16 ignored（47 目标），站点构建 21 页及 52/52 产物检查通过，模板测试、UI 构建和 workspace clippy 通过。Edge 隔离 IPC 验证原位几何位置、动画、二次确认、Esc、分层目录、源站跳转接线、失败保留及解除链接；无页面 JS 错误。首次全量 Rust 测试中既有进程树取消测试出现一次时序失败，单测与第二次全量重跑均通过，未修改其生产逻辑。
- 边界：目录 API 使用隔离 HTTP fixtures 验证，未使用真实用户语雀/飞书凭据连接线上知识库。保留既有 site_settings 的 clippy 提示及 Vite 大包提示。
- 已重建[桌面程序](target/release/veloct-desktop.exe)（17,131,448 字节）和[安装包](target/release/bundle/nsis/Veloct_1.0.0_x64-setup.exe)（29,811,232 字节，28.43 MiB）。打包时签名应用、sidecar 和安装器；打包后独立 release 应用为 NotSigned，已补签。最终两者签名均 Valid（本机自签信任，不代表其他机器消除 SmartScreen），sidecar 提交副本与新构建匹配；未安装或发布。
- 最终 SHA-256：应用 `D5D2FC6886FEC7C2DFFCDE84BB35DBA5F11C38FA579DB42289DD79ED0A9DA0C5`；安装器 `C3446B224DF421054B4E8E5E808BB52E7CC4ADA7381436231D5E9E001A1B6F15`。

## 外观预览稳定、附件管理与设置整合（2026-10-15）

- 修复预览 iframe 的未缩放布局盒与容器尺寸相互影响造成的反馈：预览视口独立定位、尺寸隔离，缩放占位裁切原始盒，重复尺寸不再触发状态更新。Edge 在 1440×900 和 1024×640、桌面/手机及 50%～150% 五档缩放下均稳定。
- 工作区新增「附件管理」入口，集中管理缓存图片、普通文件和已上传对象；普通文件的 Markdown 使用链接，不再错误生成图片语法。
- 「存储目标」配置与多图床排序只在「设置 → 附件存储」展示；附件管理只保留实际上传所需的目标选择，不再重复配置表单。
- 多图床按勾选顺序逐个同步；首次使用排序最靠前的成功地址，其他地址只作备份。已有主地址不因重排、补传、检测失败而改变，也不会自动改写 Markdown。失败目标不阻断后续目标，成功对象重试时复用；每个目标结果逐次持久化。
- 自动在打开项目及可见窗口每五分钟检测已上传图片，也可在附件管理手动检测。失效/连接异常和备份上传错误显示在附件管理消息区、每行消息泡及侧栏计数；404/410 与网络错误分开提示，后者不宣称文件已删除。不对相对本地地址发网络请求。
- 网络图片的来源密钥、搜索和插入统一放到「设置 → 网络图片」，删除独立顶级路由。网络来源与附件图床用途不同：前者使用第三方远程链接且不进入附件缓存，后者管理自己的上传文件，数据逻辑不混用。
- AI 设置直接内联展示提供商、模型与功能分配，不再按钮弹窗。
- 验证：UI 217 项（12 文件）、Rust workspace 1546 passed / 0 failed / 16 ignored（47 目标），新增同步/失效相关 Rust 10 项和 UI 9 项；UI 生产构建、workspace clippy 通过（保留既有 site_settings 测试的 field_reassign_with_default 警告及 Vite 大包提示）。实际构建 UI 的 Edge 隔离 IPC 回归验证设置分离、排序保存、同步命令接线、失效泡、网络图片和 AI 内联，无页面 JS 错误；真实上传适配器沿用原实现，未使用用户图床凭据进行线上测试。
- 已重建 `target/release/veloct-desktop.exe`（16,952,248 字节）与 `target/release/bundle/nsis/Veloct_1.0.0_x64-setup.exe`（29,724,368 字节，28.35 MiB）。打包时完成应用/sidecar/安装器签名；打包后的独立 release 应用文件实测曾为 NotSigned，已补签独立文件，最终 `sign-artifacts.ps1 -Installer -Check` 两者均为 Valid（本机自签信任，不代表其他机器消除 SmartScreen）。`build-sidecar.ps1 -Check` 通过，提交副本与新构建一致；未运行安装器、未发布。
- 最终 SHA-256：应用 `1D99EF9517EA6F0EE75C303D804745BD92DCAFDF40C9ECC3933AE50987D24BE8`；安装器 `E2DDE461F12786487BEA456EB25BC2C47546CB86AFA3310E4F8A7F27120AA868`。

## 内容管理删除入口修复

- 后端 `delete_document` 已存在，但内容管理界面漏接删除操作；文章、页面和知识库列表现已添加独立删除按钮及含标题的行内确认。
- 取消不调用删除；请求期间禁止重复提交；失败保留内容并显示错误。成功清理对应编辑状态并刷新内容列表、仪表盘和预览。
- UI 构建、208 项测试及差异检查通过；Edge 使用隔离 IPC 数据验证三类内容的取消、失败保留、成功删除及仪表盘更新，无页面 JS 错误。未删除用户项目中的真实内容。
- 现有安装包尚未包含本轮删除入口修复。

## 导航路径与 AI 建页路由修复

- 修复“页面管理 / 首页”误显示 AI 页面创建的问题：AI 建页现在使用独立 `page-creator` 视图，文档编辑继续使用 `editor` 视图；切换页面、普通新建、编辑首页后不会残留 AI 创建状态。
- 顶部路径统一来自视图元数据，补齐设置、附件、网络图片、作品和 AI 建页等入口，并显示外观/设置的子分类；预览编辑器会按当前文档路径打开。
- 删除无入口的重复顶级部署路由，保留设置中的部署目标功能；作品管理入口恢复，附件和网络图片入口继续保留。
- 全导航 Edge 回归通过：检查 13 个顶级入口、6 个设置子页、AI 后打开首页/文章/知识库、首页摘要入口和资源路径；无页面 JS 错误。
- UI 构建及 208 项测试通过。最新路由修复版安装包已重新生成，主程序和安装包签名均为 Valid；主程序 16,826,808 字节，安装包 29,696,416 字节。主程序 SHA-256：BF9EEFD289FD2F57B46DB11C082AAB1C847761A652BE510DEE3DDAA86687EFDD；安装包 SHA-256：FE73B6F1B3B12C6B736F4EB7D8AD44B798CE06BB007CE49CD6AC7007FABBEA19。

## 站点信息预览与路径显示回归修复

- 修复顶部路径 JSX 中的字面 `\n`，现在显示为 `blog / 外观`；路径按钮继续支持返回仪表盘与对应管理栏目。
- 站点信息采用左侧表单、右侧站点预览的双栏布局，表单独立滚动；进入站点信息自动启动预览，窄窗口改为纵向排列并保留预览高度。
- Edge 浏览器以隔离 IPC 数据和实际构建产物验证：1440×900 下预览约 653×591，自动启动仅调用一次，首页标题草稿实时同步，项目路径返回仪表盘；800×900 下预览高度 560px，无页面 JS 错误。
- UI 构建及 205 项测试通过。修正版安装包已重新生成，主程序和安装包签名均为 Valid；主程序 16,826,808 字节，安装包 29,703,472 字节。主程序 SHA-256：DCDC17879A8433D40F5469F8AF2C544ACE74679D7A38AD343E8057DCED2EF40B；安装包 SHA-256：BC2EF09D156E642DFB4D6B47DA62B0674FCAB5DA869FC2EDD46CFE524ABE09F7。

## 首页文字、页脚贴底与备案链接

- 站点信息新增“编辑首页摘要与正文”入口，复用首页页面文件；短欢迎语是摘要，长欢迎语是正文，编辑器提供摘要输入而不暴露永久 ID。
- 网站主内容改为最小一屏高度的纵向布局；短页面页脚贴底，长页面随正文排列，不使用 fixed 覆盖内容。
- ICP、公安及萌备案分别可编辑跳转地址，保存进 filing 的 icpUrl/policeUrl/moeUrl；空 ICP/公安链接沿用官方查询入口，空萌备案链接显示文字。
- 链接只接受有效 http/https，不允许脚本协议、带用户名密码或空白的地址；未知配置继续保留。
- 最新本轮安装包重新生成并签名校验为 Valid；主程序和安装包都已补签。主程序 16,826,296 字节，安装包 29,703,928 字节。主程序 SHA-256：880FB8C17769839F1090110EBBFC32B660DC3D902E9F7F3B13D2D26551226F20；安装包 SHA-256：E9DC64B4B0C2840DF1BDD35438D1B04A482E8C0A5902C167DB638967E18D0B62。
- Edge 验证 1440x900 与 390x844 短页贴底、长页不遮正文；独立测试配置的三类备案链接实际渲染正确。

## 上轮修正：恢复 App 动效与预览转场

- 上一轮删除 App 上浮是不正确的，现已恢复独立的 260ms 上浮；结束后 transform 为 none，不使用 both 持久保留变换。
- 网页改用 Astro ClientRouter 同文档转场，解决 iframe 预览不能依赖纯跨文档 CSS 转场的问题。编辑桥、侧栏、图片懒加载及知识库交互适配换页生命周期。
- 补充官方布局/CSS等模板安全升级，只匹配已知官方完整正文或 SHA-256；自定义模板不覆盖。补齐社交图标模块，避免升级后缺少依赖。
- Edge 浏览器实际验证：1440x900 和 390x844 下 rise、dissolve、none 分别触发对应动画，均无页面 JS 错误；Astro dev 及内嵌 iframe 均捕获到 veloct-rise-in。
- 198 项 UI 测试、165 项 Rust 测试通过，网站构建 21 页及 52/52 产物检查通过；知识引用连续导航验证通过。
- 最终 build-installer.ps1 退出 0，独立程序补签后主程序/安装包均 Valid。主程序 16,790,968 字节，安装包 29,666,880 字节。SHA-256：76FA97F8F97D56D73C48EC730D167CB3BACE0083773346E58609D0D670C05FD3。
- 测试时先退出旧 Veloct，用新安装包更新后重新打开项目；官方模板自动升级，自定义模板仍保留。

## 页面切换动效上一轮实现（历史，已被上方修正）

**原因不是样式写错，而是作用对象错了。**

- 旧实现把动效加在**桌面应用自己的内容列**上（`.content[data-page-transition] > .page`），设置值存在**应用的 localStorage**（`veloct.project-appearance:<项目路径>`）里。
- 网站是 Astro 构建出来的独立页面，读者浏览器里的那份文档**读不到应用 localStorage**。所以「在预览里翻页没有任何变化」不是偶发缺陷，而是结构上不可能成立。
- 覆盖面本来也不全：只有根节点类名恰好是 `.page` 的视图才会命中，仪表盘和文章/页面/知识库管理的根节点不是 `.page`，所以在应用内切换也基本看不到效果。
- 顺带一个真实隐患：`animation: … both` 会让 `.page` **永久保留** `transform: translateY(0)`。非 `none` 的 transform 会让元素成为包含块，于是页面内打开的 `position: fixed` 浮层（头像裁切框、AI 设置）会相对 `.page` 定位而不是视口。

**现在的实现**（设置含义只有一个：网站页面之间的切换效果）：

- 值改为**站点配置** `data/site.json` 的 `pageTransition`：Rust 侧新增 `PageTransition` 枚举（`rise` / `dissolve` / `none`），默认 `rise`；旧配置没有这个键时按默认值走，不会变成「无动画」。
- 网站用**跨文档视图转场**：`@view-transition { navigation: auto }` 加 `::view-transition-old/new(root)`，写在 `site/src/styles/global.css`。**故意不用 `<ClientRouter />`**：它会替换文档，而预览的内联编辑桥（`?veloct_editor=1`，在 `BaseLayout.astro` 里）依赖每个文档只跑一次；跨文档转场不需要任何 JavaScript。
- `<html data-page-transition="…">` 由 `BaseLayout.astro` 从配置写出；CSS 只认这个属性。
- 「无动画」是两个伪元素都 `animation: none`，立即切换——这是最容易被漏成浏览器默认淡入的一项，测试专门断言了它。
- `prefers-reduced-motion` 单独为伪元素补了一条规则：文件里原有的通配规则匹配元素、匹配不到伪元素。
- 预览里的 Astro 开发工具条已关闭（`devToolbar: { enabled: false }`）：它只存在于 `astro dev`、不存在于生产，既违背「预览要能预测生产」，也会被一起拿去做转场。
- 应用侧不再给内容列加动效；一个设置只表达一件事。
- 删除：`src/SiteBeautifyPanel.tsx`、`src/lib/projectAppearance.ts`（以及 `src/styles.css` 里对应的 `page-rise-in` / `page-dissolve-in` 与属性规则）。

**浏览器要求**：跨文档视图转场自 Chrome 126 起支持；本机 WebView2 实测版本是 **154.0.4258.53**，支持。更老的引擎会忽略该规则并正常跳转，不会坏掉。

**实测过，不是推断**：把 `data/site.json` 的 `pageTransition` 临时改成 `dissolve` 重新构建，`site/dist/site/index.html` 与 `site/dist/site/article/index.html` 的根标签都变成 `data-page-transition="dissolve"`；恢复原配置后回到 `rise`（配置文件按字节还原，`git status` 无改动）。

## 外观结构调整：备案管理并入

- 外观现在有三个分类：**站点信息 / 站点美化 / 备案管理**；备案管理不再是「设置 → 站点设置」的子标签（那里只剩文章设置和 SEO 设置）。
- 三个分类都通过同一个 `SiteSettingsPanel` 的 `onlySection` 渲染，共用一份设置状态和一个保存按钮，所以两个分类不会各持一份会互相覆盖的草稿。
- 「站点美化」不再有自己的保存按钮：它现在是设置表单的一节，和文章设置一样由「保存设置」提交。
- 涉及：`src/App.tsx`、`src/SiteSettingsPanel.tsx`、`src/styles.css`。


## 自定义模板提示、联系方式图标、头像上传裁切

- 站点信息和页面管理不再显示“已保留你的自定义站点模板 …”提示条。
- 自定义模板本身**仍然保留、不会被覆盖**；`ProjectService::upgrade_builtin_templates` 依旧把跳过的文件写入运行日志（`veloct_logging::Category::App`），所以信息没有丢，只是不再占据页面。
- 涉及：`src/App.tsx`。

### 联系方式预置常用平台图标

- “站点信息”的联系方式区新增平台目录：**国内** 12 个（微信、QQ、微博、哔哩哔哩、知乎、抖音、小红书、豆瓣、掘金、CSDN、Gitee、网易云音乐），**国外** 14 个（GitHub、X、Facebook、Instagram、YouTube、LinkedIn、Telegram、Discord、Reddit、Mastodon、Bluesky、TikTok、Pinterest、RSS）。
- 点平台按钮即添加一条联系方式，并预设名称与链接占位；已有条目可以随时改平台，**用户自己写过的名称不会被覆盖**。
- 侧边栏按 `icon` 键绘制图标；没有图标的条目（如手写的“邮箱”）仍然按文字显示，不会被丢掉。
- 图标来自 [Simple Icons](https://simpleicons.org)（CC0-1.0），路径按官方 SVG 的 `d` 逐字嵌入。**抖音没有官方图标**（CDN 返回 404），目录保留该键但不借用别的产品标志，桌面端和站点端都改画它的首字“抖”。
- 站点端和桌面端读**同一份** `site/src/lib/social-platforms.ts`，因此不存在两份图标清单互相不一致的问题；`tests/socialPlatforms.test.mjs` 断言这一共享关系、路径完整性以及无图标回退分支不是死代码。
- 涉及：`site/src/lib/social-platforms.ts`（新）、`site/src/components/SocialIcon.astro`（新）、`site/src/components/LeftSidebar.astro`、`src/SiteSettingsPanel.tsx`、`src/styles.css`、`tests/socialPlatforms.test.mjs`（新）。

### 首页头像上传并裁切

- 头像字段旁新增“上传并裁切”：选择图片后在窗口内裁成正方形（拖动定位、滑块缩放），确认后由新的 `save_site_avatar` 命令写入项目自己的 `site/public/assets/avatar.png`，字段自动填 `/assets/avatar.png`。
- 图片存进**项目**而不是应用数据目录，所以头像会进入备份和发布产物；`site/public/` 是 Astro 的静态目录，预览和正式站点都能显示。
- 服务端不信任窗口：只接受 PNG 签名、拒绝超过 4 MiB、拒绝项目里没有站点模板、拒绝经由符号链接写出项目外（5 项新测试覆盖，含“存进去的字节必须与送来的字节相同”）。
- 裁切画布固定输出 512×512 PNG；源图上限 12 MiB。
- 涉及：`crates/veloct-app/src/site_settings.rs`、`src-tauri/src/commands.rs`、`src-tauri/src/lib.rs`、`src-tauri/Cargo.toml`（新增 `base64`）、`src/lib/api.ts`、`src/SiteSettingsPanel.tsx`、`src/AvatarCropDialog.tsx`（新）、`src/styles.css`。


## 外观入口整合

- “其他选项”新增“外观”（当时包含“站点信息”和“站点美化”两个内部分类，现已是三个，见上文）；移除原来独立的站点信息、站点美化导航项。
- 保留站点信息编辑、保存及真实站点预览；预览使用状态跟随外观分类，站点美化复用外观页布局。

## 站点信息样式修复

- 删除桌面配置表单的额外蓝色边框、浅蓝色悬停背景、悬停光圈和蓝色聚焦描边，恢复与其他配置项一致的公共样式。
- 修复位置：`src/styles.css`；站点信息编辑和保存逻辑保持原有行为。
- 验证：`npm.cmd run build:ui` 退出 0，TypeScript 检查与 Vite 构建通过；仍有 bundle 大于 500 kB 的提示。
- 后续已更新 `target/release/`：`build-sidecar.ps1 -Check` 与 `build-installer.ps1` 均退出 0，生成 `bundle/nsis/Veloct_1.0.0_x64-setup.exe`（29,627,576 字节，28.26 MiB）。初次签名检查退出 1：安装包 Valid、独立主程序 NotSigned；执行 `sign-artifacts.ps1` 补签主程序后，`sign-artifacts.ps1 -Installer -Check` 退出 0，两者均 Valid。安装包 SHA-256：`8FB5415FE44AB0BA6703EBB88468FAAABC7B08B61C7CFCEBD18ED0FDA551B277`。本次未进行安装和桌面视觉实测。

## 最近用户要求

### AI 建页改为右侧配置页

“AI 建页”不再通过弹窗打开。点击导航后切换到普通主内容区，继续提供描述、生成、预览、确认创建和 AI 设置入口。

涉及：`src/App.tsx`、`src/PageCreator.tsx`、`src/styles.css`。

### 站点信息统一配置样式

站点信息保留正常配置/编辑能力，但取消预览中的蓝色描边和浅蓝色悬停高亮，不再使用类似浏览器 F12 的视觉效果。

涉及：`site/src/layouts/BaseLayout.astro` 及站点信息相关 UI。

### 热力图范围

热力图现在按文章、知识库和草稿/WIP 的日期统计，不统计普通页面。首页和文章列表页的 Astro 数据源均已调整。

涉及：`site/src/pages/index.astro`、`site/src/pages/article/index.astro`。

## 同一轮已保留的相关功能

- 站点美化提供页面切换动效：上浮、溶解、无动画。
- 应用导航只保留菜单管理入口，不另行展示网站菜单列表。
- 站点模板支持编辑相关字段的桥接协议，但当前视觉要求以“站点信息不高亮”为准。

## 本轮验证

| 命令 | 结果 |
|---|---|
| `npm.cmd run build:ui` | 通过（退出 0）；TypeScript 检查和 Vite 生产构建通过，125 个模块。Vite 仍提示 bundle 大于 500 kB。 |
| `npm.cmd run test:ui -- --run` | 通过（退出 0）；8 个测试文件、192 项通过（新增 7 项平台目录契约测试）。 |
| `powershell -ExecutionPolicy Bypass -File scripts/cargo.ps1 check --workspace --all-targets` | 通过（退出 0）。 |
| `powershell -ExecutionPolicy Bypass -File scripts/cargo.ps1 test -p veloct-app site_settings -- --nocapture` | 通过（退出 0）；12 项通过，其中 5 项为本轮新增的头像写入测试。 |
| 在 `site/` 执行 `npm.cmd run build` | 通过（退出 0）；Astro 构建 21 个页面。 |
| `node scripts/prepare-template.mjs` | 通过；`SocialIcon.astro` 与 `social-platforms.ts` 已进入 `src-tauri/resources/project-template`。 |
| `powershell -ExecutionPolicy Bypass -File scripts/cargo.ps1 test --workspace --no-fail-fast` | 通过（退出 0）；47 个测试目标，**1529 passed / 0 failed / 16 ignored**。 |
| `powershell -ExecutionPolicy Bypass -File scripts/cargo.ps1 test -p veloct-app site_settings` | 通过（退出 0）；14 项通过（含 2 项新增的 `pageTransition` 持久化测试）。 |
| `npm.cmd run test:ui -- --run` | 通过（退出 0）；9 个测试文件、**198 项**通过（新增 `tests/siteTransition.test.mjs` 6 项）。 |
| 在 `site/` 执行 `npm.cmd run build`（配置默认） | 通过（退出 0）；21 个页面，根标签为 `data-page-transition="rise"`。 |
| 同上，配置临时改为 `dissolve` | 通过；`dist/site/index.html` 与 `dist/site/article/index.html` 的根标签都变成 `data-page-transition="dissolve"`。配置随后按字节还原（1007 字节，`git status -- data/` 无改动），产物重建回 `rise`。 |
| `powershell -ExecutionPolicy Bypass -File scripts/build-installer.ps1` | 见下方“本包产物”。 |
| `powershell -ExecutionPolicy Bypass -File scripts/build-sidecar.ps1 -Check` | 通过（退出 0）；打包过程在提交的启动器上签名后已自动还原。 |
| `powershell -ExecutionPolicy Bypass -File scripts/sign-artifacts.ps1 -Installer -Check` | 首次退出 1（安装包 Valid、独立主程序 `NotSigned`，与 D178 记录的硬链接行为一致）；补签主程序后退出 0，两者均 **Valid**。 |

构建过程中出现过一次失败，值得记录：首次调用 `build-installer.ps1` 时 `tauri-build` 以 `Os { code: 5, kind: PermissionDenied }` 失败，原因是用户正在运行旧的 `target/release/veloct-desktop.exe`（及其 `veloct-astro-sidecar.exe`、`veloct-node.exe` 子进程），Windows 不允许覆盖正在运行的可执行文件。关闭应用后重建成功。**下次构建前请先关掉正在运行的 Veloct。**

另有一个**采集**假象值得记下，避免下一位被误导：`cargo.ps1 test --workspace 2>&1 | Select-String …` 这条管道报告退出码 1，而同一命令直接重定向到文件时退出码为 **0**。原因与 D94 同类——PowerShell 会把原生命令写入 stderr 的进度行变成错误记录。**判断 cargo 是否失败要看真实退出码，不要看经过 `2>&1` 管道的那个。**

本轮**没有**在真实桌面上点过界面，也没有实际安装这个包。“上传并裁切”依赖 `<input type="file">` 在 WebView2 中打开系统选择框，这一点只做了实现与构建验证，**未在运行中的应用里实测**。

## 本包产物

| 文件 | 大小 | 说明 |
|---|---|---|
| `target/release/veloct-desktop.exe` | 16,790,968 字节 | 直接运行用；已补签，`Valid`，`CN=VellowChire Code Signing, O=Veloct` |
| `target/release/bundle/nsis/Veloct_1.0.0_x64-setup.exe` | 29,666,880 字节（28.29 MiB） | 安装包；`Valid` |

安装包 SHA-256：`76FA97F8F97D56D73C48EC730D167CB3BACE0083773346E58609D0D670C05FD3`

**请先关掉正在运行的 Veloct 再替换 `target/release/veloct-desktop.exe`**（正在运行的进程会锁住该文件，`tauri-build` 会以 `PermissionDenied` 失败）。

## 外观入口整合

## 测试重点

1. 打开“其他选项 → 外观”，确认三个分类：站点信息 / 站点美化 / 备案管理；确认「设置 → 站点设置」里不再有备案管理，只剩文章设置和 SEO 设置。
2. 在「站点美化」里选**上浮**，保存，然后到「页面预览」里点几篇文章的链接——页面之间应该有上浮动效；再选**溶解**保存，翻页变成淡入淡出；选**无动画**保存，翻页应当**立即**切换。
3. 站点信息：确认没有蓝框和悬停高亮，确认没有“已保留你的自定义站点模板”提示条（也看页面管理）。
4. 联系方式：点几个平台按钮（GitHub、QQ、微信、哔哩哔哩、抖音），抖音显示为“抖”；保存后到预览左侧栏确认图标。
5. 头像：点“上传并裁切”，拖动和缩放，确认后字段变成 `/assets/avatar.png`，保存后在预览里看到头像。

**注意**：动效只在**保存设置之后**生效，因为它写进站点配置、由网站读取；改选但没保存时预览不会变。

## 版本说明

安装包文件名仍是 `Veloct_1.0.0_x64-setup.exe`，因为版本发布需要业主明确决定。不要把本地测试包自动称为 1.0.1，也不要把当前未提交工作树当成可复现发布提交。这一轮新增了 `src-tauri/Cargo.toml` 的 `base64` 依赖（工作区已声明，版本已锁定，无需联网）。

问题清单见 [`ISSUE.md`](ISSUE.md)，正式契约见 [`handoff.md`](handoff.md)，当前接手步骤见 [`PROCESSING.md`](PROCESSING.md)。
