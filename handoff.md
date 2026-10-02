# HANDOFF.md — Veloct

> 本文件是给 AI 编码助手的正式项目交接文档。
>
> **项目名称：Veloct。**
>
> 本项目直接按**完整版本**开发，不做 MVP / 二期 / 远期功能分级。完整版本范围已经由项目发起人确认；开发阶段可以使用垂直切片、技术预研和分阶段施工，但不得因此删减正式需求。
>
> 标记为【锁定】的内容不得擅自修改。未写死的具体库、函数、SQL 实现等可以由编码 AI 选择成熟方案，但不得改变本文定义的产品行为、数据真相、生命周期和安全边界。

---

## 0. AI 编码助手最高优先级规则【锁定】

1. **产品范围是完整版本。** 不得以“MVP”名义删除、延后或默认不实现正式需求。
2. 工程实现允许分阶段，但必须最终覆盖本文全部需求。
3. 不得把网站部署与附件存储混成一个系统。
4. 不得把 Markdown 正文和 SQLite 索引当成两个平等内容源；**Markdown 是正文最终真相**。
5. 不得因为实现方便而把 WIP、页面、作品、附件等重新设计成与已确认行为不同的模型。
6. 不得把用户主动取消 AI 请求当作错误并触发保底模型。
7. 不得把 API Key、Token、Secret 等敏感凭据写进 SQLite、普通配置或备份包。
8. 不得让上传的作品代码获得主站页面控制权。
9. 不得自动删除远程附件。
10. 平时编辑 / 自动保存不得产生 Git commit；只有用户执行发布且检测到实际变化时才创建发布 commit。
11. 单次发布只构建一次 Astro；同一次发布的多个自动部署目标必须使用同一份构建产物。

---

# 1. 项目定位【锁定】

Veloct 是一个**本地优先、单用户、单网站**的个人网站桌面管理与静态发布系统。

核心用途：

- 管理个人文章、普通页面、知识库和作品。
- 使用 Markdown 作为正文主要格式。
- 使用 Astro 生成最终静态网站。
- 在桌面端完成编辑、预览、Git 版本控制、构建、发布、备份、附件管理和 AI 辅助。
- 网站部署完成后，不依赖桌面 App 常驻运行。

## 1.1 明确边界【锁定】

- 本项目只管理**一个个人网站**。
- 不做多站点切换。
- 不做 SaaS。
- 不做多人协作。
- 不做在线 CMS。
- 不把桌面端作为线上网站运行时依赖。
- 网站部署与附件存储是两套完全独立的系统。

---

# 2. 技术总架构【锁定】

## 2.1 桌面端

- **Tauri** 作为桌面应用壳和本地能力入口。
- Rust 负责文件、SQLite、Git、进程生命周期、凭据访问、备份和部署编排等本地能力。
- 桌面端是管理工具，不是线上网站服务器。

## 2.2 网站

- **Astro** 是最终静态网站生成器。
- Astro 同时是桌面端网页实时预览的实际渲染来源。
- 网站使用 **Tailwind CSS**。
- 线上网站为静态产物，部署后独立运行。
- Tauri 不参与最终线上网页渲染。

## 2.3 内容与数据

- 正文保存为 Markdown 文件。
- Markdown frontmatter 保存必要的内容元数据。
- frontmatter / Markdown 信息建立 SQLite 索引，方便管理界面查询。
- **Markdown 是正文内容的唯一最终来源。**
- SQLite 内容索引属于派生数据，可以从 Markdown 重建。
- 网站、应用、部署、备份等结构化配置直接存 SQLite。
- **不使用 YAML 作为网站配置存储。**

## 2.4 SQLite

使用**一个全局 SQLite 数据库**。

SQLite 至少负责：

- 内容索引
- 网站配置
- AI Provider / Model 配置（不包含 Secret）
- 部署目标配置（不包含 Secret）
- 发布记录
- 部署目标执行记录
- 附件与存储目标索引
- AI 请求 / 任务元数据（不长期保存聊天历史）
- 备份记录
- 日志索引 / 状态信息
- 其他应用状态

## 2.5 Git

- 项目目录使用 Git 管理。
- Git 负责内容和项目版本控制。
- 每个正式发布版本通过 Git tag `release/<release-id>` 绑定到唯一 Git commit SHA。
- 自动保存不产生 commit。
- 发布才产生发布 commit。

---

# 3. 内容模型与工程契约【锁定】

## 3.1 核心内容类型

至少包含：

- `article`：文章
- `page`：普通页面
- `knowledge`：知识库页面
- `work`：作品

**WIP 不是独立内容类型。**

作品的真相源与文章不同：作品使用 `works/<stable-id>/work.json` 保存元数据，并以同目录下的 `site/` 保存作品静态本体；SQLite 只保存索引。

WIP 是 `article` 的属性 / 标签状态。

## 3.2 Markdown frontmatter 最低契约

每个可管理 Markdown 内容必须具有稳定的内部 `id`。

最少支持下列语义字段：

```yaml
id: <stable-id>
type: article | page | knowledge
slug: <mutable-public-slug>
title: <title>
summary: <optional-summary>
wip: false
wip_publish: false
created_at: <timestamp>
updated_at: <timestamp>
published_at: <optional-timestamp>
tags: []
category: <optional>
cover: <optional-url-or-reference>
```

说明：

- `id` 永久稳定，**不能因为 slug 修改而变化**。
- `slug` 可以修改。
- `wip` 只对文章有意义。
- `wip_publish` 控制 WIP 是否跟随 Astro 静态生成。
- 具体额外 frontmatter 字段可以扩展，但不得改变以上字段语义。

## 3.3 Markdown 方言

正文至少支持：

- CommonMark / Markdown 基础语法
- GitHub Flavored Markdown（GFM）能力
- 表格
- 代码块与语法高亮
- 图片
- 链接
- 列表
- 引用

项目自定义能力采用**结构化 Markdown AST / directive 形式**实现，不允许用脆弱的字符串正则作为唯一解析方式。

需要覆盖的自定义内容包括：

- 知识库引用卡片
- 下载链接卡片
- 超链接卡片
- 其他页面区块

## 3.4 Source of Truth

正文数据流必须满足：

```text
Markdown
   ↓
解析
   ↓
SQLite 索引 / AST / 构建数据
   ↓
Astro
```

禁止形成：

```text
Markdown ←→ SQLite 正文双向竞争
```

如果 SQLite 索引损坏，应能够通过重新扫描 Markdown 重建。

---

# 4. 桌面端 UI 与编辑体验【锁定】

## 4.1 总体布局

- 左侧固定导航。
- 右侧主工作区。
- 整体为偏 Notion 风格的桌面工作台。

## 4.2 Markdown 编辑器

- 完整 Markdown 编辑能力。
- 默认所见即所得。
- 支持分屏模式。
- 分屏右侧必须使用**真实 Astro 渲染结果**。
- 编辑器 / 预览分栏比例可拖动。
- 内容变化应反映到 Astro 实时预览。
- 线上前端不包含 Markdown 编辑器。

## 4.3 自动保存【锁定】

