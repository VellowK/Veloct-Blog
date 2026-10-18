// Only source allowlists enter the installer: never copy a developer project wholesale.
import { copyFileSync, existsSync, lstatSync, mkdirSync, readdirSync, rmSync, writeFileSync } from 'node:fs';
import { dirname, extname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const sourceExtensions = new Set(['.astro', '.mjs', '.js', '.ts', '.css']);
const blocked = /^(?:\.|node_modules$|dist$|build$|cache$|.*(?:secret|token|credential).*|.*\.test\.[^.]+$)/i;

export function stageTemplate(repoRoot = resolve(here, '..'), output = join(repoRoot, 'src-tauri', 'resources', 'project-template')) {
  rmSync(output, { recursive: true, force: true });
  mkdirSync(join(output, 'site'), { recursive: true });
  function copy(relative) {
    const source = join(repoRoot, 'site', relative);
    if (lstatSync(source).isSymbolicLink()) throw new Error(`Template links are forbidden: ${relative}`);
    const destination = join(output, 'site', relative);
    mkdirSync(dirname(destination), { recursive: true });
    copyFileSync(source, destination);
  }
  function sourceTree(relative) {
    const directory = join(repoRoot, 'site', relative);
    for (const entry of readdirSync(directory, { withFileTypes: true })) {
      if (blocked.test(entry.name)) continue;
      const child = join(relative, entry.name);
      if (entry.isSymbolicLink()) throw new Error(`Template links are forbidden: ${child}`);
      if (entry.isDirectory()) sourceTree(child);
      else if (entry.isFile() && sourceExtensions.has(extname(entry.name))) copy(child);
    }
  }
  for (const file of ['package.json', 'package-lock.json', 'astro.config.mjs', '.gitignore']) copy(file);
  for (const directory of ['src', 'scripts']) {
    if (existsSync(join(repoRoot, 'site', directory))) sourceTree(directory);
  }
  mkdirSync(join(output, 'data'), { recursive: true });
  // Neutral defaults, not the developer's data/ or SQLite settings/credentials.
  writeFileSync(join(output, 'data', 'site.json'), JSON.stringify({ name: 'My Site', author: { name: 'Veloct' }, comments: { enabled: false }, seo: { allowIndexing: true, engines: [] } }, null, 2) + '\n');
  writeFileSync(join(output, 'data', 'links.json'), '[]\n');
  return output;
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  console.log(`Staged clean project template: ${stageTemplate()}`);
}
