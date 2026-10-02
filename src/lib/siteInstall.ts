/**
 * The dependency-install panel's decisions, as a pure module.
 *
 * The bulk of the publish panel is rules — what the install is doing, what to say about it, whether
 * the button does anything — and a rule that lives in JSX is a rule nothing checks: this repository
 * has no React test renderer, so every decision the panel makes is here and is covered by
 * `tests/siteInstallForm.test.ts` (`PROCESSING.md` §5.10's pattern for `workForm`, `networkImageForm`
 * and `storageTargetForm`).
 *
 * **Why this exists at all.** Building or publishing a project whose `site/` has no dependencies
 * fails with "no Astro CLI in `<project>/site/node_modules/astro`; run the site's dependency install
 * first (npm install in site/)". That message is correct and actionable and still leaves the user in
 * a console. The fix is one click — and a click needs somewhere to live, which is where the user
 * MEETS the error: the publish page. Everything here is in Chinese, like the rest of the panel.
 *
 * **What it deliberately does not decide** is whether an install is possible at all — the Rust side
 * answers that (`SiteInstallView::npm` is null when no system npm exists, and the command itself
 * refuses with an actionable hint). This module only renders the answer.
 */

import type { SiteInstallView } from './api';
import type { StateTone } from '../components/ui';

/** Everything the panel needs in order to render the install state, decided once. */
export interface SiteInstallDisplay {
  tone: StateTone;
  title: string;
  detail: string;
  /** Whether the installer's own output should be shown under the detail. */
  showOutput: boolean;
  /** The install button's label. */
  actionLabel: string;
  /** Whether the install button does anything right now. */
  canStart: boolean;
  /** Whether a cancel button should be offered. */
  canCancel: boolean;
}

/** Which npm the install will run, in one line, or that there is none. */
export function npmSourceLabel(view: SiteInstallView): string {
  return view.npm ? `安装会使用 ${view.npm}` : '这台机器上没有找到 npm';
}

/**
 * What to say next to the publish button when the build cannot find its Astro CLI.
 *
 * `null` when it can. The answer comes from `astro_ready`, which the Rust side computes from
 * `veloct_sidecar::astro_entry_in` — the very check the build makes — so this cannot disagree with
 * what a publish will do.
 */
export function publishBlockedReason(view: SiteInstallView): string | null {
  if (view.astro_ready) return null;
  return '发布前要先装好站点依赖:构建在 site/node_modules/astro 找不到 Astro CLI。';
}

/**
 * The install's state as one line of Chinese, for the panel's heading area.
 *
 * Separate from [`describeSiteInstall`] because the two answer different questions: this one is
 * "where is it", the other is "what does the user need to know and do".
 */
export function siteInstallStateLabel(view: SiteInstallView): string {
  switch (view.state.state) {
    case 'idle':
      return view.astro_ready ? '依赖已就绪' : '尚未安装';
    case 'running':
      return '安装中';
    case 'succeeded':
      return '安装成功';
    case 'failed':
      return '安装失败';
    case 'cancelled':
      return '已取消';
  }
}

/** `退出码 1`, `退出码 未知`, and nothing at all when the code is not the point. */
function exitCodeText(code: number | null): string {
  return `退出码 ${code === null ? '未知(进程被信号终止)' : code}`;
}

/**
 * What the panel shows for the install.
 *
 * The order of the branches is the order of the questions a user has: is a system npm here at all;
 * is it running; how did it end — and each terminal state says what to DO next, because "failed" and
 * "cancelled" have different repairs even though both end with the same button.
 */
export function describeSiteInstall(view: SiteInstallView): SiteInstallDisplay {
  const output = view.output_tail.trim().length > 0;

  // No npm, so nothing here can work. This is the one case where the button is disabled rather than
  // offered: the Rust side refuses with an actionable hint, and a button that always fails is worse
  // than an explanation. npm is not bundled — Veloct ships the Node runtime only — so this is a real
  // state on a machine without Node.js, not a misconfiguration.
  if (view.npm === null) {
    return {
      tone: 'warning',
      title: '没有找到 npm',
      detail:
        '一键安装需要在 site/ 里运行 npm install。Veloct 自带的是 Node 运行时,并没有自带 npm,所以这一步要用系统的 npm:装一个 Node.js(它自带 npm),或者把 npm 的路径写进环境变量 VELOCT_NPM。',
      showOutput: false,
      actionLabel: '一键安装站点依赖',
      canStart: false,
      canCancel: false,
    };
  }

  switch (view.state.state) {
    case 'running':
      return {
        tone: 'loading',
        title: '正在安装站点依赖…',
        detail: `正在 ${view.site_dir} 里运行 npm install。这一步可能要几分钟;期间可以取消,取消会终止 npm 及其子进程,node_modules 可能停在半安装状态(再装一次就能修好)。`,
        showOutput: output,
        actionLabel: '安装中…',
        canStart: false,
        canCancel: true,
      };

    case 'succeeded':
      // A success the Rust side reports already means `node_modules/astro` exists. The belt-and-
      // braces branch is for the real case where it was DELETED after the install: reporting "已就绪"
      // then would send the user to a publish that fails with the message this feature exists to fix.
      return view.astro_ready
        ? {
            tone: 'success',
            title: '站点依赖已就绪',
            detail: `npm install 已成功退出,${view.site_dir}/node_modules/astro 已就位,现在可以发布。`,
            showOutput: output,
            actionLabel: '重新安装依赖',
            canStart: true,
            canCancel: false,
          }
        : {
            tone: 'warning',
            title: '依赖装过,但 Astro CLI 又不见了',
            detail: `上次安装是成功的,但现在找不到 ${view.site_dir}/node_modules/astro——它可能被删掉了。再安装一次即可。`,
            showOutput: output,
            actionLabel: '重新安装依赖',
            canStart: true,
            canCancel: false,
          };

    case 'failed':
      return {
        tone: 'error',
        title: '站点依赖安装失败',
        detail: `${view.state.reason}(${exitCodeText(view.state.exit_code)})。下面是安装器自己的最后几行,失败的原因通常就在里面。${
          view.astro_ready
            ? '不过 site/node_modules/astro 现在已经存在,构建可能仍然可用。'
            : ''
        }`.trim(),
        showOutput: output,
        actionLabel: '重新安装依赖',
        canStart: true,
        canCancel: false,
      };

    case 'cancelled':
      return {
        tone: 'warning',
        title: '已取消安装',
        detail:
          '已终止 npm 及其子进程。node_modules 可能停在半安装状态——再运行一次安装即可修复,这一步是幂等的。',
        showOutput: output,
        actionLabel: '重新安装依赖',
        canStart: true,
        canCancel: false,
      };

    case 'idle':
      return view.astro_ready
        ? {
            tone: 'success',
            title: '站点依赖已就绪',
            detail: `已经能在 ${view.site_dir}/node_modules/astro 找到 Astro CLI,可以直接发布。依赖坏掉时可以重新安装一次。`,
            showOutput: output,
            actionLabel: '重新安装依赖',
            canStart: true,
            canCancel: false,
          }
        : {
            tone: 'warning',
            title: '还没有安装站点依赖',
            detail: `构建和发布会去找 ${view.site_dir}/node_modules/astro。点下面的按钮在 site/ 里运行 npm install,不用打开控制台。`,
            showOutput: output,
            actionLabel: '一键安装站点依赖',
            canStart: true,
            canCancel: false,
          };
  }
}
