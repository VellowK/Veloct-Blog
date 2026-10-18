// Actual settings component and CSS with an isolated in-memory API boundary; no user project.
import assert from 'node:assert/strict';
import { build } from 'esbuild';
import { readFile } from 'node:fs/promises';
import { chromium } from '../target/browser-validation/node_modules/playwright/index.mjs';
const source = `
import React from 'react'; import { createRoot } from 'react-dom/client';
import SiteSettingsPanel from './src/SiteSettingsPanel'; import { api } from './src/lib/api';
import { NotificationHost } from './src/components/Notifications';
import { normalizeSiteConfig } from './site/src/lib/site-config.mjs';
`;
// The browser cannot import the Node config reader. Keep the fixture DTO explicit; production
// serialization and migration are covered by Rust and profileHome.test.mjs.
const fixture = source.replace("import { normalizeSiteConfig } from './site/src/lib/site-config.mjs';", '') + `
const settings = { name:'Profile', description:'站点描述', url:'https://example.com', base:'/', language:'zh-CN', author:{name:'Vellow',bio:'独立个人介绍',avatar:''}, social:[{label:'微信',id:'wechat-id',url:'',icon:'wechat'},{label:'GitHub',url:'https://github.com/octocat',id:'octocat',icon:'github'}], hero:{title:'',subtitle:'',ctaLabel:'',ctaHref:''}, homepage:{cover:'',githubUsername:'',showBlogHeatmap:true,showGithubHeatmap:true}, articleDisplayCount:10,archiveDisplayCount:10,categoryDisplayCount:10,tagDisplayCount:10,aliasPolicy:'redirect',pageTransition:'rise',seo:{allow_indexing:true,engines:[],keywords:'',html_injection:''},filing:{} };
window.fixture = { settings, saved:[], sync:[], covers:[], fail:false };
api.siteSettings = async () => ({settings:structuredClone(settings),revision:null});
api.saveSiteSettings = async s => { window.fixture.saved.push(structuredClone(s)); Object.assign(settings, structuredClone(s)); return {settings:s,revision:{hash:'saved',size:1},changed:true}; };
api.syncGithubActivity = async username => { window.fixture.sync.push(username); if(window.fixture.fail) throw {message:'HTTP 503',code:'invalid',hint:null}; return {username,days:Array(365).fill({date:'2026-10-15',level:1}),fetchedAt:'2026-10-15'}; };
api.saveSiteCover = async bytes => { window.fixture.covers.push(bytes); return '/assets/home-cover.png'; };
const root=createRoot(document.getElementById('root')); let revision=0;
window.mount = mode => root.render(<><NotificationHost /><div className="app"><div className="main"><div className="content"><SiteSettingsPanel key={++revision} enabled mode={mode} projectPath="fixture-only" /></div></div></div></>);
window.mount('info');
`;
const bundle = await build({ stdin: { contents: fixture, resolveDir: process.cwd(), loader: 'tsx' }, outfile: 'fixture.js', bundle: true, write: false, platform: 'browser', jsx: 'automatic', define: { 'process.env.NODE_ENV': '"production"' } });
const js = bundle.outputFiles.find(file => file.path.endsWith('.js')).text;
const css = (await readFile('src/styles.css', 'utf8')) + '\n' + (bundle.outputFiles.find(file => file.path.endsWith('.css'))?.text ?? '');
const browser = await chromium.launch({ channel: 'msedge', headless: true });
try {
  for (const width of [1440, 1024]) {
    const page = await browser.newPage({ viewport: { width, height: width === 1440 ? 900 : 640 } });
    const errors = []; page.on('pageerror', error => errors.push(error.message));
    await page.route('http://profile-settings.test/**', route => {
      const path = new URL(route.request().url()).pathname;
      return route.fulfill(path === '/fixture.js' ? { contentType:'text/javascript',body:js } : path === '/fixture.css' ? {contentType:'text/css',body:css} : {contentType:'text/html',body:'<!doctype html><html><head><link rel="stylesheet" href="/fixture.css"></head><body><div id="root"></div><script src="/fixture.js"></script></body></html>'});
    });
    await page.goto('http://profile-settings.test/');
    await page.locator('#author-bio').waitFor();
    await page.locator('#info-description').fill('更改站点描述'); await page.locator('#author-bio').fill('独立的多行介绍\n第二行');
    await page.getByRole('button', {name:'上移联系方式 2', exact:true}).click();
    await page.locator('input[type=file]').last().setInputFiles('src-tauri/icons-logo/128x128.png');
    await page.waitForFunction(() => window.fixture.covers.length === 1);
    assert.equal(await page.locator('#home-cover').inputValue(), '/assets/home-cover.png');
    await page.getByRole('button', {name:'保存站点信息',exact:true}).click();
    await page.waitForFunction(() => window.fixture.saved.length === 1);
    const saved = await page.evaluate(() => window.fixture.saved[0]);
    assert.equal(saved.description, '更改站点描述'); assert.equal(saved.author.bio, '独立的多行介绍\n第二行'); assert.equal(saved.social[0].icon, 'github'); assert.equal(saved.homepage.cover, '/assets/home-cover.png');
    for (let n=0;n<14;n++) await page.getByRole('button', {name:/添加自定义联系方式/}).click();
    assert.equal(await page.getByRole('button', {name:/添加自定义联系方式/}).isDisabled(), true); assert.equal(await page.locator('.contact-row').count(), 16);
    await page.evaluate(() => window.mount('settings'));
    await page.getByRole('button', {name:'首页与热力图',exact:true}).click();
    await page.locator('#home-github').fill('octocat');
    await page.getByLabel('显示博客创作热力图').uncheck();
    await page.getByRole('button', {name:'同步 GitHub 贡献日历',exact:true}).click();
    await page.getByText(/已同步 octocat 的 365 天贡献活跃度/).waitFor();
    await page.evaluate(() => window.fixture.fail = true);
    await page.getByRole('button', {name:'同步 GitHub 贡献日历',exact:true}).click();
    await page.getByText(/上次成功的数据不会被删除/).waitFor();
    await page.getByRole('button', {name:'保存设置',exact:true}).click();
    await page.waitForFunction(() => window.fixture.saved.length === 2);
    assert.equal(await page.evaluate(() => window.fixture.saved[1].homepage.githubUsername), 'octocat');
    assert.equal(await page.evaluate(() => window.fixture.saved[1].homepage.showBlogHeatmap), false);
    assert.deepEqual(errors, []); console.log(JSON.stringify({width,coverUpload:true,independentFields:true,contactReorderAndLimit:true,githubAccountAndSwitches:true,failedSyncNotice:true,pageErrors:errors}));
    await page.close();
  }
} finally { await browser.close(); }
