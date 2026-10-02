/**
 * The dependency-install panel's decisions.
 *
 * Building or publishing a project whose `site/` has no dependencies fails with "no Astro CLI in
 * `<project>/site/node_modules/astro`; run the site's dependency install first". The one-click fix
 * lives on the publish page, and this repository has no React test renderer — deliberately — so the
 * panel's rules are in `src/lib/siteInstall.ts` and are asserted here: what each install state says,
 * whether the button does anything, and why.
 *
 * **What is deliberately NOT here is a copy of any Rust rule.** Whether an install is possible, what
 * npm said, and whether the Astro CLI exists are all decided in `src-tauri/src/state.rs` (covered by
 * its own tests) and arrive in `SiteInstallView`; this file asserts that the window renders that
 * answer, including the one state a user cannot act on their way out of — no system npm at all.
 */

import { describe, expect, it } from 'vitest';

import type { SiteInstallView } from '../src/lib/api';
import {
  describeSiteInstall,
  npmSourceLabel,
  publishBlockedReason,
  siteInstallStateLabel,
} from '../src/lib/siteInstall';

/** One install view, with only the interesting fields overridden. */
function view(overrides: Partial<SiteInstallView>): SiteInstallView {
  return {
    state: { state: 'idle' },
    output_tail: '',
    npm: 'C:/Program Files/nodejs/npm.cmd',
    site_dir: 'D:/blog/site',
    astro_ready: false,
    ...overrides,
  };
}

/** The view of an install whose Astro CLI is on disk. */
function ready(overrides: Partial<SiteInstallView>): SiteInstallView {
  return view({ astro_ready: true, ...overrides });
}

describe('the install panel', () => {
  it('explains a machine with no npm instead of offering a button that must fail', () => {
    const display = describeSiteInstall(view({ npm: null }));

    expect(display.tone).toBe('warning');
    expect(display.title).toContain('没有找到 npm');
    // The two things a user can do about it, both named: install Node.js, or point the override at
    // an npm. The second is the seam a BUNDLED npm would use, so the message stays true either way.
    expect(display.detail).toContain('Node.js');
    expect(display.detail).toContain('VELOCT_NPM');
    expect(display.detail).toContain('npm install');
    expect(display.canStart).toBe(false);
    expect(display.canCancel).toBe(false);
    expect(display.showOutput).toBe(false);
  });

  it('says what a running install is doing, that it can take minutes, and what cancelling costs', () => {
    const display = describeSiteInstall(
      view({
        state: { state: 'running', pid: 4242, started_at: '2026-01-01T00:00:00Z' },
        output_tail: 'npm warn using --no-audit\n',
      }),
    );

    expect(display.tone).toBe('loading');
    expect(display.title).toContain('正在安装');
    // The directory, because "installing in the wrong place" is the failure this feature prevents.
    expect(display.detail).toContain('D:/blog/site');
    expect(display.detail).toContain('npm install');
    // The honest half: cancelling a half-written install is not free.
    expect(display.detail).toContain('半安装');
    expect(display.canStart).toBe(false);
    expect(display.canCancel).toBe(true);
    expect(display.showOutput).toBe(true);
  });

  it('offers the one click where the user meets the missing Astro CLI', () => {
    const display = describeSiteInstall(view({}));

    expect(display.tone).toBe('warning');
    expect(display.title).toContain('还没有安装站点依赖');
    expect(display.detail).toContain('D:/blog/site/node_modules/astro');
    expect(display.actionLabel).toBe('一键安装站点依赖');
    expect(display.canStart).toBe(true);
    expect(display.canCancel).toBe(false);
  });

  it('reports a ready install as ready, and still allows a repair', () => {
    const display = describeSiteInstall(ready({}));

    expect(display.tone).toBe('success');
    expect(display.title).toContain('已就绪');
    expect(display.canStart).toBe(true);
    expect(display.actionLabel).toBe('重新安装依赖');
  });

  it('carries the installer’s own last words and the exit code into the failure', () => {
    const display = describeSiteInstall(
      view({
        state: {
          state: 'failed',
          exit_code: 1,
          finished_at: '2026-01-01T00:05:00Z',
          reason: 'npm install exited with code 1',
        },
        output_tail: 'npm error code EAI_AGAIN\nnpm error network request failed\n',
      }),
    );

    expect(display.tone).toBe('error');
    expect(display.title).toContain('安装失败');
    // The Rust reason, which is what the log keeps too.
    expect(display.detail).toContain('npm install exited with code 1');
    expect(display.detail).toContain('退出码 1');
    // And the raw tail is offered, which is the "installer's own lines" requirement.
    expect(display.showOutput).toBe(true);
    expect(display.canStart).toBe(true);
    expect(display.canCancel).toBe(false);
  });

  it('does not turn an unreportable exit code into a zero', () => {
    // `exit_code: null` means "the process was killed, the code is not known", which is NOT success.
    const display = describeSiteInstall(
      view({
        state: {
          state: 'failed',
          exit_code: null,
          finished_at: '2026-01-01T00:05:00Z',
          reason: 'npm install exited without a reportable exit code',
        },
      }),
    );

    expect(display.detail).toContain('未知');
    expect(display.detail).not.toContain('退出码 0');
  });

  it('mentions a present Astro CLI when a failed install nevertheless left one behind', () => {
    // npm can exit non-zero after installing what the build needs (a failing lifecycle script, for
    // instance). Reporting only "失败" would hide that a publish might work.
    const display = describeSiteInstall(
      ready({
        state: {
          state: 'failed',
          exit_code: 1,
          finished_at: '2026-01-01T00:05:00Z',
          reason: 'npm install exited with code 1',
        },
      }),
    );

    expect(display.tone).toBe('error');
    expect(display.detail).toContain('node_modules/astro 现在已经存在');
  });

  it('does not claim an empty output is worth showing', () => {
    const display = describeSiteInstall(
      view({
        state: {
          state: 'failed',
          exit_code: 1,
          finished_at: '2026-01-01T00:05:00Z',
          reason: 'npm install exited with code 1',
        },
        output_tail: '   \n',
      }),
    );

    expect(display.showOutput).toBe(false);
  });

  it('says a cancel was a cancel, that it is idempotent, and that another install is the repair', () => {
    const display = describeSiteInstall(
      view({
        state: { state: 'cancelled', finished_at: '2026-01-01T00:02:00Z' },
        output_tail: 'npm warn using --no-audit\n',
      }),
    );

    expect(display.tone).toBe('warning');
    expect(display.title).toContain('已取消');
    expect(display.detail).toContain('半安装');
    expect(display.detail).toContain('幂等');
    expect(display.canStart).toBe(true);
    expect(display.showOutput).toBe(true);
  });

  it('does not report a success after the Astro CLI has been deleted', () => {
    // A real sequence: install, then delete `node_modules`. The Rust verdict says "succeeded", the
    // artefact check says otherwise, and the panel must not send the user to a publish that fails.
    const display = describeSiteInstall(
      view({ state: { state: 'succeeded', exit_code: 0, finished_at: 't' } }),
    );

    expect(display.tone).toBe('warning');
    expect(display.detail).toContain('node_modules/astro');
    expect(display.canStart).toBe(true);
  });

  it('confirms a success that the build can actually use', () => {
    const display = describeSiteInstall(
      view({ state: { state: 'succeeded', exit_code: 0, finished_at: 't' }, astro_ready: true }),
    );

    expect(display.tone).toBe('success');
    expect(display.detail).toContain('D:/blog/site/node_modules/astro');
    expect(display.detail).toContain('发布');
  });

  it('decides one display for every state, so no state can fall through unrendered', () => {
    const states: SiteInstallView[] = [
      view({}),
      ready({}),
      view({ state: { state: 'running', pid: 1, started_at: 't' } }),
      view({ state: { state: 'succeeded', exit_code: 0, finished_at: 't' } }),
      view({ state: { state: 'cancelled', finished_at: 't' } }),
      view({
        state: {
          state: 'failed',
          exit_code: 2,
          finished_at: 't',
          reason: 'npm install exited with code 2',
        },
      }),
      view({ npm: null }),
      ready({ npm: null }),
    ];

    for (const install of states) {
      const display = describeSiteInstall(install);
      expect(display.title.length).toBeGreaterThan(0);
      expect(display.detail.length).toBeGreaterThan(0);
      expect(display.actionLabel.length).toBeGreaterThan(0);
      // A cancel button only ever accompanies a running install, and a running install is never
      // startable: the two are what keep a second click from starting a second npm.
      if (display.canCancel) expect(install.state.state).toBe('running');
      if (install.state.state === 'running') expect(display.canStart).toBe(false);
    }
  });
});