- 输入停顿 2 秒后自动保存。
- 每 30 秒兜底保存。
- 关闭 App 时立即保存所有未保存内容。
- 自动保存不产生 Git commit。

## 4.4 文章信息

文章阅读页开头显示：

- 字数
- 预估阅读时间

---

# 5. Astro 实时预览工程契约【锁定】

这是本项目的关键技术风险，必须优先做真实技术验证，但**不能用另一套“伪 Astro 渲染器”替代**。

## 5.1 预览原则

- 预览使用与最终生产站**同一套 Astro 项目、组件、Markdown 解析、知识库解析和样式**。
- 不允许维护一套独立的“编辑器预览 HTML 渲染器”作为最终实现。
- Tauri 负责启动、监控和关闭本地 Astro 预览进程。

## 5.2 运行方式

推荐实现：

```text
Tauri
  ↓
管理本地 Astro dev / preview process
  ↓
本地 HTTP Server
  ↓
Tauri WebView 中的预览区域
```

为保证跨平台，允许将兼容的 Node.js 运行时作为 Tauri sidecar 一并分发；不要要求用户手动安装 Node 才能使用 Veloct。

## 5.3 生命周期

Tauri 必须负责：

- 动态选择可用本地端口。
- 启动 Astro 预览。
- 记录子进程 PID / 状态。
- 预览异常退出时显示明确错误。
- 支持自动重新启动预览进程。
- App 关闭时清理预览进程。
- 防止旧实例残留长期占用端口。

## 5.4 编辑同步

- Markdown 保存后，Astro 预览应通过 dev/HMR 或等价机制刷新。
- 不要求每次编辑都重新执行完整生产 build。
- 生产发布仍然执行完整 Astro build。

## 5.5 预览与正式构建一致性

预览与生产构建必须共享：

- Astro 配置
- 内容集合 / Markdown parser
- 组件
- 样式
- 知识库引用逻辑
- WIP 生成规则
- 作品路由规则

否则禁止声称“预览与正式结果一致”。

---

# 6. 前端网站视觉与路由【锁定】

## 6.1 视觉方向

采用已确认的 **Claude 深色模式**方向：

- 暖炭黑背景
- 暖色高亮
- 深色卡片分层
- 衬线标题
- 系统字体正文
- 不做浅色 / 深色切换

只维护一套统一模板。

## 6.2 图片策略

- 全站图片懒加载。
- 支持模糊占位 / blur-up。
- 加载完成后替换为原图。

## 6.3 主要路由

```text
/                       首页
/article                文章展示页
/article/[slug]         正式文章阅读页
/wip/[stable-id]        WIP 固定访问页（仅按 WIP 规则生成）
/works                  作品页
/works/[slug]           作品外壳页
/works/[slug]/...       作品本体入口
/docs                   知识库入口
/docs/[...path]         知识库页面
/archive                归档
/links                  友情链接
/about                  关于
```

WIP 使用稳定的内部 ID 对应固定访问路径 `/wip/<stable-id>`，不能依赖可变的正式 slug。

作品真正运行的静态本体部署到独立 Works Origin（例如 `works.example.com`）；主站 `/works/[slug]` 是外壳页，不与作品脚本共享主站同源存储。

## 6.4 页面模板

至少包含：

1. 首页
2. 404
3. 标签页
4. 分类页
5. 归档
6. WIP 文章展示 / 瀑布流
7. 友情链接
8. 文章展示页
9. 文章阅读页
10. 作品页
11. 知识库
12. 关于

## 6.5 侧边栏

左侧：

- 可折叠。
- 折叠时保留系统级导航图标。
- 展开时显示个人卡片、简介、用户名、社交入口和系统导航。

右侧：

- 默认展开。
- 有标签页切换。
- 文章展示页可显示日历、词云、标签、发布热力图。
- 文章阅读页可显示大纲、评论、知识库引用。
- 知识库引用点击后可在右侧打开对应知识位置。
- 标签页不需要跨页面持久化。

## 6.6 Hero

首页采用确定的 Hero 结构：

- 左侧标题 / 副标题 / CTA。
- 右侧个人头像。
- 头像为圆角矩形，不使用圆形头像。

## 6.7 内容能力

网站支持：

- RSS
- 文章搜索
- 词云
- Sitemap
- 灯箱
- 代码高亮
- 超链接卡片
- 默认封面图
- 下载链接卡片
- 百度网盘 / 阿里云盘 / 夸克 / OneDrive / Google Drive / MEGA 等下载链接识别图标

## 6.8 备案信息

支持可选：

- ICP 备案
- 公安备案
- MOE 备案

按单网站配置，填写后展示在页脚；不填写不影响网站运行。

---

# 7. WIP 文章【锁定】

- WIP 属于普通文章，不是独立草稿箱。
- 每篇 WIP 有稳定固定链接 `/wip/<stable-id>`。
- 固定链接不依赖文章当前 slug。
- WIP 可通过文章界面的按钮控制“是否跟随 Astro 生成静态”。

## 7.1 当前 WIP 状态

```text
wip = true,  wip_publish = false
→ 本地 Astro preview 可访问 /wip/<stable-id>
→ 生产 dist 不生成该 URL
→ 线上不存在该页面

wip = true,  wip_publish = true
→ 生产 dist 生成 /wip/<stable-id>
→ noindex
→ 不进入 sitemap
→ canonical 指向自身
→ 不同时生成 /article/<slug>

wip = false
→ 生产 dist 生成 /article/<slug>
→ 正常可索引
→ 进入 sitemap
```

WIP 瀑布流只展示进入当前静态产物的 WIP。

## 7.2 WIP 完成后的固定链接生命周期

如果某个 `/wip/<stable-id>` 曾经通过 `wip_publish=true` 实际公开过，则文章转为 `wip=false` 后，该地址不直接 404，而在后续静态构建中生成永久重定向到当前 `/article/<slug>`。

如果某个 WIP 从未公开过（始终 `wip_publish=false`），则不需要保留线上 WIP 重定向入口。

需要在内容元数据中记录“该 WIP URL 是否曾公开过”的事实。

## 7.3 WIP SEO

- WIP 页面始终 `noindex`。
- WIP 页面不进入 sitemap。
- WIP 页面 canonical 指向自身。
- WIP 完成后的历史 WIP URL 只承担重定向入口，不作为独立内容参与索引。

不建立独立 WIP 预览服务器。

# 8. 页面创建器【锁定】

页面创建器采用混合模式：

- 模板决定整体结构。
- 区块支持拖拽排序。
- 最终页面正文仍然由 Markdown 表达。
- 页面结构相关信息应通过 frontmatter / Markdown AST / directives 表达，不建立第二套不可重建的正文数据源。

## 8.1 AI 页面创建

AI 可以直接生成完整的一整页，包括：

- 页面结构
- 文案
- Markdown 内容
- 图片占位符

**不会实际生成图片文件。**

流程：

```text
AI 生成整页
→ 用户预览
→ 用户确认
→ 正式创建页面
```

未经确认，不得正式创建或覆盖页面。

