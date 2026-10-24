# 把 Veloct 网站部署到 Cloudflare Pages

面向第一次配置的人：从零开始，一步一步点，不需要懂 Cloudflare 的其他功能。

先记住分工：

- **Veloct 生成文件**。点「发布」后，网站产物在你项目目录下的 `site\dist\site`（主站）和 `site\dist\works`（作品站）。
- **Cloudflare 托管文件**。你把文件夹传上去，它给你一个免费 HTTPS 网址。

下面走的是「网页拖拽上传」路线，全程不需要命令行。文末附命令行方式和常见故障。

---

## 第 0 步：准备账号

1. 打开 <https://dash.cloudflare.com/sign-up> 注册（免费，只需邮箱并完成验证）。
2. 登录后进入控制台。**这一步不需要买域名**，先用 Cloudflare 送的 `*.pages.dev` 网址即可。

## 第 1 步：在 Veloct 里发布一次

1. 打开 Veloct，左侧点「发布」。
2. 点「发布」按钮，等提示成功。
3. 去文件资源管理器，打开你的项目目录，确认存在：

   ```text
   <项目目录>\site\dist\site\index.html
   ```

   如果项目有作品，还会看到 `site\dist\works\`。

**注意：当前 1.0.1 的实际目录是 `site\dist\site`，不是项目根目录的 `dist\site`。** 发布流程会先创建根目录 `dist`，但 Astro 把页面写到 `site\dist`，因此根目录 `dist` 为空不代表发布失败。`.veloct/releases/` 保存的是发布清单和记录，不是可直接上传的网站。

如果 `site\dist\site\index.html` 也不存在，先看 Veloct「发布」页的「发布没有完成」错误或「查看构建输出」，不要继续上传；把错误和最后几行日志提供出来排查。

## 第 2 步：进入 Pages

1. 在 Cloudflare 控制台左侧菜单点 **Workers & Pages**。
2. 点页面上的 **Create application**（创建应用）。
3. 在弹出页里切到 **Pages** 标签，选择 **Drag and drop your files**（拖拽上传资产）。

> 控制台改版时按钮文字会变。认两个关键词即可：**Pages** 和 **Drag and drop / Upload assets**。

## 第 3 步：起项目名

1. 在 **Project name** 里填一个全小写、用连字符的名字，例如 `veloct-blog`。
2. 它会决定你的默认网址：`https://veloct-blog.pages.dev`。
3. 点 **Create project**（或「继续」）进入上传页。

项目名以后不建议改，改了会换网址。

## 第 4 步：上传主站文件

1. 在文件资源管理器里打开 `<项目目录>\site\dist\site`。
2. **进入这个文件夹内部**，按 `Ctrl+A` 全选里面的内容。
3. 把这些内容拖进浏览器里的上传框，等进度条走完。

**关键检查点**：部署根目录必须直接包含 `index.html`、`_astro`、`about` 等。Cloudflare 也支持直接拖入构建文件夹；能否打开首页取决于最终根目录，而不是是否拖了文件夹。不要把包含主站和作品两套输出的上一级 `site\dist` 当作网站根目录。

| 拖错了什么 | 结果 |
|---|---|
| 拖了根目录 `dist` | 当前版本这里可能为空，没有网站可上传 |
| 拖了整个 `site\dist` | 网站外面多了 `site/` 层级，首页可能 404 |
| 只拖了 `index.html` | 页面能开，样式和图片全丢 |

## 第 5 步：部署并验证

1. 点 **Deploy site**（部署站点），等 1–2 分钟。
2. 打开它给的地址 `https://<项目名>.pages.dev`。
3. 逐项确认：首页能开、点进一篇文章、图片正常、手机上也能看。

有问题先看文末「常见问题」，不要急着绑域名。

## 第 6 步（可选）：绑定自己的域名

已经有域名才需要这一步。**注意：域名最省事的情况是它本身就托管在 Cloudflare。**

1. 打开这个 Pages 项目，点 **Custom domains**（自定义域）。
2. 点 **Set up a custom domain**，输入 `blog.example.com`，确认。
3. 按域名位置二选一：
   - **域名已在同一个 Cloudflare 账号**：DNS 记录会自动加好，等状态变 **Active**。
   - **域名在别家**（阿里云、DNSPod 等）：去那家 DNS 后台添加记录 —— 类型 `CNAME`，名称 `blog`，目标 `<项目名>.pages.dev`，然后回来等状态变 Active。
