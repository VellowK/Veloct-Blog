# 把 Veloct 网站部署到 Cloudflare Pages

这份文档面向第一次配置部署的人：不解释 Cloudflare 的全部功能，只走通「Veloct 生成网站 → Cloudflare 托管 → 绑定自己的域名」这一条路。

先明确分工：

- **Veloct 负责生成文件**。点「发布」后，网站产物是本地两个文件夹：项目目录下的 `dist/site`（主站）和 `dist/works`（作品站）。
- **Cloudflare 负责托管文件**。你把上面两个文件夹传上去，它就提供免费 HTTPS 网址。

Veloct 不会自动上传，Cloudflare 也不会来读你的电脑。下面两条上传路线选一条即可。

---

## 一、先做好发布前的两件事

### 1. 填对「站点 URL」

打开 Veloct → 左侧「外观」→「站点信息」→「信息」分区，把 **站点 URL** 改成你最终要用的地址，例如 `https://blog.example.com`。

这个值会写进页面的 canonical、分享卡片和 sitemap。先写 `example.com` 再上线，搜索引擎拿到的就是示例域名，后面还得重发一次。

### 2. 点「发布」并确认产物

左侧「发布」→ 点 **发布**。发布成功后会生成：

```text
<你的项目目录>\dist\site\      ← 主站，部署用这个
<你的项目目录>\dist\site\index.html
<你的项目目录>\dist\works\     ← 作品站，只有导入过作品才存在
```

只要 `dist\site\index.html` 存在，就可以上传了。**不要上传整个 `dist` 目录**，那样会把作品站也塞进主站项目。

---

## 二、路线 A：网页拖拽上传（不需要命令行）

适合偶尔更新、不想装工具的人。

1. 登录 [Cloudflare 控制台](https://dash.cloudflare.com/)，左侧选 **Workers & Pages**。
2. 点 **Create application**（创建应用），切到 **Pages** 标签页，选择 **Drag and drop your files**（拖拽上传资产）。
3. 填一个项目名，例如 `veloct-blog`。它决定你的默认网址：`https://veloct-blog.pages.dev`。项目名一旦创建，后期改名会换网址。
4. 把 `dist\site` 文件夹**里面的内容**拖进上传框（进入该文件夹后全选，或直接拖文件夹）。确认页面列表里第一层是 `index.html`、`assets`、`about` 等，而不是一个叫 `site` 的文件夹。
5. 点 **Deploy site**，等一两分钟，打开 `https://<项目名>.pages.dev` 检查首页、文章页和图片。

以后再更新：进入这个 Pages 项目 → **Create a new deployment** → 重新拖一次新的 `dist\site`。

> 拖拽方式一次最多 1000 个文件、单文件 25 MiB（[官方限制](https://developers.cloudflare.com/pages/platform/limits/)）。个人博客通常远低于这个量；超了就用路线 B。

## 三、路线 B：wrangler 命令行（适合经常更新）

Veloct 本身就要求 Node.js 22 以上，所以这台机器已经具备条件。

```powershell
# 首次：登录（会打开浏览器授权）
npx.cmd wrangler login

# 首次：创建 Pages 项目
npx.cmd wrangler pages project create veloct-blog

# 每次更新：上传主站（把路径换成你的项目目录）
npx.cmd wrangler pages deploy "D:\path\to\your-project\dist\site" --project-name veloct-blog
```

命令结束会打印一个 `https://<哈希>.veloct-blog.pages.dev` 的预览地址，你的正式地址始终是 `https://veloct-blog.pages.dev`。

wrangler 方式上限是 20000 个文件、单文件 25 MiB，并且**只接受文件夹，不接受 zip**。

---

## 四、绑定自己的域名

1. 打开该 Pages 项目 → **Custom domains**（自定义域）→ **Set up a custom domain**。
2. 输入域名，例如 `blog.example.com`，按提示确认。
3. 如果这个域名已经托管在同一个 Cloudflare 账号下，DNS 记录会自动加好；如果域名在别处，需要到你的 DNS 服务商添加一条 **CNAME**，指向 `<项目名>.pages.dev`。
4. 等状态变成 **Active**（证书签发通常几分钟）。期间访问可能报证书错误，属正常。

**绑定成功后回到 Veloct**：把「站点 URL」改成 `https://blog.example.com`，重新发布，再上传一次。否则 sitemap 和分享卡片仍指向 `pages.dev` 地址。

## 五、作品站（用了作品功能才需要）

作品是第三方静态站点，必须和主站分开在不同域名下运行，这是 Veloct 的安全边界，不是可选项。

1. 用路线 A 或 B **再建一个 Pages 项目**，例如 `veloct-works`，上传的是 `dist\works`。
2. 给它绑定一个子域名，例如 `works.example.com`，等状态 Active。
3. 打开项目目录下的 `data\site.json`，把这一行改成你的作品域名：

   ```json
   "worksOrigin": "https://works.example.com",
   ```

4. 回到 Veloct 重新「发布」并上传主站。作品页里的链接才会指向真实地址；不填的话页面会显示 `works.example.com` 这类示例值。

> 部署设置里的「作品源（独立 origin）」只用于记录和结果展示，不改变构建产物；真正决定作品链接的是 `data/site.json` 的 `worksOrigin`。

---

## 六、常见问题

| 现象 | 原因与处理 |
|---|---|
| 打开是 404 或 "Nothing is here yet" | 上传的层级错了：应该是 `dist\site` 的内容，而不是 `dist` 或 `dist\site` 这一层文件夹本身 |
| 页面能开但没有样式 | 只传了 HTML，`assets` 目录没上传；重新完整上传 |
| 图片、文章链接失效 | 站点用的是绝对地址，站点 URL 与实际域名不一致；改站点 URL 后重新发布上传 |
| 更新后仍是旧内容 | Cloudflare 有缓存；刷新时用 `Ctrl+F5`，或在项目里确认新部署已经 Ready |
| 想自动化、不想手动传 | 用路线 B 的 wrangler 命令，或把它放进自己的 CI；Veloct 的发布与部署本来就是分开的两步 |
| 担心传了不该传的东西 | Veloct 只复制站点构建产物，不含 `.veloct`、附件池、凭据或源码；上传前仍建议打开 `dist\site` 扫一眼 |

## 七、关于应用内的「Cloudflare Pages（自动）」目标

Veloct 的部署面板里有这一项，但**目前不要用它对接真实 Cloudflare 账号**：

- 表单只有一个「主站项目 / 账号」输入框，而代码把这个值同时当作账号 ID 和项目名，真实项目无法被定位；
- 作品子目标的上传地址也仍然指回主站项目名，一旦启用会覆盖主站。

这两点属于尚未与真实账号联调的部分（见 [已知问题](../../ISSUE.md) 第 10 项）。在修好之前，请用上面的路线 A 或 B 上传；应用内的「1Panel（手动上传）」同样只是导出提示，不代替上传。

## 八、官方参考

- [Direct Upload](https://developers.cloudflare.com/pages/get-started/direct-upload/)：拖拽与 wrangler 两条上传路线。
- [Pages 限制](https://developers.cloudflare.com/pages/platform/limits/)：文件数、单文件大小。
- [自定义域](https://developers.cloudflare.com/pages/configuration/custom-domains/)：域名绑定与 CNAME。
- [wrangler pages 命令](https://developers.cloudflare.com/workers/wrangler/commands/#pages)：所有 `pages` 子命令。