## 8.2 AI 与区块映射

AI 生成页面必须输出项目支持的 Markdown / directives / frontmatter 结构，而不是创建只有 AI 自己懂的私有 JSON 页面格式。

---

# 9. 知识库【锁定】

- 知识库独立于文章模块。
- 文章与知识库可互相引用。
- 分类树自由，不写死分类维度。
- 状态标记使用自由文本。
- 知识库不进入文章普通导航 / 列表。

## 9.1 引用

编辑器插入知识库时生成引用卡片：

- 标题
- 原文摘录
- 省略号

点击卡片后在右侧边栏打开原文。

## 9.2 段级稳定 ID

- 引用粒度为段级。
- 默认按换行分段。
- 每段具有隐藏稳定 ID。
- ID 由编辑器自动维护。
- ID 不依赖段落正文内容。
- ID 必须稳定，不因为普通文字修改而改变。

推荐表达方式为 Markdown AST / 隐藏 HTML comment 元数据，具体实现可以调整，但不得让用户正文可见。

## 9.3 冲突规则

如果外部编辑、Git 合并或其他操作导致：

- ID 重复
- 引用 ID 不存在
- 段落 ID 丢失

则：

- 构建时产生明确 warning / error。
- 不允许静默把引用猜到别的段落。
- 原引用保持 broken state，等待用户修复。

## 9.4 动态引用

- 知识库更新后，被引用文章读取最新内容。
- 构建时解析。
- 知识库改动需要重新构建受影响内容。
- 文章末尾自动生成 Harvard style Reference List。
- 被引用知识库条目自动进入 Reference List。
- 知识库页面不显示“被哪些文章引用”。
- 知识库浏览器使用弹窗，支持搜索和层级浏览。

---

# 10. 作品系统【锁定】

## 10.1 作品真相源

每个作品使用稳定 ID 作为目录：

```text
works/<stable-id>/
├── work.json
├── preview/
│   ├── cover.webp
│   └── screenshot.webp
└── site/
    ├── index.html
    ├── assets/
    └── ...
```

- `work.json` 是作品元数据真相源。
- `site/` 是作品本体真相源。
- SQLite 只保存作品索引。
- `preview/` 属于作品本身，也进入项目备份。
- 作品本体真实文件进入 Git。
- 当前不要求 Git LFS，也不把作品真相源放到外部附件存储。
- 导入体积过大时可以给出警告，但不能因为体积而自动改变真相源。

这样保证：无网络时也能恢复、预览、重新构建历史 Release。

## 10.2 导入

支持：

- ZIP
- 文件夹

导入后解包到作品对应目录。

用户主要填写：

- 作品名称
- 作者（自动带默认值，可编辑）

完成时间取上传时间戳，默认不要求用户填写。

## 10.3 展示

- `/works` 支持海报墙 / 列表两种视图。
- 海报默认使用截图或占位。
- hover 时懒加载作品 iframe。
- hover 离开后销毁动态预览。
- 点击进入 `/works/[slug]` 外壳页。
- 外壳页顶部有浮动条，显示作品名称、完成时间、作者、Demo 声明。
- 作品本体可在独立 Works Origin 下直接访问。

## 10.4 作品安全隔离【锁定】

作品不能获得主站控制权。正式强隔离方案必须同时满足：

1. **独立 Origin**：作品本体运行在独立的 origin / 子域，例如：
   - 主站：`https://example.com`
   - 作品：`https://works.example.com`
2. **sandbox iframe**：主站嵌入作品时使用 sandboxed iframe。
3. 不授予 `allow-same-origin`。
4. 只开放实际需要的最小 sandbox 权限。
5. 主站不把 DOM、localStorage、JS bridge 等主动暴露给作品。
6. 不设置允许作品直接调用 Tauri bridge 的接口。
7. 主站与作品不能共享会话 Cookie / 同源 LocalStorage。

若某个部署目标无法提供独立 Works Origin，则该目标不能宣称提供“强隔离”作品运行环境；编码实现必须明确显示这是降级模式，并默认不得让作品以主站同源顶层文档运行。

## 10.5 作品部署编排

用户界面仍只展示三个逻辑自动部署目标：

- Cloudflare Pages
- EdgeOne · 中国大陆
- EdgeOne · 国际

每个逻辑目标内部必须有两个子目标快照：

```text
logical_target
├── main_target
└── works_target
```

一次 Astro build 只生成一份 `dist/`，但允许包含多个静态发布根：

```text
dist/
├── site/
└── works/
```

同一次 Release：

- `dist/site/` 发布到主站。
- `dist/works/` 发布到 Works Origin。

因此“一个逻辑目标”可以由适配器内部映射到两个物理平台项目，但用户不需要管理这两个物理项目。

失败重发时必须使用 Release 当时冻结的 `main_target` / `works_target` 子目标快照。

## 10.6 AI 页面创建中的作品关系

AI 页面创建只负责网站页面内容，不自动创建实际作品资源；需要作品时应通过作品导入流程创建正式 Work。

# 11. AI 系统【锁定】

AI 是可选能力，不是核心编辑 / 构建 / 发布 / 备份能力的运行依赖。

未配置 AI 时，Veloct 的核心编辑、SQLite、Git、Astro、预览、部署、备份、附件功能必须正常工作。

## 11.1 Provider

支持多个 Provider。

每个 Provider：

- 名称
- 协议类型
- Base URL
- API Key / Token
- Model 列表

协议至少支持：

- OpenAI Compatible
- Anthropic
- Ollama

Base URL 必须允许用户自定义。

## 11.2 Model

支持：

- 自动获取模型列表
- 手动添加模型
- 刷新模型

## 11.3 功能级模型分配

每个程序内置 AI 功能可独立指定：

```text
AI 功能 → Provider → Model
```

用户不能创建新的 AI 功能，也不维护长期提示词模板系统。

## 11.4 全局保底模型

只有一个全局保底模型：

- 可以使用完全独立的 Provider。
- 不要求与正常功能共用 Provider。
- 首选模型发生**真实调用错误**时切换保底模型。
- 保底模型再次失败才最终返回 Error。

前端提示：

> 首选模型调用失败，已自动切换到保底模型。

并提供默认折叠的首选失败原因。

### 错误边界

- 用户主动点击“停止生成”属于 `CANCELLED`，**不是错误**，不触发 fallback。
- 其他 Provider / API 调用失败按照“有错误就切换”的产品规则执行，包括连接错误、超时、4xx、5xx、模型不存在、上下文超限、Provider 拒绝等。
- fallback 本身失败后不得再次进入同一个 fallback 循环。

## 11.5 Provider 工具

- 测试连接
- 刷新模型

测试连接使用当前真实配置并显示成功 / 失败原因。

## 11.6 AI 生成过程

- 支持停止生成。
- AI 对话历史不长期保存。
- AI 不是独立聊天系统。

## 11.7 上下文

默认：

```text
用户临时提示词
+
当前 Markdown
```

编辑器选中内容后：

```text
用户临时提示词
+
选中内容
```

还可以主动选择网站中其他内容作为额外上下文：