4. 状态 Active 前访问报证书错误是正常的，通常几分钟内签发完成。

**绑定成功后必须回 Veloct**：左侧「外观」→「站点信息」→「信息」→ 把「站点 URL」改成 `https://blog.example.com` → 重新「发布」→ 再按第 4、5 步上传一次。

不改这一步，页面里的 canonical、分享卡片和 sitemap 还是 `pages.dev` 地址。

## 第 7 步：以后怎么更新

1. Veloct 里改完内容，重新「发布」。
2. Cloudflare 里打开这个 Pages 项目，点 **Create a new deployment**。
3. 重新拖一次新的 `site\dist\site` 内容。

没有「自动同步」，每次都是这两步。嫌麻烦就用下面的命令行方式。

---

## 作品站（只有用了作品功能才需要）

作品是第三方代码，必须跑在和主站不同的域名下，这是 Veloct 的安全边界。

1. 重复第 2–5 步，**再建一个 Pages 项目**（例如 `veloct-works`），上传的是 `site\dist\works`。
2. 按第 6 步给它绑一个子域名，例如 `works.example.com`，等 Active。
3. 打开项目目录下的 `data\site.json`，把作品域名写进去：

   ```json
   "worksOrigin": "https://works.example.com",
   ```

4. 回 Veloct 重新「发布」，再把主站上传一次。

不填 `worksOrigin`，作品页里会出现 `works.example.com` 这类示例地址。

> 部署设置里的「作品源（独立 origin）」只用于记录和结果展示，不影响构建产物；真正生效的是 `data\site.json` 的 `worksOrigin`。

---

## 命令行方式（可选）

机器上已有 Node 22（Veloct 本身要求），直接可用：

```powershell
# 首次：登录，会打开浏览器授权
npx.cmd wrangler login

# 首次：创建项目（名字要和网页里一致）
npx.cmd wrangler pages project create veloct-blog

# 每次更新：上传主站
npx.cmd wrangler pages deploy "D:\你的项目目录\site\dist\site" --project-name veloct-blog
```

wrangler 上限是 20000 个文件、单文件 25 MiB；网页拖拽是 1000 个文件、单文件 25 MiB。

---

## 常见问题

| 现象 | 原因与处理 |
|---|---|
| 根目录 `dist` 为空 | 先找 `site\dist\site\index.html`；这是当前版本实际的主站输出位置 |
| 打开是 404 / "Nothing is here yet" | 确认上传的是 `site\dist\site` 构建目录，部署根目录直接包含 `index.html` |
| 页面能开但没样式 | 检查 `_astro` 等资源目录是否一起上传；不能只上传 HTML |
| 图片、文章链接失效 | 站点 URL 与实际域名不一致；改完站点 URL 要重新发布再上传 |
| 更新后还是旧内容 | 浏览器缓存，`Ctrl+F5`；或确认新部署状态已是 Ready |
| 想自动化 | 用上面的 wrangler 命令，或接进自己的 CI；Veloct 的「发布」和「上传」本来就是两步 |
| 担心传了不该传的 | Veloct 只复制站点构建产物，不含 `.veloct`、附件池、凭据或源码；上传前可打开 `site\dist\site` 扫一眼 |

## 关于应用内的「Cloudflare Pages（自动）」

不要用它对接真实账号，目前有两个实现问题：

- 表单只有一个「主站项目 / 账号」输入框，而代码把这个值同时当作账号 ID 和项目名，真实项目无法定位；
- 作品子目标的上传地址仍指回主站项目名，启用后有覆盖主站的风险。

见 [已知问题](../../ISSUE.md) 第 10 项。修好之前请用上面的网页或命令行方式；「1Panel（手动上传）」也只是导出提示，不代替上传。

## 官方参考

- [Direct Upload](https://developers.cloudflare.com/pages/get-started/direct-upload/)：拖拽与 wrangler 两条路线。
- [Pages 限制](https://developers.cloudflare.com/pages/platform/limits/)：文件数与单文件大小。
- [自定义域](https://developers.cloudflare.com/pages/configuration/custom-domains/)：域名绑定与 CNAME。
- [wrangler pages 命令](https://developers.cloudflare.com/workers/wrangler/commands/#pages)：全部 `pages` 子命令。
