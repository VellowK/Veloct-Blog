import { test, expect } from 'vitest';
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { stageTemplate } from './prepare-template.mjs';

test('staging allows source only and supplies neutral data, never developer files', () => {
  const root = mkdtempSync(join(tmpdir(), 'veloct-template-'));
  try {
    for (const file of ['package.json', 'package-lock.json', 'astro.config.mjs', '.gitignore',
      'src/pages/index.astro', 'src/styles/global.css', 'scripts/verify-build.mjs',
      'src/.env', 'src/secret.ts', 'src/cache/leak.ts', 'src/.cache/leak.ts',
      'node_modules/secret.js', 'dist/output.js', '.astro/cache.js', '.veloct-generated/index.json']) {
      const path = join(root, 'site', file);
      mkdirSync(join(path, '..'), { recursive: true });
      writeFileSync(path, 'fixture');
    }
    mkdirSync(join(root, 'data'));
    writeFileSync(join(root, 'data/site.json'), '{"token":"DO_NOT_SHIP"}');
    const output = stageTemplate(root, join(root, 'staged'));
    expect(existsSync(join(output, 'site/src/pages/index.astro'))).toBe(true);
    expect(existsSync(join(output, 'site/scripts/verify-build.mjs'))).toBe(true);
    for (const path of ['node_modules', 'dist', '.astro', '.veloct-generated', 'src/.env', 'src/secret.ts', 'src/cache', 'src/.cache']) {
      expect(existsSync(join(output, 'site', path))).toBe(false);
    }
    expect(readFileSync(join(output, 'data/site.json'), 'utf8')).not.toContain('DO_NOT_SHIP');
    expect(JSON.parse(readFileSync(join(output, 'data/links.json'), 'utf8'))).toEqual([]);
    writeFileSync(join(output, 'stale-secret.txt'), 'old');
    stageTemplate(root, output);
    expect(existsSync(join(output, 'stale-secret.txt'))).toBe(false);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