- 文章
- 页面
- 知识库
- 其他可引用内容

不得默认把整个网站发送给 AI。

## 11.8 输出格式

支持：

- 预设格式
- 自定义格式要求

## 11.9 AI 结果应用

```text
AI 生成
→ 结果预览
→ 用户确认
→ 应用全部 / 应用局部 / 放弃
```

AI 不得未经确认直接覆盖 Markdown。

## 11.10 内置 AI 能力

至少包含：

- 文章摘要
- 文章润色 / 改写
- 页面创建
- 其他程序预先定义的 AI 功能

## 11.11 摘要

- 手动触发。
- 发布时未填写摘要，可选择：取消 / 手动填写 / AI 生成。
- AI 生成后可手动编辑。
- 保存到 frontmatter。

---

# 12. AI / Secret 凭据安全【锁定】

以下均视为敏感凭据：

- API Key
- API Token
- S3 Secret
- GitHub Token
- Cloudflare Token
- EdgeOne Token
- 其他部署 / 存储凭据

规则：

- 不进入 SQLite。
- 不进入普通配置文件。
- 不进入日志。
- 不进入备份包。
- 使用系统安全凭据存储 / keychain / credential store 等成熟机制。
- 具体平台的安全存储实现由 Tauri 生态选择，但必须使用操作系统级安全存储能力，而不是明文文件。

日志必须对 Authorization、Token、Secret、API Key 等做脱敏。

---

# 13. 附件与文件存储【锁定】

网站部署与附件存储完全解耦。

## 13.1 正式附件存储目标

1. 本机 / 服务器目录（§13.10）
2. S3 / S3-compatible
3. Lsky Pro（兰空图床）
4. GitHub 仓库
5. 免费网络图片供应来源（如 Pixabay）

## 13.2 本地附件缓存池

**所有本地文件上传动作首先进入本地缓存池。**

每个缓存文件默认继承全局默认存储目标，但可以在列表右侧：

- 单独修改目标。
- 拖拽到存储目标修改目标。
- 批量选择后上传到同一个目标。

### 13.2.1 三维状态

附件状态不是单个巨大枚举，而是三个独立维度：

```text
upload_status
  PENDING / UPLOADING / SUCCEEDED / FAILED

reference_status
  NOT_APPLIED / APPLYING / APPLIED / FAILED

cache_status
  PRESENT / CLEANED
```

### 13.2.2 关键迁移

```text
PENDING
→ UPLOADING
→ SUCCEEDED / FAILED
```

远端成功后：

```text
SUCCEEDED
→ 尝试把最终 URL 应用到 Markdown / 更新引用索引
→ APPLIED
→ cache_status = CLEANED
```

如果远程成功但引用应用失败：

```text
upload_status = SUCCEEDED
reference_status = FAILED
cache_status = PRESENT
```

此状态允许用户“重试应用引用”，**不得重新盲目上传同一文件**。

如果上传失败：

```text
upload_status = FAILED
reference_status = NOT_APPLIED
cache_status = PRESENT
```

允许重新上传。

### 13.2.3 引用不是单一布尔值

一个附件可以被多篇文章 / 页面引用。底层必须维护附件与内容引用之间的集合 / 索引关系。

`reference_status` 是聚合状态：

- `APPLIED`：至少存在一处有效 Markdown AST 引用。
- `NOT_APPLIED`：当前不存在有效引用。
- `FAILED`：最近一次引用应用操作失败。

删除其中一个引用，不会因为该引用消失就把状态改成 `NOT_APPLIED`；只有最后一个有效引用消失后才进入无引用状态。

### 13.2.4 安全孤儿

只有同时满足以下条件，缓存文件才可进入“可清理”：

- 不存在任何有效 Markdown AST 引用。
- 不在待应用引用队列。
- `upload_status` 不是 `PENDING` / `UPLOADING`。
- 不存在待恢复 / 重试的附件操作。

`PENDING` / `UPLOADING` 文件禁止被一键清理。
`SUCCEEDED + NOT_APPLIED` 或 `SUCCEEDED + FAILED` 文件必须经过明确的手动确认才能删除。

## 13.3 上传事务

缓存池文件 → 上传 → 记录远端对象 → 应用 Markdown 引用 → 建立引用索引 → 清理缓存。

任一步失败都必须保留足够状态以便继续操作。

如果用户在引用应用过程中编辑了同一篇文章并产生冲突，不得静默覆盖；必须提示并要求重新应用。

## 13.4 去重

上传文件使用内容哈希进行重复判断。

去重键至少包含：

```text
storage_target_id
+ storage_namespace
+ content_hash
```

不能单独用 `content_hash` 在整个系统里全局复用 URL。

同一存储目标、同一命名空间、相同内容：复用已有远端文件 / URL。

同名但内容不同：生成新的随机后缀。

并发上传相同 `storage_target_id + storage_namespace + content_hash` 时必须有并发保护，避免多个相同远端对象被重复创建；实现可使用 DB 锁、任务去重或等价机制。

## 13.5 storage_namespace

`storage_namespace` 是某个存储目标内部用于确定对象身份的稳定命名空间。当前语义：

- S3：`bucket + key_prefix`
- GitHub：`repository + branch + path_prefix`
- 本机 / 服务器目录：目标目录（项目根 + 目标 id）+ `path_prefix`
- Lsky Pro：`instance + configured album / upload scope`

改变路径前缀、远程目录等形成新的 namespace。旧 namespace 的对象记录继续保留，系统不自动搬迁远程文件。

## 13.6 公开 URL 前缀迁移

本机 / 服务器目录、S3、GitHub 等目标的公开 URL 前缀与对象实际路径分离。

修改公开 URL 前缀：

- 不自动搬迁远程文件。
- 不自动删除旧文件。
- 不自动假设旧文章引用仍然有效。
- 提供显式“重写附件引用”工具。

## 13.7 文件名

保留原始文件名，并在扩展名前添加**固定 6 位随机字符**：

```text
photo.webp
→ photo-a8K3xQ.webp
```

相同内容复用，不重新生成后缀。

## 13.8 远程删除

从文章中删除附件引用时：

- 只删除 Markdown 引用。
- **永远不自动删除远程文件。**

## 13.9 缓存清理

提供：

- 一键清除安全孤儿缓存。
- 手动选择缓存文件删除。

清理必须尊重状态和引用索引。

## 13.10 本机 / 服务器目录附件存储

使用本机目录，或这台机器已经能写入的服务器目录。**不需要 SSH 客户端**：目标是一个路径，不是一个协议。

用户可配置：

- 目录：由项目根与目标 id 决定，没有主机 / 端口 / 用户名 / 认证信息
- 路径前缀
- 公开 URL 前缀

## 13.11 S3

使用 S3-compatible API。

支持自定义：

- Endpoint
- Region
- Bucket
- 路径前缀
- 公开 URL 前缀
- Access Key
- Secret Key

## 13.12 Lsky Pro

只正式适配 Lsky Pro。

支持多个实例、实例地址 / API 地址、独立 Token。

## 13.13 GitHub