describe('what the publish button’s neighbour says', () => {
  it('blocks with the missing CLI named, so the action is where the error is', () => {
    expect(publishBlockedReason(view({}))).toContain('node_modules/astro');
  });

  it('says nothing once the build can find its Astro CLI', () => {
    expect(publishBlockedReason(ready({}))).toBeNull();
  });
});

describe('the install’s one-line status', () => {
  it('names the npm the install will run, or says there is none', () => {
    expect(npmSourceLabel(view({}))).toContain('C:/Program Files/nodejs/npm.cmd');
    expect(npmSourceLabel(view({ npm: null }))).toContain('没有找到 npm');
  });

  it('labels every state distinctly, and reads the artefact rather than the last outcome when idle', () => {
    expect(siteInstallStateLabel(view({ state: { state: 'running', pid: 1, started_at: 't' } }))).toBe(
      '安装中',
    );
    expect(
      siteInstallStateLabel(view({ state: { state: 'succeeded', exit_code: 0, finished_at: 't' } })),
    ).toBe('安装成功');
    expect(siteInstallStateLabel(view({ state: { state: 'cancelled', finished_at: 't' } }))).toBe(
      '已取消',
    );
    expect(
      siteInstallStateLabel(
        view({
          state: { state: 'failed', exit_code: 1, finished_at: 't', reason: 'boom' },
        }),
      ),
    ).toBe('安装失败');
    // Idle reads the DISK, not the previous verdict: after an install finished, a restart of the
    // window shows "依赖已就绪" because that is what the build will find.
    expect(siteInstallStateLabel(view({ state: { state: 'idle' } }))).toBe('尚未安装');
    expect(siteInstallStateLabel(ready({ state: { state: 'idle' } }))).toBe('依赖已就绪');
  });
});
