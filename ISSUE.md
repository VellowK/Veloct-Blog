# ISSUE.md — 未决问题清单（交给下一个 agent）

**这份文件不是契约。** `handoff.md` 是锁定的产品契约、永远优先；`PROCESSING.md` 是状态与取向；
`docs/spec/02-test-matrix.md` 是逐条证据；`UPDATE.md` 是给**测试者**看的本次变更说明。
这份文件回答的是另一个问题：**现在已知哪些东西是坏的、缺的、或没被验证过的，以及每一件的证据在哪里。**

写它的原因：这个仓库最不能容忍的是"看起来做完了"。下面每一条都标了它**为什么**还开着。

---

## P0 — 影响用户、且已经确认是真缺陷

### 1. 已发布的 1.0.0 安装包**无法启动任何预览**

出厂应用接的是 `NullSpawner`（`src-tauri/src/lib.rs`），而工作区里从来没有真实的 OS spawner 实现。
所以 `https://github.com/VellowK/Veloct-Blog/releases/tag/v1.0.0` 上那个包，**"真实 Astro 实时预览"从来没有工作过**。
契约里那一条此前只被**注入假 spawner 的单测**覆盖（§5.8 那一类缺陷），而且**我自己的装机探针也没覆盖它**——
探针是手工启动 sidecar，从未走应用内的预览按钮。

**已修**（`330c07b` 真 `OsSpawner`、`ed519bd` launcher 的静态 `serve` 模式），**测试包已构建并装机验证**。
**未做**：把它作为 1.0.1 发布（业主决定：先多测几轮）。

### 2. 界面：像素未验证，8 个面板未转换

样板（Halo 的结构 + `DESIGN.md` 的视觉 token）只落在**附件页**，并已提交 `f6acc54`。
- **真实像素没有任何人看过**：实现者当时没有浏览器/WebView 可渲染，仓库也没有 jsdom 或无头浏览器（不许加依赖），
  它只用 `react-dom/server` 确认每个分支能渲染。**间距、溢出、窄窗口对比度都需要人眼。**
- 未转换：运行日志、网络图片、作品、备份与恢复、部署、AI 建页、AI 设置、所见即所得编辑器（发布页只加了依赖安装区）。
  它们继承外壳与配色，内部仍是旧表单。

### 3. "一键装依赖"仍假设系统里有 npm

实测事实（不是推测）：内置的 `veloct-node.exe` **只是 node**——`scripts/node-runtime.toml` 只 pin 了一个归档成员，
整个仓库里**没有 npm**；ADR-0005 明写"只发 Node，不发 npm"。所以内置 node **无法**跑 npm。
当前实现：`VELOCT_NPM`（指向真实文件）→ 否则扫 PATH；找不到时按钮禁用并解释原因。

**要真正免前置条件，必须把 npm 打进产物**：约 10 MB JS 树 + `npm.cmd`/`npx` 垫片，
需要新的 pin/hash（`node-runtime.toml` + `fetch-node.ps1`）、Tauri 资源、安装包与签名清单的更新。
**这是打包决策，属于业主**；代码侧的接缝只有 `resolve_npm` 的第一个分支。

---

## P1 — 已知弱点，有明确修法

| # | 问题 | 修法 / 位置 |
|---|---|---|
| 4 | `dist/works` 缺失时，用户只看到 supervisor 的泛化消息（`exited before it became ready on port N`），launcher 那句**可执行的 stderr 被丢掉** | `PreviewSupervisor::start` 的 exit-before-ready 分支没有像 `poll()` 那样附上 `output_tail()`——在 `crates/veloct-process` |
| 5 | 安装依赖过程中**关闭窗口**会正确杀掉进程树，但**不像取消那样**警告 `node_modules` 可能半装 | 需要一个关闭确认 |
| 6 | launcher 的 `serve` 模式**只在 Windows 上验证过**；Unix 分支的 `kill_process_tree` 只是给直接子进程发 SIGTERM（既有实现） | 要么在 Unix 上验证，要么写明不支持 |
| 7 | `npm install` 用的是**系统 npm/node**（不是 pinned `veloct-node.exe`），很旧的 npm 会报它自己的错；`resolve_npm` 扫 PATH，**不在 app PATH 上的 npm 会被判为"没有"** | 已由 `VELOCT_NPM` 缓解，未消除 |
| 8 | `-Check` 的**逐字节**比对对"重建是否确定"很敏感。一位实现者报告过非确定性（PE 时间戳/PDB GUID），**我在同一棵树上三次运行都未复现**（两次强制重建哈希完全一致、跑完整套件后 `-Check` 仍绿） | 记为"报告过、未复现"。若再遇到，先量差异字节再改检查 |