允许多个 GitHub 仓库作为附件目标。每个仓库支持：

- Repository
- 默认路径前缀
- 公开 URL 前缀
- 独立凭据

## 13.14 网络图片供应

Pixabay 等来源：

```text
搜索
→ 用户选择
→ 直接写入远程 URL
→ Markdown
```

不进入附件缓存池，不下载，不再上传。

# 14. 发布与部署【锁定】

## 14.1 单次构建与单份产物

每次发布只构建一次 Astro。

```text
Working Tree
→ Release Snapshot
→ Git commit
→ Git tag release/<release-id>
→ Astro build
→ 当前唯一 dist/
→ 分发
```

本地长期只保留一份当前 `dist/`。

同一次 Release 的所有自动部署目标必须共享同一份构建产物与 build fingerprint。

一次构建允许产生多个静态发布根：

```text
dist/
├── site/
└── works/
```

## 14.2 发布触发

**只支持手动发布。**

保存、预览、编辑不会自动部署。

## 14.3 发布变化判定

点击发布后：

1. 等待当前未保存修改完成保存。
2. 计算工作区变化。
3. 无实际变化：不创建 commit，不创建新 Release。
4. 有变化：创建发布 commit。

Commit message 由程序生成。

## 14.4 Release 真相源与 Git 锚点

Release 不能只存在 SQLite，也不能让 `release.json` 自包含本次 commit SHA。

采用以下不可循环模型：

### 第一步：生成 Release manifest

在 commit 前生成：

```text
.veloct/releases/<release-id>/release.json
```

`release.json` **不得包含自己的 commit SHA**。

它至少包含：

```text
release_id
build_inputs_hash
build_toolchain_snapshot
deployment_target_snapshot
created_at
```

### 第二步：Git commit

将 snapshot 与本次发布相关内容提交到 Git，得到 commit SHA。

### 第三步：创建 Release tag

创建：

```text
release/<release-id>
```

tag 指向本次发布 commit。

因此：

```text
Release 真相源 = release.json + Git tag + tag 指向的 commit
```

SQLite 只保存 Release 查询索引，不是唯一真相源。

## 14.5 Release 不可变性

Release manifest、build inputs、deployment target snapshot 在 Release 建立后视为不可变。

部署执行状态不反写 Release manifest；部署运行结果单独存为 DeploymentRun。

## 14.6 Build Inputs Snapshot

为保证历史 Release 可以重新构建，发布时必须把影响 Astro 构建的非 Secret 输入冻结到 snapshot。

至少包括：

- 内容版本 / 内容快照引用
- 网站配置快照
- 页面 / 导航配置
- WIP 生成相关配置
- 评论配置
- SEO 配置
- Work 元数据与作品构建相关输入
- build toolchain 版本
- lockfile hash
- 允许进入构建的非敏感环境变量
- 部署目标配置快照
- `credential_ref`，只保存凭据引用，不保存 Secret

发布时从 SQLite / 项目文件读取这些信息并生成 snapshot。

因此：

```text
SQLite = 当前运行态 / 配置
Release snapshot = 历史构建输入真相
```

## 14.7 Deployment Target Snapshot

用户可配置的当前部署目标不是历史 Release 的唯一依据。

每个 Release 冻结当时的部署目标配置。

每个逻辑部署目标必须支持子目标：

```text
logical_target
├── main_target
└── works_target
```

例如 Cloudflare：

```text
Cloudflare Pages
├── main_target = 主站项目
└── works_target = Works Origin 项目
```

如果用户后来修改项目名、站点绑定、Works 项目等，历史 Release 重发仍使用原 snapshot。

## 14.8 构建 Fingerprint

```text
build_fingerprint = hash(
    release_commit_sha
    + build_inputs_hash
    + lockfile_hash
    + toolchain_version
    + build_env_hash
)
```

`build_inputs_hash` 不含 `commit_sha`。

`build_env_hash` 只允许纳入经过白名单确认的、影响构建且非敏感的环境变量。不得哈希“全部环境变量”。

## 14.9 构建确定性与语义一致

不要求 dist 二进制文件逐字节完全一致。历史 Release 重建必须做到：

- 使用同一个 Release commit。
- 使用同一个 build inputs snapshot。
- 使用相同 lockfile / toolchain。
- 构建时间 / RSS / Sitemap 等可变时间字段使用 Release 固定值或稳定规则。
- 输出 manifest 在规范化后可稳定比较。

规范化至少包括：

- 路由排序。
- 资源清单排序。
- Sitemap 条目排序。
- RSS 条目排序。
- 固定 / 移除不应随构建重建而变化的时间字段。
- 不把 Secret 或随机运行时变量纳入 build fingerprint。

“语义一致”按以下可测试标准定义：

- 路由集合一致。
- 页面内容哈希一致。
- 静态资源清单与资源内容哈希一致。
- Sitemap 条目一致。
- RSS 条目一致。
- WIP 路由集合一致。
- Works 路由集合一致。

## 14.10 正式自动部署目标

正式支持且可独立勾选：

1. **Cloudflare Pages**
2. **EdgeOne · 中国大陆**
3. **EdgeOne · 国际**

EdgeOne 两个区域是两个独立 logical deployment target。

## 14.11 1Panel

1Panel **不是自动部署适配器**。

发布生成当前唯一 `dist/` 后：

- 用户自行将 `dist/site` 与 `dist/works` 静态内容上传到自己的 1Panel 网站环境 / Works Origin 环境。

## 14.12 部署结果

同一个逻辑目标内：

```text
main_status
works_status
```

因此可能出现：

```text
Cloudflare
├── main  ✅
└── works ❌
```

该逻辑目标整体状态为“部分成功”，但成功部分不能被撤回。

## 14.13 失败处理与手动重发

部署失败：

- 不自动重试。
- 不撤回其他成功目标。
- 前端显示失败原因。
- 用户可单独重新发布失败的子目标。

重新发布失败的子目标必须：

1. 使用原 Release 的 commit / tag。
2. 使用原 Release 的 build inputs。
3. 使用原 Release 的对应 `main_target` / `works_target` snapshot。
4. 如果当前 `dist` 的 fingerprint 仍匹配该 Release，则优先复用。
5. 否则在临时目录重新 build，完成后删除临时产物。

不得把当前 Working Tree 的新修改混进历史 Release 重发。

## 14.14 回滚

整体回滚按 Release：

```text
选择历史 Release
→ 使用其 tag / commit + snapshot
→ 重建
→ 使用同一发布流程部署
```

回滚不会改写历史 Release，也不会删除当前项目内容。

# 15. SEO【锁定】

支持：

- 百度
- Bing
- Google
- 360

网站功能：

- robots.txt / meta noindex 联动的禁止搜索引擎开关
- 自动 OG 图
- 结构化数据
- Sitemap

## 15.1 OG 图

OG 图在构建过程中生成。

实现可以使用成熟的 SVG / Satori / Sharp 等工具，但不得依赖联网下载字体才能完成构建；需要的中文字体资源应作为项目依赖 / 构建资源可控管理。

## 15.2 SEO 推送生命周期

