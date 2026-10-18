// Browser test of the real DocumentPreview/API boundary; no iframe is mounted on a 404 probe.
import assert from 'node:assert/strict';
import { build } from 'esbuild';
import http from 'node:http';
import { chromium } from '../target/browser-validation/node_modules/playwright/index.mjs';
const result = await build({ stdin: { contents: `
import React from 'react';
import {createRoot} from 'react-dom/client';
import DocumentPreview from './src/DocumentPreview';
window.probeReady = false; window.probeError = false; window.calls = [];
window.__TAURI_INTERNALS__ = {invoke: async (command,args) => {
  window.calls.push({command,args});
  if (window.probeError) throw {code:'PREVIEW_STOPPED',message:'preview stopped',actionable:true};
  return window.probeReady;
}};
const root = createRoot(document.getElementById('root'));
window.selectDocument = (id) => root.render(<DocumentPreview key={id} siteUrl={location.origin+'/'} kind="article" id={id} route={'/wip/'+id+'/'} />);
window.selectDocument('a');
`, resolveDir: process.cwd(), loader: 'tsx' }, bundle: true, write: false, format: 'iife', jsx: 'automatic', define: { 'process.env.NODE_ENV': '"production"' } });
const script = result.outputFiles[0].text;
const navigations = [];
const server = http.createServer((request, response) => {
  if (request.url === '/app.js') { response.setHeader('Content-Type', 'text/javascript'); response.end(script); }
  else if (request.url.startsWith('/wip/')) { navigations.push(request.url); response.end('<html><body>real preview placeholder</body></html>'); }
  else { response.setHeader('Content-Type','text/html'); response.end('<html><body><div id="root"></div><script src="/app.js"></script></body></html>'); }
});
await new Promise(resolve => server.listen(0,'127.0.0.1',resolve));
let browser;
try {
  browser = await chromium.launch({ channel:'msedge', headless:true });
  const page = await browser.newPage(); const errors = []; page.on('pageerror', error => errors.push(String(error)));
  await page.goto(`http://127.0.0.1:${server.address().port}/`);
  await page.waitForFunction(() => window.calls.length >= 3);
  assert.equal(await page.locator('iframe').count(),0); assert.equal(navigations.length,0);
  assert.match(await page.locator('#root').innerText(),/正在生成页面预览/);
  await page.evaluate(() => { window.probeReady = true; });
  await page.waitForSelector('iframe'); await page.waitForFunction(() => document.querySelector('iframe')?.contentDocument?.body.textContent.includes('placeholder'));
  assert.deepEqual(navigations,['/wip/a/']);
  await page.evaluate(() => { window.probeReady=false; window.selectDocument('b'); });
  await page.waitForFunction(() => window.calls.some(call => call.args.id==='b'));
  assert.equal(await page.locator('iframe').count(),0);
  await page.evaluate(() => { window.probeError=true; });
  await page.getByRole('button',{name:'重试预览'}).waitFor();
  assert.equal(navigations.length,1);
  await page.evaluate(() => { window.probeError=false; window.probeReady=true; });
  await page.getByRole('button',{name:'重试预览'}).click(); await page.waitForSelector('iframe');
  await page.waitForFunction(() => document.querySelector('iframe')?.contentDocument?.body.textContent.includes('placeholder'));
  assert.deepEqual(navigations,['/wip/a/','/wip/b/']);
  assert.deepEqual(errors,[]);
  assert.ok((await page.evaluate(() => window.calls)).every(call => call.command==='preview_document_ready'));
  console.log(JSON.stringify({ noIframeWhile404:true, exactDocumentProbe:true, correctRouteAfterReady:true, selectionResetsReadiness:true, commandErrorAndRetry:true, pageErrors:errors }));
} finally { if(browser)await browser.close(); await new Promise(resolve => server.close(resolve)); }