---

## P2 — 卡在外部输入或需要业主决定

| # | 事项 | 卡在 |
|---|---|---|
| 9 | **Lsky Pro 从未连过真实实例**（§13.12 的适配器按厂商产品页实现、有单测，但没有 `live_lsky.rs`） | 一个实例地址 + API token |
| 10 | **Cloudflare / EdgeOne** 部署目标从未联调 | 账号 |
| 11 | **正式 CA 证书** | 购买。自签名证书只在本机装了 `.cer` 后显示发布者名；新机器上仍是未知发布者 |
| 12 | **轮换 `Git.md` 里的 fine-grained PAT** | 它在本会话中被读出使用过（从未打印、从未进仓库，`git grep`/`git log -S` 均 0 命中），但应轮换 |
| 13 | **发布流程**：本地有 5+ 个提交**未推送**（远端只有到 `c588c57` + tag `v1.0.0`）；远端默认分支仍是 `main`（其上有一个与本地无关的初始提交），本地的 `master` 已推送但未设为默认 | 业主决定是否改默认分支、以及 1.0.1 何时发 |
| 14 | 剩下的**英文诊断**在中文界面里（最要紧的是 S3 上传失败时用户看到的那句 `veloct-s3::refused()`），另有约十个校验器 | 受控改动，逐条可验证；清单在各轮提交信息里 |

---

## 交接须知：这个仓库的工作方式（比上面任何一条都重要）

1. **`handoff.md` 永远优先**，其次是 `PROCESSING.md` 的状态、`docs/spec/02-test-matrix.md` 的证据。
2. **先验证再声称。** 每条"已完成"都要有**命令 + 数字**；矩阵里 `I` 行必须同时给出测试名与命令。
3. **对着假实现断言 ≠ 验证。** 本会话最贵的三个缺陷（预览 spawner、作品预览命令、`x-amz-date` 亚秒）
   全都是"单测用假的、真实路径从未被碰过"。**接线缺陷要有接线测试。**
4. **记录失败的那一次运行，不要替换它。** 失败并被诊断的记录比只有成功的记录有价值。
5. **绝不 `git add -A`。** 本会话它把业主的 `DESIGN.md` 扫进了一个无关提交，导致提交信息与内容不符——
   改成逐路径暂存。
6. **提交信息必须与内容一致**；发现不一致时，**拆成两个提交各自说真话**，而不是改写历史。
7. **未复现的结论不要写成事实。** 标注"报告过、我这里未复现"。
8. 命令：`scripts/cargo.ps1 test --workspace --no-fail-fast`、`… clippy --workspace --all-targets`、
   `scripts/build-sidecar.ps1 [-Check]`、`scripts/build-installer.ps1`（签名，自动还原被就地签名的 launcher）、
   `scripts/sign-artifacts.ps1`、`cmd /c "npm.cmd run test:ui"`、`cmd /c "npm.cmd run build:ui"`、
   `site/` 内 `npm.cmd run build` + `node scripts/verify-build.mjs`。发布顺序见 `PROCESSING.md` §2.15。
9. 这台机器上：`github.com` 被 hosts 指到 127.0.0.1 → git/HTTP 需要显式代理 `http://127.0.0.1:10818`；
   控制台显示 `§` 为 `搂`，**要按码点判断**（`U+00A7` vs `U+6402`），别凭肉眼。

---

## 当前的测试构建（给业主装机用）

```
D:\Veloct-Blog\target\release\bundle\nsis\Veloct_1.0.0_x64-setup.exe
29,425,272 字节（28.06 MiB）
SHA-256 EC33D1ACF1CD60ABEC537FD6730833ADC228F448FF0E7FD5A030E4F2370C11DE
签名 Valid（CN=VellowChire Code Signing, O=Veloct），本机当前用户已信任该证书
装机验证：四个落地文件全部 Valid；装机 launcher 起真实 Astro，http://localhost:4399/ 返回 200；
进程树杀后端口释放；卸载干净
```

**版本号故意仍是 1.0.0**（业主决定先测几轮再发 1.0.1）。构建与签名过程见 `PROCESSING.md` §2.15；
变更内容见 `UPDATE.md`。