SEO 推送属于发布后的独立任务，不是静态站生成的一部分。

例如：

```text
站点发布 ✅
SEO 推送 ⚠️
```

SEO 推送失败：

- 不判定整个网站发布失败。
- 记录日志。
- 允许用户手动重新执行。
- 离线时可以标记为待处理，不要求后台偷偷无限重试。

---

# 16. 评论【锁定】

评论功能可选开启。

开启后：

- 必须连接 GitHub。
- 使用 GitHub SSO / OAuth。
- 评论存储在 GitHub Discussions。
- 前端评论区使用 giscus。
- 提供提示 icon 告知用户评论存储位置。

本地 Astro 预览中：

- 不实现假的本地评论系统。
- 评论不可用时显示明确的不可用状态 / 已发布站点才能使用的提示。

---

# 17. 备份系统【锁定】

## 17.1 备份范围

备份采用**整个项目目录的完整打包**。

应包含项目实际需要恢复的内容，例如：

- Markdown
- SQLite
- 网站 / 应用配置
- Git 仓库
- Astro 项目与源文件
- 当前唯一 `dist/`
- 本地附件缓存
- 其他项目运行所需数据

必须排除：

- API Key
- Token
- Secret
- S3 Secret
- GitHub / 部署平台访问凭据
- 其他敏感凭据

## 17.2 备份包

- **不加密。**
- 凭据单独存储，不因备份而混入项目目录。
- GitHub 备份目标必须使用私有仓库。
- S3 备份目标必须使用私有 bucket / 私有访问控制。
- UI 必须明确提示用户：备份包未加密，只应发送到可信、私有目标。

## 17.3 Secret 扫描【锁定】

因为备份包含 Git 仓库，所以“当前配置不含 Secret”不代表 Git 历史中绝对没有旧 Secret。

备份前必须进行 Secret / Credential 扫描。

至少检查：

- 当前工作树文件
- 可被备份的配置文件
- Git 历史 / Git 对象中明显的常见 Secret 模式

如果发现疑似敏感信息：

- 自动云端备份不得静默上传。
- UI 明确提示发现疑似 Secret。
- 记录日志。
- 用户手动确认后才能执行该次高风险备份 / 导出。

系统不得擅自改写 Git 历史来“清理” Secret。

## 17.4 云端目标

正式支持：

- GitHub
- S3 / S3-compatible

不继续扩展网盘适配器。

## 17.5 自动备份

条件：

1. 内容发生变化。
2. 连续默认 **10 分钟**没有新修改。
3. 执行一次备份。

期间再次修改则重新计时。

## 17.6 保留数量

- 每个云端目标默认保留 **3 份**。
- 用户可以修改保留数量。
- 过量旧备份按该目标的保留策略清理。

## 17.7 自动备份部分成功

自动备份会尝试所有已配置的云端目标，但允许部分失败。

例如：

```text
GitHub ✅
S3      ✅
```

或：

```text
GitHub ✅
S3      ❌
```

成功目标独立保留本次备份；失败目标不影响成功目标。

## 17.8 手动备份

支持随时手动备份。

用户可以临时选择本次使用哪些云端备份目标。

## 17.9 本地导出

提供“导出备份”：

- 用户选择本地目录 / 文件名。
- 导出完整备份包。
- 不受云端 3 份保留策略影响。

## 17.10 恢复

支持：

### 完整恢复

恢复整个项目。

恢复流程必须采用“临时解包 → 校验 → 确认 → 替换”的安全方式，不能直接半覆盖当前项目。

完整恢复会恢复：

- Markdown
- Works
- SQLite
- Git 仓库与 Release tag / metadata
- 当前 dist
- 项目配置
- 其他进入备份包的项目数据

但不会恢复真实 Secret；恢复后必须重新绑定缺失的 `credential_ref`。

### 仅恢复内容

“内容”包括：

- Markdown 内容
- 内容 frontmatter
- Knowledge 内容
- Works（`work.json` + `site/` + `preview/`）
- 内容相关附件数据
- 内容相关 SQLite 索引

“仅恢复内容”**不覆盖历史 Release 元数据、Git tag、部署目标配置、AI Provider 配置、凭据、应用设置。**

恢复内容后，当前 Working Tree 视为已发生变化：

- 当前工作区可能与最近 Release 不同。
- 不自动生成新 Release。
- 用户必须重新发布才能让恢复后的内容进入线上站点。
- 历史 Release 保持不可变。

恢复完成后重新扫描 Markdown / Works 并重建内容索引。

## 17.11 不做

- 实时云同步
- 自动冲突合并
- 多人协作同步

---

# 18. 日志系统【锁定】

- 写 log 文件。
- 记录操作、保存、发布、构建、部署、备份、AI、附件等关键事件。
- 使用 `[info]` / `[warning]` / `[error]` 等级。
- 提供独立日志页面。

## 18.1 日志安全

日志不得记录：

- API Key
- Token
- Secret
- Authorization header
- 完整的敏感请求体

敏感字段必须脱敏。

日志应进行合理的大小控制 / 轮转，避免无限增长。

---

# 19. 工程契约总览【锁定】

这是编码 AI 不应自行改变的模块间合同。

## 19.1 内容链路

```text
Markdown
→ parser / AST
→ SQLite 索引
→ Astro
→ dist
```

SQLite 内容索引损坏可以重建；不能要求用户手动维护第二份正文。

## 19.2 编辑链路

```text
编辑器
→ 自动保存
→ Markdown
→ Astro 本地预览
```

自动保存不触发 Git commit。

## 19.3 发布链路

```text
用户点击发布
→ 检查实际变化
→ 有变化才 commit
→ Astro build
→ 当前唯一 dist
→ 自动部署目标分发
→ 记录每个目标状态
```

## 19.4 发布版本链路

```text
release_id
↔ commit_sha
↔ build_fingerprint
↔ deployment results
```

失败目标重新发布时必须使用原 release 的 commit，不得拿当前未发布工作区混进去。

## 19.5 附件链路

```text
本地文件
→ 缓存池
→ 目标选择
→ 上传
→ URL 写入 Markdown
→ 引用确认
→ 清理缓存
```

任何中间失败都不能让文件状态变得不可恢复。

## 19.6 AI 链路

```text
用户提示词
+ 当前 / 选中 Markdown
+ 用户主动选择的额外上下文
→ 首选 Provider / Model
→ 错误时 fallback
→ 结果预览
→ 用户确认
→ 应用全部 / 局部
```

主动取消不触发 fallback。

## 19.7 备份链路

```text
项目目录
→ Secret scan
→ 完整打包（排除 Secret）
→ 云端目标 / 本地导出
→ 记录状态
```

## 19.8 作品链路

```text
作品源文件
→ 静态作品资源
→ sandboxed iframe 预览
```

作品代码不得获得主站控制权。

---

# 20. SQLite、外部修改与项目操作并发【锁定】

## 20.1 SQLite migration

SQLite 必须有 schema version / migration 机制。

