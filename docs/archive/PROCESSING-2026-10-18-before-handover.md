# 过程记录归档 — 2026-10-18 整理前快照

> 历史材料，不是当前开工指令。原过程正文和证据保留，仅增加本说明并将Markdown链接调整到归档所在目录；正文中的“当前/最新”依原轮次理解。接手先读 [NEXT_AI.md](<../../NEXT_AI.md>) 和精简后的 [PROCESSING.md](<../../PROCESSING.md>)。

> 面向下一位 AI 编码助手。本文记录当前产品边界、工作树状态、最近验证与接手步骤；它不是产品契约。

**权威顺序：** 用户最新明确要求 > `handoff.md` 锁定的产品行为 > `docs/spec/` 的架构、数据和测试定义 > 本文。冲突时先核实，不要自行改写锁定条款。

## 交接检查点（2026-10-16）

- **2026-10-18 最新：首页图片附件化。** 头像/头图先入附件池再上传一个首选云目标；同步首项优先，否则选可见云目标最高上传优先级，失效不静默换目标。失败保留池资源及旧地址；成功仅填入表单，显式保存后才写站点设置并更新站点图片引用，重开恢复引用，换图不删文章引用/远程对象。UI271/21files、首页图片/五项界面/资源栏生产Edge双宽度、最终应用204+1ignored与desktop155及clippy通过。完整workspace1575/1failed/17ignored，失败仅既有安装取消进程测试且单项复核失败；不改该独立问题。最终构建退出0（release5分47秒），独立补签后EXE/NSIS均Valid、sidecar一致；最终见[NEXT_AI](<../../NEXT_AI.md>)和[UPDATE](<../../UPDATE.md>)，下面五项界面包/全量通过是历史。
- **下一位 AI 先读 [NEXT_AI.md](<../../NEXT_AI.md>)**：这是精简接手入口，包含当前最终形态、代码入口、证据边界和测试包。**2026-10-18 前轮五项界面：** 最新五项界面修改已通过UI271/21files、workspace1569/0failed/17ignored/47targets、clippy及五套生产bundle双宽度浏览器回归；附件选择/预览、5秒通知、上一页和移除AI建页已完成，**用户关闭后最终构建/补签完成，EXE/NSIS均Valid，sidecar检查通过；不重复已完成工作**。上轮为 S3/R2 策略配置：协议/访问风格、目录/命名、条件写防覆盖、CDN/后缀/缩略图、上传隐藏与排序，以及只读草稿验证；旧配置兼容且不改既有正文链接。UI267/20files、S3双尺寸Edge、完整串行workspace1595 passed/0 failed/17 ignored及clippy通过。没有真实桶或CDN联调；EdgeOne仅显式部署、本文资源栏和GitHub代理修复保留。最新测试包与保留历史失败见 [UPDATE](<../../UPDATE.md>)，不要仍把旧轮次当最新，不自动扩展修复或发布。
- 应用仓库 `D:\Veloct-Blog`，分支 `master`，HEAD `c97ad9e`；大量已修改/未跟踪功能尚未提交，交接依赖完整当前工作目录，不是只有 HEAD。此次整理没有提交、推送、安装或部署。
- 用户实际博客是 **`C:\Users\vellow\Desktop\blog`**，与应用仓库分开。本机位置只用于获授权的实际项目检查，不写死进产品代码。当前三份官方组件/CSS 与源码及 staged 模板已核验一致；8 个用户内容/配置/资源文件哈希保留，备份在该项目 `.veloct/template-backups/<sha256>/site/src/`，更早手动升级原件在 `.veloct/template-backups/profile-home-manual-20261016-164136/`。不得自动覆盖新出现的自定义文件。
- 当前首页是**单张近365天、标题旁切换按钮、无底部按钮行**，不是历史的双图/90天/右下按钮；联系人只显示有效填充项。写作是单栏 WYSIWYG/MD，旧右预览不再挂载。具体已覆盖条目见测试矩阵 D220–D221。
- 本轮按既有要求重建测试程序与 NSIS；最终 SHA/字节与签名状态见 [NEXT_AI.md](<../../NEXT_AI.md#6-最终交付物不是已发布版本>) 和 [UPDATE.md](<../../UPDATE.md>)，不要使用下方历史包哈希。它们是本地自签测试包，不代表本轮已安装/发布。
- **以下“最近验证”保留多轮历史，只有第一条是最近功能轮次**；旧的双图、右侧 Markdown、232/236 测试数、旧包哈希及早期装机结论不得当作当前状态。未重新验证的旧 ISSUE 条目也不能直接视为当前 P0。

## 快速阅读

1. `handoff.md`：正式产品合同。
2. `README.md`：仓库结构、开发和构建命令。
3. `ISSUE.md`：问题清单；历史条目按当前代码重新核实。
4. `docs/spec/00-architecture.md`、`01-data-model.md`、`02-test-matrix.md`、`03-upstream-facts.md`：实现决策、数据模型、测试证据和上游事实。
5. `UPDATE.md`：最近的用户可见改动和验证记录。
6. `docs/archive/PROCESSING-2026-10-03.md`：旧版长篇过程记录，仅供查历史，不代表当前状态。

## 当前状态

- 当前 HEAD：`c97ad9e`（`Bring the documentation back in line with the tree, and write the handover issue list`）。
- 工作树**不干净**，有大量已修改和未跟踪文件；不要清理、回滚、重置或覆盖无关改动。先运行 `git status --short` 和 `git diff --stat`。
- 最近安装包是本地测试包，不等于已发布版本。重新打包前必须重新验证并检查最终产物。
- 环境是 Windows，通用命令可用 Git Bash；Windows 构建脚本使用 PowerShell，通常需要 `-ExecutionPolicy Bypass`。UI 命令使用 `npm.cmd`；离线 pnpm 安装可能因缓存不完整而失败。

## 最近的产品要求

- **五项界面（2026-10-18，优先于旧AI条款）**：附件目标下拉；文件点击/拖拽/悬停或聚焦粘贴，预览后确认入池，不自动上传或改正文；右上五秒分类通知含进度/关闭/完整详情，结构性错误和重试入口保留；面包屑返回前页保留缓冲且切项目清历史；删除AI建页导航/设置/前端调用/桌面命令，普通页面保留，不清理旧用户配置和凭据。一次20文件、每个64MiB，内联位图预览8MiB上限，SVG/HTML不执行，不开放任意file://。
- **S3/R2 策略（2026-10-17）**：参照用户提供的Halo表单，端点和CDN各自选择协议并填写无协议主机；支持Path/Virtual Hosted、R2小写auto、目录占位符/命名/同名后缀、首匹配网址后缀与仅预览缩略图。新策略默认UUID+签名条件PUT避覆盖，旧策略无新字段时不迁移；命名和URL设置不重写旧正文/对象记录/备份。隐藏和高优先级排序仅影响上传下拉，不禁用已配置同步。验证对象存储只读ListObjectsV2，用当前草稿/输入或已存Secret，不写配置、不上传删除；成功不等于PUT权限或CDN支持，未经授权不拿示例端点真实联调。
- **EdgeOne 部署（2026-10-17）**：国内/国际都支持程序直接上传；只有用户点击部署才发请求，范围为全部已启用目标及当前发布构建 `dist/site` / `dist/works` 的所有公开产物，不是上传项目源码/缓存/密钥。主站/作品站使用不同的直接上传项目 ID，区域 Token 仅系统凭据库，UI不回显。发布/构建/保存配置不自动上传，不自动跨区域/创建项目/重传，等待对应云端 Success 才报告成功；1Panel仍手动导出。未经用户另行明确确认不进行真实云端部署，协议及fixture成功不代表真实账户可用。
- **每轮修复后更新测试产物**：用户要求每次修改完成都重建 `D:\Veloct-Blog\target\release` 中的桌面程序及安装包，检查签名后交付，方便立即测试；无需再次询问是否打包。**构建前先确认没有正在运行的 `veloct-desktop.exe`**：它会锁住 `target/release/` 下的程序、sidecar 和 node，`tauri-build` 会以 `PermissionDenied`（Windows 错误 5）失败；遇到该错误先问用户关闭应用，不要直接结束进程。
- **首页（最新要求，替代上一轮临时留白）**：按 osu! 个人主页主区域结构呈现头图、叠放头像、右侧名字/站点描述，中部单张近 365 天热力图（最新恢复原年度范围），标题旁的紧凑按钮切换博客/GitHub（不再占用底部行）；独立联系图标区每行最多 4 项、总计最多 16 项，空项自动隐藏、全空时整区隐藏，底部独立个人介绍。全站左导航/页脚保留，原右栏仅在非首页挂载。外观 → 站点信息配置头图上传、头像、名字、描述、个人介绍、联系方式/ID 和排序；设置 → 站点设置 → 首页与热力图配置两个数据源开关/GitHub 用户名及手动同步（只开一项不显示切换按钮）。最多新增 16 个联系方式，旧配置不截断，主页只显示前 16 个有效条目；ID-only 按钮复制，不再空白。首页不恢复旧 Hero/文章列表/首页 Markdown 渲染，原内容/设置保留。
- **页脚**短页贴底、长页跟随正文，不用 fixed 遮挡。
- **备案链接**各类备案号对应 icpUrl/policeUrl/moeUrl 可编辑，保存及模板校验 http/https；空 ICP/公安沿用官方入口，萌备案留空只显示文字。
- **自定义模板提示**只在运行日志里，不再作为页面提示条；自定义模板本身仍然保留、不会被覆盖。
- **联系方式**从预置平台目录添加（国内 12、国外 14），图标与站点共用 `site/src/lib/social-platforms.ts`；抖音无官方图标，两端都回退为首字。
- **首页头像/头图（2026-10-18覆盖旧本地写入）**：头像保留512×512裁切，二者经附件池/去重和首选目标上传，不再为新上传写固定本地文件；旧本地图片不迁移/删除。`save_site_avatar`/`save_site_cover` 返回远端地址，表单仍须显式保存。首选同步第一项（含显式隐藏）不可用不回退；无同步列表时才选可见云目标最高上传优先级。S3/R2无有效公开域名时入池后零网络上传；引用/清理保护包括已保存站点图，换图不丢文章引用。上传/保存工作线程捕获项目，导航晚到结果不污染新面板。
- **外观**位于“其他选项”，内部三个分类：**站点信息 / 站点美化 / 备案管理**；备案管理不再是「设置 → 站点设置」的子标签。
- **页面切换动效**描述网站页面之间切换，存入 `data/site.json` 的 `pageTransition`（rise/dissolve/none）。当前使用 Astro ClientRouter 同文档转场，适配内嵌 iframe；组件在 astro:page-load 初始化、before-swap 清理。改选后必须保存设置。
- **App 上浮独立保留**：内容区 260ms 上浮，结束后 transform:none，不使用 both 固定变换。上一轮把它删除是回归，已恢复。
- **旧项目模板升级**：仅官方完整正文/已知 SHA-256 匹配时更新布局、CSS和相关组件，备份后替换；自定义模板保留。
- **编辑区（最新要求，2026-10-17）**：一份所见即所得/MD 正文编辑器，文章右侧新增 240–300px「本文资源」小栏，以「附件池 / 图片池」标签切换，可收起恢复全宽；它不是第二份 Markdown/网站预览，仍不恢复拖动分隔条。新附件按稳定文章 ID 的 namespace 归属本文，旧资源按正文引用归属，其他项目资源须明确选用；添加/上传不改正文，插入走光标/选区与原自动保存。破坏性共享资源清理留在全局管理。本地未上传图显示占位，远程 HTTPS 图可显示缩略图。独立站点预览仍核对真实 Astro。顶部保存提示与站点按钮保留固定宽度；打开文章不应意外写入。
- **新建文章站点预览**仍支持默认私有草稿的永久 ID 地址，只在开发预览显示、不进入生产。上轮的文档就绪等待组件/后端检查保留，但最新写作区不再挂载该 iframe，不要恢复旧接线。
- **Windows 图标**分别保证小图标与任务栏 ICON_BIG，必须用新 release 实窗验证，不以 EXE 资源存在替代窗口验证。
- **Astro 工具栏**在所有应用预览中关闭，不仅新模板；不能用工具栏的存在代表生产输出。
- **内容删除**在原按钮位置动画切换确认，不能再移到列表顶部。
- **链接知识库**支持语雀/飞书目录快照，只展示大纲、点击源站，不复制正文；凭据不进入项目/网站/备份。网站会公开目录标题和源链接，私有文档访问仍受源站权限控制。
- **AI 建页**：2026-10-18按用户要求移除；旧“主内容区而非弹窗”条款仅历史，不得据此恢复入口。
- **站点信息**不使用蓝色描边或悬停高亮，视觉上与其他配置项统一。
- **热力图**：桌面统计范围含文章、知识库和草稿/WIP，不统计普通页面；公开首页博客图只统计已公开文章/WIP 和知识库，排除私有草稿元数据。GitHub 只显示经过验证的公开可见日历**活跃度等级**（不是贡献数量）；显式桌面同步、原子快照 `data/github-activity.json`、静态离线显示、失败保留前次结果/时间，换账号不复用旧数据，不自动刷新远程源。
- **站点美化**用于选择页面切换动效：上浮、溶解、无动画。
- 应用侧只保留“菜单管理”入口，不在应用导航栏另行呈现网站菜单列表。「新增菜单」使用“链接地址/页面”字段，右侧小按钮打开可搜索的页面链接选择弹窗；只填规范地址（首页 `/`），不自动新增；名称为空可填页面标题，取消不改草稿。仅列可生成的页面，文章/知识库不混入。

相关实现主要在 `src/App.tsx`、`src/AttachmentFilePicker.tsx`、`src/components/Notifications.tsx`、`src/lib/navigationHistory.ts`、`src/SiteSettingsPanel.tsx`、`src/SiteBeautifyPanel.tsx`、`src/NavigationPanel.tsx`、`site/src/layouts/BaseLayout.astro`、`site/src/pages/index.astro` 和 `site/src/pages/article/index.astro`。先读当前文件再编辑。

## 最近验证

2026-10-18 最新（首页图片）：UI271/21files及TS/Vite218modules，首页图片/五项界面/资源栏生产Edge1440/1024通过。完整workspace1575 passed/1 failed/17 ignored/47targets，仅既有安装取消进程测试失败且单项复核未过；最终相关防护更新后应用204+1ignored、desktop155与clippy0通过。测试为隔离存储/IPC，没有真实云账户上传、用户博客写入、安装或发布；最终构建退出0（release5分47秒），独立EXE补签后两项Check均Valid，sidecar一致；新包哈希见UPDATE/NEXT_AI。

2026-10-18 前轮（五项界面）：UI271/21files、TS/Vite218modules；五项界面/编辑器/资源栏/EdgeOne/S3生产bundle双宽度Edge通过，无JS错误。最终完整串行workspace1569 passed/0 failed/17 ignored/47targets，clippy0仅既有站点设置警告；数量下降主要因删除AI桌面功能及其测试，新添字节导入命令与缓存去重覆盖。原生事件和剪贴板为隔离fixture，非真实系统剪贴板/云端联调。打包预检发现桌面运行时没有结束用户进程；用户确认关闭后构建退出0（release3分19秒），独立EXE补签及两项Check均Valid，sidecar与fresh build一致，最终新包哈希见UPDATE/NEXT_AI；版本仍1.0.0，没有装机或发布。

2026-10-17 前轮（S3/R2策略）：UI267/20files、生产构建、真实Edge生产bundle隔离IPC的1440/1024表单检查通过；资源栏和EdgeOne双宽度追加复核也通过，无JS错误。最终完整串行workspace1595 passed/0 failed/17 ignored/47targets（attach278、app195+1ignored、desktop184），clippy0，仅既有site-settings警告。新增请求字段/测试类型造成的编译失败及去重夹具错误均修正后全量重跑，记录不删除；未真实桶/CDN测试，不写用户博客、不安装/发布。最终构建与签名、SHA见 [UPDATE](<../../UPDATE.md>) / [NEXT_AI](<../../NEXT_AI.md#6-最终交付物不是已发布版本>)。

2026-10-17 前轮（EdgeOne 显式直传）：按官方 Makers SDK 0.1.0 / contract0.1.39 实现两区域 Pages→COS临时凭据→ZIP→Production→状态轮询，仅明确部署动作上传，不安装CLI。全量发布根与全部启用目标，主站/作品项目隔离，部分失败不抹去成功结果；禁止redirect、敏感错误回显、源码/缓存/密钥误上传，Token仅OS凭据库。UI259/19files、TS/Vite、Edge1440/1024部署隔离fixture通过；最终完整串行workspace1580 passed/0 failed/17 ignored/47targets，clippy0，仅保留既有字段赋值警告。以前taskkill失败没有删历史，本轮两次串行通过且没有修改进程代码。未真实部署/写用户博客/安装/提交；最终签名包见 [UPDATE](<../../UPDATE.md>)。

2026-10-17 前一轮（文章本文资源栏）：文章编辑右侧新增可收起的附件/图片双标签小栏，按 namespace + 正文真实引用归属本文，复用全局附件/存储接口但不自动改写正文，支持多选导入、上传与重试、富文本/MD 光标插入及 HTTPS 网络图；破坏性删除仍在全局管理。资源栏真实 Edge 1440×900 / 1024×720 与旧写作回归 1440×900 / 1024×640 通过，UI254/18 files、TS/Vite 生产构建通过。新单测先修正 Markdown alt 转义断言，浏览器脚本修复定位/懒加载等待；人工截图另发现窄窗口标题挤压，修布局并加宽度断言后通过。无 Rust/网站模板变更，不重跑全 Rust；云上传为隔离 fixture，不是实际账号联调。未写用户博客、安装或发布，签名新包记录见 [UPDATE](<../../UPDATE.md>)。

2026-10-17 前一轮（GitHub 日历 HTTP 500）：根因是 reqwest 禁用默认功能后遗漏 `system-proxy`，生产客户端没有读取已启用的 Windows 系统代理。显式恢复后，同一真实账号的生产 Rust 请求解析 371 天；不写死代理、不改变系统设置或放松 TLS、不写用户项目。UI244/17files、应用192 passed/1 ignored、workspace clippy通过，UI生产构建通过（保留大包/既有clippy提示）。全workspace首次链接失败，关闭增量缓存后编译成功；运行阶段两个Windows taskkill测试失败，串行复核安装取消恢复通过但desktop库仍183 passed/1 failed，因此不能宣称全量通过。完整命令、失败和本轮新测试包见 [UPDATE](<../../UPDATE.md>)，本轮不安装或发布。网站/浏览器形态未改，不重复旧视觉验收。

2026-10-16 上一轮（切换按钮移至标题旁）：按钮现在是紧挨日历标题的28×26px图标按钮，完整可访问名称/目标提示保留；移除底部操作行，不再因按钮留下空白。365天、单图、联系方式和数据隐私不改。统计/日历以aria-hidden+inert不可见叠放预留最大尺寸，避免Tailwind重要[hidden]层规则让移动卡片切换时塌陷；不要换回会被重置折叠的hidden占位。补齐上一版三份完整官方基线/哈希和升级测试，实际项目三份匹配模板已先备份再更新，8个受保护文件哈希不变。UI构建、244tests/17files、模板测试、应用190tests/clippy、网站21页/64检查通过；主页Edge1440/1024/390、设置两尺寸、实际项目8页构建与两尺寸真实产物验证通过，无JS错误。首轮移动几何断言揭示上述隐藏规则冲突，修复后全尺寸通过；不是放宽断言。本轮未跑Rust全workspace。release编译4分53秒成功，首轮在临时NSIS文件签名后被用户中断（主程序新版，安装器仍旧时间戳）；应区分临时文件签名与最终完成。用户要求续做后只运行Tauri bundle续NSIS，不重复编译/已通过测试。续打包退出0，两项最终Check均Valid，本机自签信任不变；sidecar新构建匹配、三份staged模板逐字节一致。EXE17,266,624bytes SHA256 `64EA05E3D117D695DB9E9A8162CA2438D4C24E2D927C2F419AA121C1A472E682`；安装器29,883,352bytes SHA256 `DE09567588B961D75C3D1B4C2168F8BEDA29E5D9F768A17A4BB3E10FDEFAC6ED`，仍沿用原target/release路径。未安装或部署。完整主页同版脚本先前已通过；追加无障碍断言的最后重跑三尺寸均通过后命令120秒超时，未宣称该次剩余单/空来源构建完成。

2026-10-16 上一轮（恢复年度范围，单图切换不变）：用户要求回到原来的范围，但仍只要一张图和切换；现在默认连续近365天而非90天，UTC对齐/冻结发布时间/公开内容隐私不变，年度图恢复最小590px并在自身容器横向滚动，整体页面不溢出。切换、联系人隐藏与单/空来源状态不改。三份90天版完整官方哈希及逆向重建的精确基线测试已补，应用190tests/clippy、UI244/17files、模板与网站21页/64检查、主页三尺寸/设置两尺寸Edge通过；合并验证命令在主页完成后120秒超时，设置单独重跑通过，非代码失败。本轮无全workspace重跑。实际项目三份匹配模板已备份更新，8个受保护文件哈希不变，冻结时间8页构建及双尺寸真实产物验证通过。NSIS重建成功（release11分37秒），独立EXE打包后补签，EXE/安装器独立Check均Valid（仅本机自签信任），sidecar新构建一致；未安装或发布。最终EXE17,266,616bytes SHA256 `5763EC056F3A638921E780124B4AF72379FADAC2E68FB6DC1CF46F53C4958C79`；安装器29,862,584bytes SHA256 `41361EFB2C5301631E2A12DF03ED230C0A9F5D433CABDDAD776706D42A9B4EAC`，路径沿用 `target/release/veloct-desktop.exe` 与 `target/release/bundle/nsis/Veloct_1.0.0_x64-setup.exe`。

2026-10-16 上一轮（单图切换 / 空联系方式隐藏）：用户先提出双列竖图，随即取消该方案，改为一张近三个月图与右下角切换按钮；按最终要求实现，不能恢复双图/空占位。`ProfileHome.astro` 两面板同尺寸叠放但仅一张可见，按钮宽度固定，document 委托及根节点标记避免重复绑定，ClientRouter 返回已验证，切换无网络请求；`ActivityHeatmap.astro` 及 helper 裁剪 UTC 连续 90 天，生产沿用冻结时间、开发按当天。有效联系人为非空 ID 或有效 URL；纯空白不算，最多显示 16 项，不删配置。全空区完全移除且图表铺满剩余空间，单来源隐藏按钮、双关闭显示空态。已备份升级实际用户项目四份已知官方模板，8 个内容/配置/静态资源文件哈希未变；实际项目以显式冻结本轮时间构建 8 页、双尺寸真实产物 Edge 验证通过。UI244/17files、模板测试、应用190tests/clippy、网站21页/64检查通过；主页1440/1024/390、设置1440/1024真实 Edge 通过（含键盘/路由返回/90天/隐私/不请求网络），保留既有警告，本轮无全 workspace 重跑。新测试程序 17,266,104 字节 / SHA-256 `89CEF154BFD473FEE51428DC48D6382BE95CF650301682720DFF213A379C5625`，安装包 29,858,152 字节 / `81E7CC988E637ADB4A80F6DF76074AAEC3641AD87455E37B1F8864A957752941`，两者独立检查均 Valid（独立 EXE 打包后 NotSigned，已补签），本地自签信任边界不变，未安装/部署。sidecar Check、四份 staged 模板与源码字节比对通过，新 release 隔离实窗 ICON_BIG64×64 / ICON_SMALL32×32 有有效透明/彩色像素并正常退出。

2026-10-16 上一轮（真实旧项目兼容）：用户反馈首页未变后核对实际项目，日志确证旧首页/布局/配置读取器被当成自定义模板跳过，而不是缓存。用户明确同意备份后更新三份代码，原件留在该项目 `.veloct/template-backups/profile-home-manual-20261016-164136/`；8 个内容/数据配置/静态资源文件替换前后 SHA-256 不变。补入三份完整旧模板精确哈希和源基线，升级测试包含 LF/CRLF、备份、幂等、追加自定义代码仍保留。应用 190 tests/clippy 通过；实际项目网站 8 页构建及 `verify-existing-home.mjs` 双尺寸真实产物 Edge 验证通过，无旧右栏/横溢出/JS 错误，原个人信息、双日历、16 联系格正确。本轮只改模板识别与测试，不沿用上轮全 workspace 结果；既有空知识库/clippy 警告保留。UI 242 tests / 17 files、模板测试和 UI 构建亦通过。新签名程序 17,246,648 字节、安装包 29,842,280 字节，打包退出 0；独立程序补签后双文件 Check 均 Valid（本机自签），sidecar Check 通过，哈希见 [更新记录](UPDATE.md)。构建前无运行用户应用，未结束进程或安装/发布。对未知自定义模板仍不能自动强制覆盖；本轮手动替换有用户明确授权。

2026-10-16 上轮（个人主页）：双热力图、头图/头像/名字/描述、独立介绍和 4×4 联系图标区已完成，原首页右栏不挂载，外观/设置均已接通。UI 242 tests / 17 files、模板 staging/测试、网站 21 页/62 检查、Rust 全 workspace 1561 passed / 0 failed / 16 ignored / 47 targets 及 clippy 通过（既有默认字段赋值警告/Vite 大包提示保留）。主页 Edge 覆盖 1440×900、1024×640、390×844，设置覆盖两种桌面尺寸，验证生产开关、账号匹配、独立字段、上传/排序/上限、ID 复制、无横溢出/原右栏和错误提示。GitHub 使用 HTML/HTTP fixture，未承诺真实账号联网同步已验证。原位删除/知识库、菜单选择、单栏写作仍通过；预览缩放首轮 1024px/移动 125% 有单次不稳定，补样本诊断后不改生产代码重跑全矩阵通过，详情保留在 [更新记录](UPDATE.md)。官方升级覆盖上一轮留白首页基线、原始首页、布局及读取器，备份/自定义保护/幂等不变；未操作用户项目。打包退出 0，独立主程序补签后两份交付文件签名 Check 均 Valid（仅本机自签信任），sidecar Check 通过。程序 17,246,648 字节，安装器 29,849,408 字节（28.47 MiB）；应用 SHA-256 `F02CDE3CA6871692B79634FBDD93EEF1DCBD48BDE71150B8FB7661C21AAD000A`；安装器 `75FD169765F9DB678EC0450EEC9F0ACB325B59EC4579784A4B954E25E68D34A7`。未安装或发布。

2026-10-16 上一轮（首页留白，已被个人主页替代）：用户暂不修联系方式，只清空首页内容及移除首页右栏；导航、页脚、其他页右栏和用户内容/配置保留。[首页模板](site/src/pages/index.astro)不再读取/渲染首页正文或文章；[共享布局](site/src/layouts/BaseLayout.astro)提供默认开启的可选右栏，首页显式关闭并使用剩余宽度。新增官方旧首页/布局精确哈希，先备份再升级，自定义保留；新增 Rust 测试验证 LF/CRLF、幂等、备份、用户内容字节不变。网站 21 页/57 检查、UI 236 tests / 16 files、模板 staging/测试、veloct-app 全量 186 tests（项目定向 17）及应用 clippy 通过，未重跑整个 Rust workspace。[Edge 双尺寸回归](scripts/verify-empty-home.mjs)验证空主区、无首页右栏、导航页脚保留、切换其他页面右栏仍在及返回首页，无 JS 错误。首次打包因构建期间用户打开测试程序触发文件占用（`os error 32`）而退出 1，未杀用户进程；用户关闭后重建退出 0。独立程序补签后两份交付文件签名 Check 均 Valid（仅本机自签信任），sidecar Check 通过。程序 17,163,704 字节，安装包 29,820,784 字节（28.44 MiB）；应用 SHA-256 `C215D041B6CC880DCD08C79B0084558AF879413738D3195C2D46558C38DC4698`；安装器 `ACEBEB014F976770105263D773F1BE621F14D62EDD939746DE316B635966163A`。未安装或发布。详情见 [最近更新](UPDATE.md)。

上轮（单栏所见即所得）：按用户新要求移除重复的右侧 Markdown 阅读视图和分隔条，编辑器占满内容区；MD 切换、摘要、保存/冲突继续保留，真实站点预览仍为独立页面。UI 生产构建、236 tests / 16 files、[双尺寸 Edge 写作回归](scripts/verify-editor-preview.mjs)、菜单弹窗回归与外观双尺寸/双设备/五档缩放回归通过；隔离 IPC，不操作用户项目，无页面 JS 错误。没有新增依赖或修改 Rust/网站模板，未重跑 Rust 全量/clippy/网站构建。程序（17,163,192 字节）与安装器（29,807,968 字节，28.43 MiB）已重建；独立程序补签后两者独立签名 Check 均 Valid（仅本机自签信任），sidecar Check 通过，未安装或发布。应用 SHA-256 `AFFEC584DF93AA0FCDCCE2F453CAB1CB09996DB9C50BF8F013FB462150B85E7A`；安装器 `05203D583AD4B7AB655702C814BF6B8F2555B392CF7D0E229543CD90E66F5AE7`。详情见 [最近更新](UPDATE.md)。

上轮（菜单选择页面）：[菜单面板](src/NavigationPanel.tsx)增加“链接地址/页面”及选择弹窗，复用当前项目可生成页面的规范路由，支持搜索、键盘、取消、空态和长列表。新增 [4 项纯函数测试](tests/menuPages.test.ts)；UI 构建、全量 236 tests / 16 files、[菜单弹窗 Edge 回归](scripts/verify-menu-page-picker.mjs)及既有写作预览回归均通过，两个实际窗口尺寸，无页面 JS 错误。测试使用隔离 IPC，不操作用户项目。外观双尺寸/双设备/五档缩放回归也通过。没有新增依赖或修改 Rust/网站模板，未重跑 Rust 全量/clippy/网站构建。程序（17,164,216 字节）与安装器（29,819,096 字节，28.44 MiB）已重建；独立程序补签后，两者签名 Check 均 Valid（仅本机自签信任），sidecar Check 通过；未安装或发布。应用 SHA-256 `3D02C67A28E7C14757A970FFC7D2A99C0A7990BF50E1B43BE1BA60B9707C2A29`；安装器 `0714CE967DCDDABED58893B48F7BE2B5C0BBA0B9841FA4DF4414573DCA21C606`。详情见 [最近更新](UPDATE.md)。

上轮（2026-10-16）：写作区改为当前缓冲区的只读 Markdown 即时预览，独立站点预览仍保留；固定保存状态/站点按钮宽度，修复已同步→未保存时按钮左移 44px。显式 Markdown 解析、外部设置不触发更新事件，防止初次打开即保存或 MD/富文本切换显示旧内容。UI 232 tests / 15 files、UI 生产构建、模板测试通过。实际 Edge 在 1440×900 / 1024×640 验证状态三态几何不变、未保存内容/摘要/表格/代码即时显示、无 Astro 依赖、零打开写入、保存冲突、切换与安全渲染；既有外观缩放及删除/链接知识库回归通过，无页面 JS 错误。首次新增测试类型检查拦截打包，修正可选索引与类型断言后 UI 构建/全测及重新打包通过。独立主程序补签后，主程序（17,163,192 字节）和安装器（29,824,816 字节，28.44 MiB）均 Valid（仅本机自签信任）；sidecar Check 通过，未安装或发布。应用 SHA-256 `53A4F5BF2D33EF43A339316836667C794B65304753309EF718287B9716EAEA1A`；安装器 `B3C57A0F2BA2BE2B2B028EB07D1EE3490456DDE45FB5CF12589FF50D0F0DD39B`。未重跑 Rust 全量/clippy/站点构建；它们的以下数字仅为历史。更多记录见 [最近更新](UPDATE.md)。

上轮（2026-10-15）：新建文章默认私有 WIP，开发预览现在生成对应 ID 路由，生产仍不输出私有草稿；编辑器挂载 iframe 前检查当前文档就绪，切换文档取消旧等待，失败可重试。真实 Astro 热创建曾出现四次临时 404 再 200，已验证等待逻辑而非固定延时。Windows 任务栏单独设置 `ICON_BIG`，小图标使用编译期 PNG；旧程序原生探针确认大图标缺失，不能只靠 Tauri `set_icon`。Windows FFI 限于私有模块，其余桌面 crate 继续 deny unsafe。官方 WIP 模板升级仍先备份、保留自定义。

本轮重新验证：UI 229 tests / 14 files，Rust workspace 1556 passed / 0 failed / 16 ignored / 47 目标，UI 构建、模板测试、clippy、站点 21 页及 52/52 检查通过；真实 Astro 私有/公开草稿和稳定 ID 回归、实际 DocumentPreview 的 Edge 等待/切换/重试验证通过。原位删除/目录及双尺寸预览缩放回归也通过。新 release 原生探针确认 ICON_BIG=64×64（alpha=2897、chromatic=2655）、ICON_SMALL=32×32（837 / 762），不再缺失；旧程序实测 ICON_BIG=missing。独立 WebView2 配置防止读取真实项目历史，测试窗口已正常关闭。此验证不清理 Explorer 旧固定项缓存。

本轮产物：[桌面程序](target/release/veloct-desktop.exe) 17,163,192 字节，[安装包](target/release/bundle/nsis/Veloct_1.0.0_x64-setup.exe) 29,817,016 字节（28.44 MiB）；打包签名后独立程序补签，最终均 Valid（仅本机自签信任），sidecar Check 通过。应用 SHA-256 `246BD3A747BEFC5255236839149A49722B1424CCCDF2EA9A2A600CCAA302753C`；安装器 `2E4B3E2795B624FFADBEC9C95139C7FFC919FBAA5D9D93A317B29A00EC4FA8D0`。未安装或发布。串联签名调用方曾因陈旧 LASTEXITCODE 误报失败，签名已成功；改用成功状态并独立复查通过，不能把该误报写成生产签名失败。

上一轮（工具栏、删除、链接知识库）：Astro 预览由 sidecar 在启动前关闭项目级 devToolbar 偏好，兼容明确开启工具栏的旧模板且不修改其配置；真实 sidecar + Astro dev + Edge 隔离旧项目通过。删除按钮原位切换确认、160ms 动画、Esc 取消；链接知识库提供语雀/飞书分层大纲和默认浏览器源站跳转，工作区及网站知识库入口均支持，不抓正文或生成本地文档。目录在 `data/linked-knowledge.json`，OS 凭据独立，跨设备需重绑；刷新失败保留快照，冲突/损坏文件不覆盖。公共网站会展示目录标题和链接，源站私有权限不改变。官方知识库入口安全升级（备份/自定义保留），新组件已进入打包模板。

最新验证：UI 223 tests / 13 files；Rust workspace 1556 passed / 0 failed / 16 ignored / 47 目标；UI 构建、模板测试、站点 21 页构建与 52/52 产物检查、workspace clippy 通过。Edge 生产 UI 隔离 IPC 回归通过原位几何/动画/二次确认及目录层级/跳转/失败保留/解除链接，无 JS 错误。语雀/飞书 API 以隔离响应验证，未测试真实账户；既有 site_install 进程树取消测试首次全量曾时序失败，单独及完整重跑均通过。仅保留既有 site_settings clippy 和 Vite 大包提示。sidecar 已重建并校验与源一致。最新[桌面程序](target/release/veloct-desktop.exe) 17,131,448 字节、[安装包](target/release/bundle/nsis/Veloct_1.0.0_x64-setup.exe) 29,811,232 字节（28.43 MiB）；打包签名与独立程序补签后两者均为 Valid（仅本机自签信任），未安装或发布。最终 SHA-256：应用 `D5D2FC6886FEC7C2DFFCDE84BB35DBA5F11C38FA579DB42289DD79ED0A9DA0C5`，安装器 `C3446B224DF421054B4E8E5E808BB52E7CC4ADA7381436231D5E9E001A1B6F15`。

上一轮（附件与外观）：外观预览尺寸反馈已修复；工作区提供附件管理，存储目标与多图床排序只在设置附件存储中，网络图片配置/搜索合并到设置，AI 设置内联。同步首次按排序选首个成功 URL，之后永不自动切换；每个副本结果持久化。图片健康检测由 Rust 执行 HEAD，失效与网络异常用附件消息泡提示，不改写内容、不自动删除。配置在项目 `.veloct/attachment-sync.json`，检测/上传消息在 `.veloct/attachment-messages.json`，都通过原子写入保存，凭据仍只在 OS 凭据库。

本轮 UI 217 tests passed（12 files），Rust workspace 1546 passed / 0 failed / 16 ignored（47 目标），clippy 和 UI 构建通过。Edge 对生产 PreviewPanel 的双尺寸/双设备/五档缩放采样稳定；实际 UI 隔离 IPC 回归验证排序、同步命令、消息泡与设置整合。未用用户凭据测试真实线上图床。桌面 release 与 NSIS 安装包均已重建，最终应用与安装器签名 Check 均 Valid（仅本机自签信任）；独立 release 应用在打包结束后需要补签，已执行。sidecar Check 通过，完整产物大小与 SHA-256 见 UPDATE.md。未执行安装或发布。本轮不改站点模板。

本轮新增首页摘要编辑、备案独立跳转链接、贴底页脚；UI 205 tests passed（10 files），veloct-app 167 tests passed，网站 21 页构建与 52/52 verify-build 通过；Edge 验证短页贴底和长页不遮正文。最终安装包重建并签名 Valid，SHA-256 见 UPDATE.md。

上一轮（历史，仅验证产物属性，不代表预览动画实际生效）：

| 检查 | 结果 |
|---|---|
| `npm.cmd run build:ui` | 退出 0；123 个模块。 |
| `npm.cmd run test:ui -- --run` | 退出 0；9 个文件、198 项通过（新增 `tests/siteTransition.test.mjs` 6 项）。 |
| `scripts/cargo.ps1 test -p veloct-app site_settings` | 退出 0；14 项通过（含 2 项 `pageTransition` 持久化）。 |
| `site/` 内 `npm.cmd run build` | 退出 0；21 个页面。**并做过端到端验证**：把 `data/site.json` 临时改为 `pageTransition: "dissolve"` 重建，产出的 `index.html` 与 `article/index.html` 根标签都变成 `data-page-transition="dissolve"`；配置按字节还原（`git status -- data/` 无改动）。 |
| 安装包 | `Veloct_1.0.0_x64-setup.exe` 29,662,640 字节（28.29 MiB）；主程序补签后两者均 `Valid`。哈希见 `UPDATE.md`。 |
| Rust workspace 全量测试 / clippy | 本轮未重跑（上一轮为 1529 passed / 0 failed / 16 ignored）。 |

**转场的浏览器前提**：跨文档视图转场需 Chrome 126+；本机 WebView2 为 **154.0.4258.53**，支持。更老引擎会忽略该规则并正常跳转。预览里的 Astro 开发工具条已关闭，以免它被一起做转场、并让预览更接近生产。

上一轮（自定义模板提示、联系方式图标、头像上传裁切）：

| 检查 | 结果 |
|---|---|
| `npm.cmd run build:ui` | 退出 0；125 个模块，仍有 bundle 大于 500 kB 提示。 |
| `npm.cmd run test:ui -- --run` | 退出 0；8 个文件、192 项通过（新增 `tests/socialPlatforms.test.mjs` 7 项）。 |
| `scripts/cargo.ps1 check --workspace --all-targets` | 退出 0。 |
| `scripts/cargo.ps1 test -p veloct-app site_settings` | 退出 0；12 项通过，含 5 项新增头像写入测试。 |
| `site/` 内 `npm.cmd run build` | 退出 0；Astro 21 个页面。 |
| `node scripts/prepare-template.mjs` | 通过；`SocialIcon.astro` 与 `social-platforms.ts` 已进入打包模板。 |
| Rust workspace 全量测试 / clippy | `cargo.ps1 test --workspace --no-fail-fast` 退出 0：47 个目标、1529 passed / 0 failed / 16 ignored。clippy 本轮未执行。 |
| 安装包 | `Veloct_1.0.0_x64-setup.exe` 29,652,176 字节（28.28 MiB），`Valid`；独立主程序补签后 `Valid`。哈希见 `UPDATE.md`。 |

首次打包曾以 `tauri-build` 的 `PermissionDenied`（错误 5）失败，原因是用户正在运行旧的 `target/release/veloct-desktop.exe`；应用关闭后重建。

上一轮：新增“其他选项 → 外观”，内部切换站点信息与站点美化。`npm.cmd run build:ui` 退出 0；`npm.cmd run test:ui -- --run` 退出 0，7 个文件、185 项通过。随后已按用户要求重建 `target/release/`，安装包构建退出 0；补签独立主程序后，主程序和安装包签名检查均 Valid。

站点信息样式修复：已删除 `src/styles.css` 中配置字段的蓝色边框、浅蓝悬停高亮和蓝色聚焦描边，恢复公共表单样式。

2026-10-03 本会话相关改动后：

| 检查 | 结果 |
|---|---|
| `npm.cmd run build:ui` | 通过；TypeScript 与 Vite 构建通过，仍有 bundle 大于 500 kB 的提示。 |
| `npm.cmd run test:ui -- --run` | 通过；7 个测试文件、185 项通过。 |
| `site/` 内 `npm.cmd run build` | 通过；Astro 构建 21 个页面。 |
| 安装包 | 成功；`target/release/bundle/nsis/Veloct_1.0.0_x64-setup.exe`，28.26 MiB，签名成功。该包不包含之后的文档整理。 |
| Rust 全量测试 / clippy | 本轮未执行；不要引用旧文档数字声称本轮已通过。 |

## 接手流程

1. 先读 [NEXT_AI.md](<../../NEXT_AI.md>)；运行 `git status --short`，识别并保留既有改动。最新轮次已经交付，不默认重做测试/打包；先了解用户下一项需求。
2. 阅读用户最新要求、`handoff.md` 对应条款和实际实现；用 grep 搜索调用方及测试，不要只凭文件名推断功能已接通。
3. 跨 Rust/Tauri/UI/Astro 功能沿链路验证：React → `src/lib/api.ts` → Tauri command → app service/domain crate → 真相源或外部系统。只测 mock 不足以证明接线正确。
4. 修改后按范围运行测试，记录命令、退出结果和测试数；失败运行也保留记录。
5. 修改站点模板后运行 `node scripts/prepare-template.mjs`，补完整旧官方基线/哈希和升级保护测试；交付安装包时运行构建脚本并检查最终签名。若未实际安装该包，明确写“未装机”，不要沿用旧包安装结论。临时 NSIS 文件签名不等于最终包完成，细节见 NEXT_AI.md。
6. 完成一轮后更新 `UPDATE.md` 和本文件；已解决的问题在 `ISSUE.md` 中关闭或补证据。

## 文档约定

- `handoff.md` 是契约，不放运行状态和一次性测试数字。
- `PROCESSING.md` 保持短小、记录当前接手信息；旧流水账放 `docs/archive/`。
- `UPDATE.md` 面向测试者；新包未重新实测前，不写新的哈希或装机结论。
- `ISSUE.md` 只列有证据的问题，并区分已确认缺陷、未验证风险和业主决定。
- `docs/spec/02-test-matrix.md` 是规范测试证据表，改行为或测试后同步对应行。
- 不提交凭据、令牌、个人目录或机器私有配置；操作 git 时逐路径暂存，避免 `git add -A`。
