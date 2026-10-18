// Real Chromium layout regression, using the production PreviewPanel and production CSS.
// Starts only a short-lived fixture server, never replaces the desktop/DSH application.
import { build } from 'esbuild';
import { readFile } from 'node:fs/promises';
import { createServer } from 'node:http';
import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { resolve } from 'node:path';
const edge = process.env.VELOCT_TEST_BROWSER ?? 'C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe';
const execute = promisify(execFile);
const source = `
import React from 'react';
import { createRoot } from 'react-dom/client';
import PreviewPanel from './src/PreviewPanel';
const preview = { site: { state: 'running' }, site_url: location.origin + '/site' };
createRoot(document.getElementById('root')).render(
  <div className="app"><aside className="sidebar">Veloct</aside><div className="main"><header className="topbar">外观</header><div className="content content--site-info"><div className="page page--legacy settings-page"><header className="page-header"><h1>外观</h1></header><div className="settings-tabs">站点信息 / 主题美化 / 备案设置</div><div className="site-info-workspace"><div className="site-info-workspace__settings">{Array.from({length:30},(_,i)=><div className="card" key={i}>站点设置 {i}<input defaultValue="示例站点" /></div>)}</div><PreviewPanel enabled preview={preview} busy={false} error={null} revision={0} onRetry={()=>{}} /></div></div></div><footer className="statusbar">就绪</footer></div></div>
);
const wait = (ms) => new Promise(resolve => setTimeout(resolve, ms));
const results = [];
(async () => {
  await wait(500);
  for (const mobile of [false, true]) {
    document.querySelectorAll('.preview-toolbar__devices button')[mobile ? 1 : 0].click();
    for (const zoom of ['50','75','100','125','150']) {
      const select = document.querySelector('select');
      select.value = zoom;
      select.dispatchEvent(new Event('change', {bubbles:true}));
      await wait(250);
      const sizes = [];
      for (let i=0;i<12;i++) {
        const panel = document.querySelector('.preview-panel').getBoundingClientRect();
        const viewport = document.querySelector('.preview-panel__viewport');
        sizes.push([panel.width, panel.height, viewport.clientWidth, viewport.clientHeight].join(','));
        await wait(25);
      }
      const stable = new Set(sizes).size === 1;
      const [width,height] = sizes[0].split(',').map(Number);
      results.push({mobile,zoom,stable,width,height,...(!stable ? {samples:sizes} : {})});
    }
  }
  const passed = results.every(r => r.stable && r.width > 100 && r.height > 100 && r.height < innerHeight);
  document.body.innerHTML = '<pre id="result">' + JSON.stringify({passed,results}) + '</pre>';
})().catch(e => { document.body.innerHTML = '<pre id="result">' + JSON.stringify({error:String(e)}) + '</pre>'; });
`;
const bundle = await build({ stdin: { contents: source, resolveDir: process.cwd(), loader: 'tsx' }, bundle: true, write: false, outfile: 'fixture.js', jsx: 'automatic', define: { 'process.env.NODE_ENV': '"production"' } });
const js = bundle.outputFiles.find(f => f.path.endsWith('.js')).text;
const css = bundle.outputFiles.find(f => f.path.endsWith('.css')).text;
const baseCss = await readFile('src/styles.css','utf8');
const server = createServer((req,res) => {
  if (req.url === '/fixture.js') { res.setHeader('Content-Type','text/javascript'); res.end(js); }
  else if (req.url === '/fixture.css') { res.setHeader('Content-Type','text/css'); res.end(baseCss + '\n' + css); }
  else if (req.url === '/site') { res.end('<html><body style="height:3000px">Real iframe viewport</body></html>'); }
  else { res.end('<!doctype html><html><head><link rel="stylesheet" href="/fixture.css"></head><body><div id="root"></div><script src="/fixture.js"></script></body></html>'); }
});
await new Promise(resolve => server.listen(0,'127.0.0.1',resolve));
const url = `http://127.0.0.1:${server.address().port}`;
try {
  for (const size of ['1440,900','1024,640']) {
    const { stdout } = await execute(edge, ['--headless','--disable-gpu','--no-first-run','--no-default-browser-check',`--user-data-dir=${resolve('target', 'preview-layout-profile-' + size.replace(',','-'))}`, '--dump-dom','--virtual-time-budget=14000',`--window-size=${size}`,url], {timeout:60000,maxBuffer:4*1024*1024});
    const text = stdout.match(/<pre id="result">([^<]+)<\/pre>/)?.[1];
    if (!text) throw new Error('Browser did not finish layout sampling: ' + stdout.slice(-1000));
    const result = JSON.parse(text.replaceAll('&quot;','"').replaceAll('&amp;','&'));
    console.log(size, JSON.stringify(result));
    if (!result.passed) process.exitCode = 1;
  }
} finally { server.close(); }