- 升级必须可重复执行且可检测当前版本。
- 数据库损坏时，允许从 Markdown / Work 等项目真相源重新建立可重建索引。
- 不把 SQLite 损坏视为项目内容丢失。

## 20.2 外部 Markdown 修改

如果编辑器正在编辑某文件，同时发现磁盘内容已被外部程序修改：

- 不允许静默覆盖外部修改。
- 必须检测基准内容 hash 与磁盘 hash 的差异。
- UI 提示用户选择重新载入、保留当前编辑或进入冲突处理。

## 20.3 Git 冲突

- 自动保存不自动解决 Git merge conflict。
- 冲突必须可定位到文件。
- 不能把 unresolved conflict 静默提交为发布版本。

## 20.4 项目操作锁

项目目录、Git 和 SQLite 共享同一项目状态。

### 独占操作

以下操作需要项目级 exclusive lock：

- Publish
- Restore
- 切换 / 重建历史 Release
- 其他会整体替换工作树的操作

独占操作期间不能产生竞争性的自动保存或另一个发布。

### 共享操作

普通编辑、Astro Preview、附件上传、自动备份允许并行，但备份必须从一致性快照开始。

### 关闭 App

关闭 App 时：

- 先完成未保存内容保存。
- 正在上传的附件不得静默丢失；应保存可恢复状态。
- Astro preview 子进程必须退出或进入可清理状态。

## 20.5 Backup 一致性快照

自动备份开始时建立一个项目一致性快照点，再从该快照生成备份包。

用户可以继续编辑，但本次备份只代表该快照，而不是抓取一个不断变化的目录。

---

# 21. Engineering Lifecycle Contracts【锁定】

以下五个生命周期必须同时定义：**真相源、输入快照、状态、失败行为、恢复路径、测试点**。

## 21.1 Release Lifecycle

| 项目 | 契约 |
|---|---|
| 真相源 | `.veloct/releases/<release-id>/release.json` + Git tag `release/<release-id>` |
| commit 锚点 | tag 指向发布 commit |
| snapshot | 非 Secret 的 build inputs + deployment target snapshot |
| SQLite | 只做查询索引 |
| 失败 | Release 历史不修改，DeploymentRun 单独记录 |
| 恢复 | 按 tag / snapshot 重建 |
| 测试 | tag→commit、snapshot hash、历史重建、失败目标重发 |

## 21.2 Build Lifecycle

| 项目 | 契约 |
|---|---|
| 输入 | Release commit + build inputs snapshot |
| 输出 | 当前唯一 `dist/`，包含 `site/` 和 `works/` 发布根 |
| fingerprint | 由 release commit、inputs hash、lockfile、toolchain、白名单 build env 计算 |
| 确定性 | 通过规范化后的站点 manifest 验证，不要求字节级 dist 相同 |
| 失败 | 不替换上一份可用 dist |
| 恢复 | 用原 Release 临时重建 |
| 测试 | 双次重建 fingerprint、语义 manifest 比较 |

## 21.3 Work Lifecycle

| 项目 | 契约 |
|---|---|
| 真相源 | `works/<stable-id>/work.json + site/` |
| 备份 | 随项目完整备份，不依赖外部对象才能恢复 |
| 预览 | 外壳页 + sandbox iframe |
| 运行隔离 | 独立 Works Origin + sandbox |
| 发布 | `dist/works` 随 Release 分发 |
| 失败 | 作品部署可以独立失败 |
| 恢复 | 从 Git / 完整备份直接恢复 |
| 测试 | 导入、截图、独立 Origin、sandbox、历史 Release 重建 |

## 21.4 Attachment Lifecycle

| 项目 | 契约 |
|---|---|
| 真相源 | Markdown AST 引用 + 远程对象记录 + cache state + 引用集合 |
| 去重 | target + namespace + content hash |
| 上传失败 | 缓存保留 |
| 引用失败 | 不重新上传，允许重试引用应用 |
| 删除引用 | 不删除远端文件 |
| 清理 | 只处理安全孤儿 |
| 恢复 | 保留可恢复的 pending / failed state |
| 测试 | 多引用、并发上传、引用冲突、清理保护 |

## 21.5 Backup Lifecycle

| 项目 | 契约 |
|---|---|
| 输入 | 一致性项目快照 |
| 包含 | 项目目录 + Git + Release metadata / tags + Works + current dist 等恢复所需内容 |
| 排除 | Secret / credential values |
| 安全 | Secret scan；GitHub private repo；S3 private bucket |
| 保留 | 云端默认 3 份，可配置 |
| 失败 | 目标独立失败，不影响其他目标 |
| 恢复 | 完整恢复 / 仅内容恢复 |
| 测试 | 完整恢复、内容恢复、Secret 排除、Release/tag 保持 |

---

# 22. 关键数据结构最低要求【锁定】

以下不要求编码 AI 现在就写成最终 SQL，但语义必须存在。

## 22.1 Content

至少需要：

```text
id
type
file_path
slug
title
frontmatter metadata
created_at
updated_at
```

## 22.2 Release

SQLite 索引至少需要：

```text
release_id
commit_sha
build_fingerprint
created_at
```

项目真相源：

```text
.veloct/releases/<release-id>/release.json
Git tag: release/<release-id>
```

`release.json` 不包含自身 commit SHA。部署运行状态不写入 Release manifest。

## 22.3 DeploymentRun

至少需要：

```text
id
release_id
logical_target_id
sub_target: main | works
status
error
started_at
finished_at
```

历史 Release 重发必须读取对应 Release 冻结的 `main_target` / `works_target` snapshot。

## 22.4 Attachment

至少需要：

```text
id
original_name
content_hash
mime_type
size
storage_target_id
storage_namespace
remote_key
public_url
upload_status
reference_status
cache_status
created_at
```

并存在附件引用集合 / 索引：

```text
attachment_id
content_id
reference_location
created_at
```

## 22.5 KnowledgeParagraph

至少需要：

```text
knowledge_content_id
paragraph_id
paragraph_order
content_hash / revision marker
```

## 22.6 AI Request

只保存必要的请求元数据与结果状态，不建立长期聊天历史。

至少包含：

```text
id
feature_id
provider_id
model
status
created_at
completed_at
error_code
```

## 22.7 Backup

至少需要记录：

```text
backup_id
target_id
created_at
status
artifact_identifier
size
error
```

---

# 23. 发布 / 构建一致性要求【锁定】

以下行为必须通过自动化测试覆盖：

1. Release tag 唯一且始终指向对应发布 commit。
2. 同一个 Release 的多个部署子目标收到完全相同的 build fingerprint。
3. 失败目标重发不会混入之后的工作区变化。
4. 历史 Release 重建不读取当前 SQLite 的新配置作为旧输入。
5. 同一 Release 二次重建的 fingerprint 相同。
6. 规范化后的路由、页面、资源、Sitemap、RSS、WIP、Works manifest 可稳定比较。
7. 无内容变化时发布不产生空 commit。
8. WIP `wip_publish=false` 时不得进入 dist。
9. `wip_publish=true` 时必须生成 `/wip/<stable-id>`。
10. WIP 完成后，曾公开过的 `/wip/<id>` 必须生成重定向到 `/article/<slug>`。
11. Works 主站外壳与 Works Origin 的发布目标使用同一 Release fingerprint。

