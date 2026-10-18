import { chromium } from '../target/browser-validation/node_modules/playwright/index.mjs';
import { writeFile } from 'node:fs/promises';
const browser = await chromium.launch({ channel: 'msedge', headless: true });
try {
 const page = await browser.newPage({ viewport: { width: 1440, height: 1100 } });
 await page.goto('https://osu.ppy.sh/users/20791206', { waitUntil: 'domcontentloaded', timeout: 60000 });
 await page.locator('[data-page-id="main"]').waitFor({ timeout: 30000 });
 const main = page.locator('[data-page-id="main"]');
 console.log((await main.innerText()).slice(0, 6500));
 await writeFile('target/osu-reference-main.html', await main.evaluate(el => el.outerHTML));
 await page.screenshot({ path: 'target/osu-reference.png', fullPage: false });
} finally { await browser.close(); }
