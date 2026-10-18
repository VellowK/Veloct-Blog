import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'vitest';
const read = path => readFileSync(new URL('../' + path, import.meta.url), 'utf8');
const pkg = JSON.parse(read('package.json'));

test('desktop, template and npm lockfile root versions agree', () => {
  assert.match(pkg.version, /^\d+\.\d+\.\d+$/);
  assert.equal(JSON.parse(read('src-tauri/tauri.conf.json')).version, pkg.version);
  for (const prefix of ['', 'site/']) {
    const manifest = JSON.parse(read(prefix + 'package.json'));
    const lock = JSON.parse(read(prefix + 'package-lock.json'));
    assert.equal(manifest.version, pkg.version, prefix + 'manifest');
    assert.equal(lock.version, pkg.version, prefix + 'lockfile');
    assert.equal(lock.packages[''].version, pkg.version, prefix + 'lockfile root');
  }
});

test('npm dependency versions continue to match their resolved registry tarballs', () => {
  for (const prefix of ['', 'site/']) {
    const lock = JSON.parse(read(prefix + 'package-lock.json'));
    for (const [name, entry] of Object.entries(lock.packages)) {
      if (entry.resolved?.includes('/-/') && entry.resolved.endsWith('.tgz')) {
        assert.ok(entry.resolved.endsWith('-' + entry.version + '.tgz'), prefix + name);
      }
    }
  }
});

test('Rust workspace and every internal locked crate use the desktop release version', () => {
  const workspaceVersion = read('Cargo.toml').match(/\[workspace\.package\]\s*version\s*=\s*"([^"]+)"/)?.[1];
  assert.equal(workspaceVersion, pkg.version);
  const internal = [...read('Cargo.lock').matchAll(/\[\[package\]\]\s*name = "(veloct-[^"]+)"\s*version = "([^"]+)"/g)];
  assert.equal(internal.length, 17);
  for (const [, name, version] of internal) assert.equal(version, pkg.version, name);
});