---

# 24. 附件一致性要求【锁定】

必须通过测试覆盖：

1. 相同内容 + 相同存储目标 + 相同 namespace → 复用。
2. 相同内容 + 不同存储目标 → 不错误复用 URL。
3. 同名不同内容 → 新随机后缀。
4. 上传失败 → 缓存保留。
5. 远程成功、Markdown 更新失败 → 不丢文件。
6. 删除文章引用 → 不删除远程文件。
7. 一键清理不能删除仍被引用的文件。
8. 一键清理不能删除 PENDING / UPLOADING。
9. `SUCCEEDED + FAILED + PRESENT` 可以重试引用应用且不重复上传。
10. 多篇文章引用同一附件时，删除单个引用不会错误地将附件变成无引用。
11. 并发上传相同 target + namespace + hash 时只能得到一个最终远端对象或等价复用结果。

---

# 25. AI 安全与生命周期测试要求【锁定】

必须覆盖：

1. 用户点击停止 → 请求取消，不 fallback。
2. Provider 连接失败 → fallback。
3. Provider 超时 → fallback。
4. Provider 4xx / 5xx → fallback。
5. fallback 失败 → 最终 Error，不无限循环。
6. AI 结果在未确认前不会改写正文。
7. 局部应用只改变用户选定范围。
8. Secret 不进入日志。

## 25.1 Restore / Concurrency Tests

必须覆盖：

1. 完整恢复后 Git tag / Release metadata 可用。
2. 仅恢复内容后当前工作区被标记为与最近 Release 不同，但历史 Release 不变化。
3. 备份期间编辑不会让备份包出现半写入文件。
4. Publish 独占锁期间不能启动第二个 Publish。
5. Restore 独占锁期间不允许竞争性自动保存 / 备份覆盖工作树。
6. App 关闭时正在上传的附件状态可恢复。

---

# 26. 技术实现自由度【锁定边界】

编码 AI 可以自行决定：

- 具体 Markdown 编辑器库
- 具体 SQLite Rust crate
- 具体 Git Rust / WASM 实现
- 具体 keychain / credential store crate
- 具体 Astro component 组织方式
- 具体 Tauri sidecar 打包方案
- 具体 S3 SDK
- 具体 Lsky API client 实现
- 具体 Cloudflare Pages / EdgeOne API client 实现
- 具体日志框架
- 具体状态管理方式

但不能自行改变：

- 单网站
- 完整版本范围
- Markdown 为正文真相
- Work 的 `work.json + site/` 真相源
- SQLite 为索引 / 当前运行态配置
- 单次发布只构建一次、长期只保留一份当前 dist
- 手动发布
- 发布有变化才 commit
- Release tag 作为源码锚点
- Cloudflare Pages / EdgeOne 中国大陆 / EdgeOne 国际三个逻辑自动部署目标
- 每个逻辑部署目标具有 main / works 子目标快照
- 1Panel 手动上传
- WIP 语义与稳定 URL 生命周期
- 作品独立 Origin + sandbox 隔离原则
- 附件缓存池、引用集合、三维状态和远程不自动删除
- AI 全局 fallback 行为
- 备份 3 份默认保留
- Secret 不进备份

---

# 27. 开发执行原则【锁定】

虽然项目**不做 MVP**，但工程实施必须采用可验证的垂直切片。

推荐顺序不是功能优先级，而是为了尽早发现技术风险：

1. Veloct 桌面壳 + 项目目录 + SQLite + Markdown
2. Astro 工程 + Tauri 管理的实时预览进程
3. 编辑器 + 自动保存 + WIP
4. Git + Release tag + Build Inputs snapshot + 单份 dist + 发布链路
5. Cloudflare Pages / EdgeOne 三逻辑目标分发（含 main / works 子目标）
6. 作品系统、Works Origin 与 sandbox 隔离
7. 知识库动态引用与引用修复工具
8. 附件缓存池、引用集合与全部存储适配器
9. AI Provider / Model / fallback / 应用确认
10. 备份 / 恢复 / Secret scan
11. SEO / 评论 / 日志 / 其余内容能力
12. 全量自动化测试、故障恢复和打包分发

以上是**施工顺序**，不是 MVP / 二期 / 远期分级。

任何垂直切片完成后，都应该执行对应构建 / 测试，并记录真正的工程事实。

---

# 28. 完整版本完成定义【锁定】

只有在以下能力全部可工作时，才视为 Veloct 完整版本完成：

- 单网站项目可以创建并管理。
- Markdown 可编辑、自动保存、关闭时保存。
- SQLite 索引可重建。
- Astro 可以真实实时预览。
- WIP 可以独立控制是否进入静态生成，并拥有稳定链接 / 完成后重定向语义。
- 知识库可以进行稳定段级动态引用，并能修复 broken references。
- 页面创建器可以使用模板 / 区块 / Markdown。
- AI 可以按 Provider / Model 工作并正确 fallback。
- AI 结果可以确认、全部应用或局部应用。
- 作品可以导入、截图、iframe 预览，并与主站隔离。
- Git 发布 commit 与 Release tag 正确生成。
- 无变化不产生空 commit。
- 单次发布只构建一次。
- Cloudflare Pages、EdgeOne 中国大陆、EdgeOne 国际可以独立发布主站 / Works 子目标。
- 失败目标可以手动按原 Release 重发。
- 1Panel 可以由用户手动上传当前 `dist`。
- 附件缓存池、本机 / 服务器目录、S3、兰空、GitHub、远程图片供应全部按规则工作。
- 相同文件可以安全去重；多引用场景不会错误清理。
- 远程附件不会因删除引用而自动删除。
- 自动备份、手动备份、本地导出、完整恢复、仅内容恢复可用。
- Secret 不进入普通配置、日志或备份；备份前存在 Secret scan。
- SEO、评论、日志功能可正常运行。
- 关键失败场景有明确错误状态，而不是静默失败。

---

# 29. 最终原则

> **Veloct 是一个完整的本地优先个人网站工作台，不是 MVP。**
>
> **Markdown 是正文真相，Work 使用 `work.json + site/` 作为作品真相源，SQLite 是索引 / 运行态配置，Astro 是最终渲染来源。Git commit 通过 `release/<release-id>` tag 成为 Release 的源码锚点。**
>
> **单次发布只构建一次，多目标共享同一构建；历史版本需要重发时，按其 tag / commit 与 frozen snapshot 精确重建。**
>
> **附件进入缓存池后才上传，远程文件永不因文章删除而自动删除。**
>
> **AI 是可选能力，用户确认后才应用结果；用户主动取消不视为错误。**
>
> **作品必须运行在独立 Works Origin，并通过 sandboxed iframe 嵌入主站；作品代码不得获得主站控制权。**
>
> **备份完整但排除 Secret，并在上传前进行 Secret 扫描；云端备份目标必须是私有的。**
>
> 不允许为了“简单”而破坏以上原则。
