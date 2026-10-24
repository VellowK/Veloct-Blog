# 把 Veloct 网站部署到 GitHub Pages

当前 Veloct 没有内置的 **GitHub Pages 自动部署目标**，但可以把本地生成的静态网站上传到 GitHub Pages。这个教程不用配置 API Token，也不需要编译 Veloct 程序。

先走最简单的「个人主页站」方式：网址为 `https://你的用户名.github.io/`，不带仓库子路径。本教程只部署主站。

## 1. 先找到真正的网站文件

在 Veloct 打开你的网站项目，点击「发布」，等待构建结束。

**1.0.1 的实际产物位置：**

```text
<你的网站项目目录>\site\dist\site\index.html
<你的网站项目目录>\site\dist\site\_astro\
```

作品内容则在 `<项目目录>\site\dist\works`，不要把两套输出混到一起。

注意中间有一个 `site`：项目根目录的 `dist` 当前可能是空的，不能拿它上传。`.veloct/releases/` 是发布记录，也不是网站目录。

如果 `site\dist\site\index.html` 也没有，先看发布页的错误和「查看构建输出」，把日志排查完再继续。

## 2. 新建一个专门放网站的仓库

1. 登录 GitHub，右上角 **+ → New repository**。
2. Repository name 必须填 **`你的GitHub用户名.github.io`**。例如用户名为 `VellowK`，就填 `VellowK.github.io`。
3. 选 **Public**，创建仓库。GitHub Free 的免费 Pages 使用公开仓库。
4. 如果这个仓库已经存在并托管了旧网站，**先不要覆盖**，先备份或另行规划域名。

这是你的网站仓库，**不是 Veloct 程序源码仓库 `Veloct-Blog`**。不要把私人 Markdown 真源、`.veloct`、凭据、整个项目或 `node_modules` 上传进去。

不要先换成任意仓库名：那会变成 `https://用户名.github.io/仓库名/`，涉及 `base` 和站内绝对链接适配，不能保证当前模板直接可用。

## 3. 设置网站网址，再发布一次

1. Veloct →「外观」→「站点信息」→「信息」。
2. 把「站点 URL」设置为 `https://你的用户名.github.io`，保存。
3. 再点「发布」，使用这次新生成的 `site\dist\site`。

不要把 URL 填成 GitHub 仓库浏览页 `https://github.com/用户名/仓库名`。

## 4. 上传网站内容

1. 进入新建的网站仓库，在空仓库的提示里选 **uploading an existing file**；已有文件的仓库选 **Add file → Upload files**。
2. 打开本地 `site\dist\site`，上传**里面的内容**，包括 `index.html`、`_astro` 和各页面目录。
3. 点 **Commit changes**，提交到网站仓库的主分支（新建仓库通常叫 `main`）。

完成后仓库根目录应该直接看到 `index.html` 和 `_astro`，不能再套一层 `site/`。

GitHub 网页上传有每次文件数和单文件大小限制。文件太多时可分批上传，或改用 Git/GitHub Desktop。重新上传不会自动删除旧资源；经常更新时建议用 Git 同步构建输出，但不要为清理文件误删原有网站。

## 5. 添加 `.nojekyll`

这一步不能漏：Astro 的 `_astro` 目录需要原样发布。

1. 在网站仓库点 **Add file → Create new file**。
2. 文件名填 `.nojekyll`（开头有一个英文句点，后面没有 `.txt`）。
3. 内容可写一行 `Disable Jekyll for this prebuilt static site.`，然后 **Commit changes**。

每次更新保留这个文件，它不需要由 Veloct 构建产生。

## 6. 开启 Pages

1. 进入**网站仓库**的 **Settings**。
2. 左侧点 **Pages**。
3. **Build and deployment → Source** 选择 **Deploy from a branch**。
4. **Branch** 选择刚刚上传文件的分支，例如 `main`；文件夹选 **`/ (root)`**。
5. 点 **Save**。
6. 到 **Actions** 查看 Pages 部署任务，等它变绿后访问 `https://你的用户名.github.io/`。

第一次部署可能需要几分钟。若失败，先看 Actions 日志，不要只反复刷新网站。

## 7. 以后怎么更新

Veloct 修改内容 →「发布」→ 把新的 `site\dist\site` 内容提交到**网站仓库**同一个分支。GitHub Pages 会自动重新部署；这不等于 Veloct 自动推送，上传仍需你完成。

不要上传整个 `site` 源码目录，也不要把 Veloct 程序的安装包当网站上传。

## 常见问题

| 现象 | 先检查 |
|---|---|
| 根目录 `dist` 为空 | 找 `site\dist\site`，当前版本实际输出在那里 |
| Pages 地址 404 | 仓库名是否严格为 `用户名.github.io`；分支是否正确；根目录是否有 `index.html`；Actions 是否成功 |
| 没样式、图片不显示 | `_astro` 等资源是否完整；根目录 `.nojekyll` 是否存在；是否误用了带 `/仓库名/` 的网址 |
| 分享卡片或 sitemap 域名不对 | Veloct 的「站点 URL」改好后是否重新发布、上传 |
| 如何部署作品 | 作品需要不同 origin 隔离；同一 `用户名.github.io` 下换一个路径并不隔离。先部署主站，作品另用独立域名或 Cloudflare 项目，参考 [Cloudflare 指南](<cloudflare-pages.md>) |

## 官方参考

- [配置 GitHub Pages 发布源](<https://docs.github.com/en/pages/getting-started-with-github-pages/configuring-a-publishing-source-for-your-github-pages-site>)：分支、根目录、`.nojekyll` 和 Actions 的说明。
- [创建个人 GitHub Pages 站点](<https://docs.github.com/en/pages/getting-started-with-github-pages/creating-a-github-pages-site>)：仓库命名规则。
