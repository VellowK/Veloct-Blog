/**
 * The 联系方式 platform catalogue.
 *
 * The catalogue is the CONTRACT between the desktop form and the site template: the desktop writes a
 * platform `key` into `data/site.json` as a contact's `icon`, and the site looks that key up to draw
 * a glyph. A key that exists on one side and not the other is a contact that renders as nothing,
 * which is why the checks below are about the catalogue's own shape rather than about pixels.
 *
 * This file is `.mjs` rather than `.ts` for the same reason `siteInformation.test.mjs` is: it reads
 * files from disk with Node's own API, and the desktop `tsconfig.json` includes `tests/`, so a `.ts`
 * test would be type-checked by `build:ui` without `@types/node` present to describe `node:fs`.
 *
 * The catalogue lives under `site/src/lib/` and BOTH sides import that one file, so there is no
 * second copy to compare — the copies this repository usually has to hold together with a
 * transcription test do not exist here. What is left to prove is that the site really does read the
 * shared module (rather than a private list that could drift), and that the one entry with no
 * published icon is handled instead of drawn as an empty box.
 */

import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'vitest';
import {
  SOCIAL_PLATFORMS,
  SOCIAL_PLATFORM_KEYS,
  findSocialPlatform,
} from '../site/src/lib/social-platforms';

const SITE = new URL('../site/src/', import.meta.url);

test('every platform key is unique and safe to persist in site.json', () => {
  assert.equal(new Set(SOCIAL_PLATFORM_KEYS).size, SOCIAL_PLATFORM_KEYS.length, 'keys must be unique');
  assert.deepEqual(SOCIAL_PLATFORM_KEYS, SOCIAL_PLATFORMS.map((platform) => platform.key));
  for (const platform of SOCIAL_PLATFORMS) {
    // A key is written into JSON and read back by the template, so it stays lowercase and bare: no
    // spaces, no punctuation, nothing that would need escaping on either side.
    assert.match(platform.key, /^[a-z][a-z0-9-]*$/, `unexpected key ${platform.key}`);
    assert.ok(platform.label.trim().length > 0, `${platform.key} has no label to show`);
    assert.ok(platform.urlHint.trim().length > 0, `${platform.key} has no url hint`);
    assert.ok(['domestic', 'international'].includes(platform.region), `${platform.key} has no region`);
  }
  // The picker offers the two groups separately, so one of them being empty would render as an
  // unexplained empty row rather than as a missing feature.
  for (const region of ['domestic', 'international']) {
    assert.ok(SOCIAL_PLATFORMS.some((platform) => platform.region === region), `no ${region} platform`);
  }
});

test('every embedded icon path is a complete, well-formed SVG path', () => {
  let withIcon = 0;
  for (const platform of SOCIAL_PLATFORMS) {
    if (platform.path === '') continue;
    withIcon += 1;
    const path = platform.path;
    assert.match(path, /^[Mm]/, `${platform.key} does not start with a move command`);
    // The whole SVG path grammar: commands, numbers, separators. Anything else — an HTML fragment, a
    // truncated escape, a stray quote — means the path was not copied out of the SVG intact, which
    // is the exact failure a literal this long invites.
    assert.match(path, /^[MmLlHhVvCcSsQqTtAaZz0-9eE.,+\-\s]+$/, `${platform.key} has characters outside the SVG path grammar`);
    assert.ok(path.length > 20, `${platform.key} is too short to be a 24x24 glyph`);
    // Every Simple Icons path ends on a coordinate or on a close-path command (measured: 25/25). A
    // path that stops after a command letter — `…M12 .297c` — is the shape a careless copy produces,
    // and the grammar check above cannot see it because every one of those characters is legal.
    assert.match(path, /[0-9zZ]$/, `${platform.key} does not end on a coordinate or a close-path, so it looks truncated`);
    assert.doesNotMatch(path, /undefined|NaN/, `${platform.key} contains a substituted value`);
  }
  // A catalogue that silently lost every path would pass the loop above; this is the non-vacuity check.
  assert.ok(withIcon >= 20, `only ${withIcon} platforms carry an icon`);
});

test('the platforms the requirement named are present', () => {
  for (const key of ['github', 'qq', 'wechat', 'bilibili', 'zhihu', 'sinaweibo', 'xiaohongshu']) {
    assert.ok(SOCIAL_PLATFORM_KEYS.includes(key), `${key} is missing from the catalogue`);
  }
});

test('an absent, hand-written or unknown icon resolves to no platform rather than to a wrong one', () => {
  // `changePlatform` clears `icon` to '' when the user picks 自定义, so the empty string is a real
  // stored value and must not be mistaken for a platform.
  for (const value of ['', '   ', 'not-a-platform', null, undefined]) {
    assert.equal(findSocialPlatform(value), undefined, `resolved ${JSON.stringify(value)}`);
  }
  assert.equal(findSocialPlatform('github')?.label, 'GitHub');
});

test('抖音 is kept without an invented icon, so the fallback branch is not dead code', () => {
  const douyin = findSocialPlatform('douyin');
  assert.ok(douyin, 'douyin must stay in the catalogue: saved contacts already name it');
  assert.equal(douyin.path, '', 'no 抖音 icon is published by Simple Icons, so none may be substituted');
  assert.equal(douyin.label, '抖音');
});

test('the site reads the shared catalogue and draws a letter when there is no icon', () => {
  // The real risk this closes is a private list on the site side: it would render today and diverge
  // from the picker the first time either side changed.
  const icon = readFileSync(new URL('components/SocialIcon.astro', SITE), 'utf8');
  assert.match(icon, /from '\.\.\/lib\/social-platforms'/, 'SocialIcon must import the shared catalogue');
  assert.match(icon, /platform\.path \?/, 'SocialIcon must branch on a platform that has no icon');
  assert.match(icon, /platform\.label\.slice\(0, 1\)/, 'the no-icon branch must draw the label, not a blank');

  const sidebar = readFileSync(new URL('components/LeftSidebar.astro', SITE), 'utf8');
  assert.match(sidebar, /import SocialIcon from '\.\/SocialIcon\.astro'/, 'the sidebar must use the glyph component');
  assert.match(sidebar, /findSocialPlatform\(entry\.icon\)/, 'the sidebar label must fall back to the platform name');
  // A contact the catalogue does not know must still be rendered: dropping it would remove the user's
  // own data from the page without saying so.
  assert.match(sidebar, /entry\.url \? \(/);
});

test('the desktop form picks from the same catalogue it writes', () => {
  const panel = readFileSync(new URL('../src/SiteSettingsPanel.tsx', import.meta.url), 'utf8');
  assert.match(panel, /from '\.\.\/site\/src\/lib\/social-platforms'/, 'the form must import the shared catalogue');
  assert.match(panel, /icon: platform\.key/, 'adding a platform must persist its key');
  // The label is only filled in when the user has not written one, so picking a platform cannot
  // overwrite a name the user chose.
  assert.match(panel, /contact\.label\.trim\(\) \? contact\.label : platform\?\.label/, 'a chosen label must not be overwritten');
});
