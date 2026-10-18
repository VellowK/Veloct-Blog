import { useEffect, useRef, useState, type ChangeEvent } from 'react';
import AvatarCropDialog from './AvatarCropDialog';
import { Card, StateBlock } from './components/ui';
import { Feedback } from './components/Notifications';
import { api, describeError, type AliasPolicy, type FileRevision, type PageTransition, type SiteSettings } from './lib/api';
import { SOCIAL_PLATFORMS, findSocialPlatform, type SocialPlatform } from '../site/src/lib/social-platforms';

const DEFAULT_SETTINGS: SiteSettings = {
  name: '',
  description: '',
  url: '',
  base: '/',
  language: 'zh-CN',
  author: { name: '', bio: '', avatar: '' },
  social: [],
  hero: { title: '' },
  homepage: { cover: '', githubUsername: '', showBlogHeatmap: true, showGithubHeatmap: true },
  articleDisplayCount: 10,
  archiveDisplayCount: 10,
  categoryDisplayCount: 10,
  tagDisplayCount: 10,
  aliasPolicy: 'redirect',
  pageTransition: 'rise',
  seo: { allow_indexing: true, engines: [] },
  filing: { icp: '', icpUrl: '', police: '', policeUrl: '', moe: '', moeUrl: '' },
};

type Section = 'article' | 'seo' | 'homepage' | 'beautify' | 'filing';

/** The three effects, in the order the form offers them, with what each one does. */
const PAGE_TRANSITIONS: Array<{ value: PageTransition; label: string; description: string }> = [
  { value: 'rise', label: '上浮', description: '新页面从下方轻盈进入，保留方向感。' },
  { value: 'dissolve', label: '溶解', description: '页面平滑淡入淡出，适合安静阅读。' },
  { value: 'none', label: '无动画', description: '立即切换，减少视觉干扰。' },
];

/**
 * Above this a source picture is refused before it is decoded.
 *
 * The same ceiling `veloct-app::works` puts on a poster, and for the same reason: this is a picture
 * being chosen as an avatar, and a file this large is a mistake rather than an avatar.
 */
const MAX_AVATAR_SOURCE_BYTES = 12 * 1024 * 1024;

/**
 * One platform's glyph.
 *
 * A platform may legitimately have NO icon — 抖音 is one, because Simple Icons publishes no `douyin`
 * — and the catalogue records that rather than borrowing a logo for a different product. Such a
 * platform is drawn as its own first character, so the picker still shows something identifiable and
 * an empty box never looks like a rendering bug.
 */
function SocialGlyph({ platform, size = 18 }: { platform: SocialPlatform; size?: number }) {
  if (platform.path) {
    return (
      <svg viewBox="0 0 24 24" width={size} height={size} fill="currentColor" aria-hidden="true">
        <path d={platform.path} />
      </svg>
    );
  }
  return <span className="contact-letter" aria-hidden="true">{platform.label.slice(0, 1)}</span>;
}

/**
 * One editor for the parts of the site's configuration this window owns.
 *
 * `mode` picks between the two SURFACES the app puts this in — the 站点信息 form, and the settings
 * form — and `onlySection` lets a caller ask for exactly one section with no tab strip. 外观 uses
 * that to give 站点美化 and 备案管理 a tab each while keeping ONE copy of the settings state and one
 * Save button, so two tabs can never hold conflicting drafts of the same file.
 */
export default function SiteSettingsPanel({ enabled, mode = 'settings', projectPath, onlySection, onEditHome, onDraftChange }: { enabled: boolean; mode?: 'settings' | 'info'; projectPath?: string | null; onlySection?: Section; onEditHome?: () => void; onDraftChange?: (settings: SiteSettings) => void }) {
  const [section, setSection] = useState<Section>('article');
  const activeSection = onlySection ?? section;
  const [settings, setSettings] = useState<SiteSettings>(DEFAULT_SETTINGS);
  const [revision, setRevision] = useState<FileRevision | null>(null);
  const [status, setStatus] = useState('');
  const [error, setError] = useState('');
  const [loaded, setLoaded] = useState(false);
  const [saving, setSaving] = useState(false);
  const [cropFile, setCropFile] = useState<File | null>(null);
  const [avatarBusy, setAvatarBusy] = useState(false);
  // The picture just cropped, kept as a data URL. It is what the thumbnail shows, because
  // `/assets/avatar.png` is a path only the SITE origin can resolve — inside the desktop window it
  // would be a broken image that looks like a failed upload.
  const [croppedPreview, setCroppedPreview] = useState('');
  const avatarInputRef = useRef<HTMLInputElement>(null);
  const coverInputRef = useRef<HTMLInputElement>(null);
  const [coverBusy, setCoverBusy] = useState(false);
  const imageScope = useRef(0);
  useEffect(() => {
    imageScope.current += 1;
    return () => { imageScope.current += 1; };
  }, [enabled, projectPath]);
  const [githubBusy, setGithubBusy] = useState(false);
  const homepage = settings.homepage ?? DEFAULT_SETTINGS.homepage!;
  const updateHomepage = (patch: Partial<typeof homepage>) => setSettings(current => ({ ...current, homepage: { ...DEFAULT_SETTINGS.homepage!, ...current.homepage, ...patch } }));
  const chooseCover = async (event: ChangeEvent<HTMLInputElement>) => {
    const file = event.target.files?.[0];
    event.target.value = '';
    if (!file || coverBusy || avatarBusy || saving) return;
    const scope = imageScope.current;
    setError(''); setStatus('');
    if (!['image/png', 'image/jpeg', 'image/webp', 'image/gif'].includes(file.type) || file.size > MAX_AVATAR_SOURCE_BYTES) {
      setError('请选择 12 MiB 以内的 PNG、JPEG、WebP 或 GIF 头图。'); return;
    }
    setCoverBusy(true);
    let bitmap: ImageBitmap | null = null;
    try {
      bitmap = await createImageBitmap(file);
      if (scope !== imageScope.current) return;
      if (bitmap.width * bitmap.height > 40_000_000) throw new Error('头图像素过大，请缩小后再上传。');
      const scale = Math.min(1, 2400 / bitmap.width, 1600 / bitmap.height);
      const canvas = document.createElement('canvas');
      canvas.width = Math.max(1, Math.round(bitmap.width * scale)); canvas.height = Math.max(1, Math.round(bitmap.height * scale));
      const context = canvas.getContext('2d');
      if (!context) throw new Error('无法处理头图。');
      context.drawImage(bitmap, 0, 0, canvas.width, canvas.height);
      const url = await api.saveSiteCover(canvas.toDataURL('image/png').split(',')[1] ?? '');
      if (scope !== imageScope.current) return;
      updateHomepage({ cover: url }); setStatus('头图已加入附件池并上传至首选目标，请保存站点信息后应用。');
    } catch (caught) { if (scope === imageScope.current) setError(describeError(caught)); }
    finally { bitmap?.close(); if (scope === imageScope.current) setCoverBusy(false); }
  };
  const syncGithub = async () => {
    if (githubBusy) return;
    setGithubBusy(true); setError(''); setStatus('');
    try {
      const result = await api.syncGithubActivity(homepage.githubUsername.trim());
      setStatus(`已同步 ${result.username} 的 ${result.days.length} 天贡献活跃度；请保存设置后预览/发布。`);
    } catch (caught) { setError(`${describeError(caught)} 上次成功的数据不会被删除。`); }
    finally { setGithubBusy(false); }
  };

  useEffect(() => {
    let cancelled = false;
    setLoaded(false);
    setStatus('');
    setError('');
    if (!enabled) return;
    void api.siteSettings().then((view) => {
      if (cancelled) return;
      setSettings({ ...view.settings, homepage: { ...DEFAULT_SETTINGS.homepage!, ...view.settings.homepage } });
      setRevision(view.revision);
      setLoaded(true);
    }).catch((caught) => { if (!cancelled) setError(describeError(caught)); });
    return () => { cancelled = true; };
  }, [enabled, projectPath]);

  useEffect(() => {
    if (mode !== 'info') return;
    const receive = (event: MessageEvent) => {
      const data = event.data as { type?: string; key?: string; value?: string };
      if (saving || event.source === window || !Array.from(document.querySelectorAll('iframe')).some((frame) => frame.contentWindow === event.source)) return;
      if (event.origin === 'null' || data?.type !== 'veloct-site-edit' || typeof data.key !== 'string' || typeof data.value !== 'string') return;
      if (data.key === 'description') update({ description: data.value });
      else if (data.key === 'hero.title') update({ hero: { ...settings.hero, title: data.value } });
       else if (data.key === 'hero.subtitle') update({ hero: { ...settings.hero, subtitle: data.value } });
      else if (data.key === 'author.name') update({ author: { ...settings.author, name: data.value } });
      else if (data.key === 'author.bio') update({ author: { ...settings.author, bio: data.value } });
      else if (data.key === 'author.avatar') update({ author: { ...settings.author, avatar: data.value } });
    };
    window.addEventListener('message', receive);
    return () => window.removeEventListener('message', receive);
  }, [mode, settings.author, settings.hero]);
  const onDraftChangeRef = useRef(onDraftChange);
  onDraftChangeRef.current = onDraftChange;
  useEffect(() => {
    if (mode === 'info' && loaded) onDraftChangeRef.current?.(settings);
  }, [mode, loaded, settings]);

  const save = async () => {
    if (!loaded || saving || avatarBusy || coverBusy) return;
    setError('');
    setStatus('');
    const snapshot = JSON.stringify(settings);
    setSaving(true);
    try {
      const result = await api.saveSiteSettings(settings, revision);
      setRevision(result.revision);
      if (JSON.stringify(settings) === snapshot) {
        setSettings(result.settings);
        setStatus('设置已保存');
      } else {
        setStatus('已保存当前版本，仍有新改动');
      }
    } catch (caught) {
      setError(describeError(caught));
    } finally {
      setSaving(false);
    }
  };

  const update = (patch: Partial<SiteSettings>) => setSettings((current) => ({ ...current, ...patch }));
  const updateContact = (index: number, patch: Partial<SiteSettings['social'][number]>) => setSettings((current) => ({ ...current, social: current.social.map((contact, position) => position === index ? { ...contact, ...patch } : contact) }));
  const updateSeo = (patch: Partial<SiteSettings['seo']>) => setSettings((current) => ({ ...current, seo: { ...current.seo, ...patch } }));
  const updateFiling = (patch: Partial<SiteSettings['filing']>) => setSettings((current) => ({ ...current, filing: { ...current.filing, ...patch } }));

  /**
   * What the thumbnail is able to show.
   *
   * A picture just cropped is a data URL and always displays; a stored `http(s):` address displays
   * too. A site-relative path such as `/assets/avatar.png` does NOT resolve inside this window — it
   * belongs to the preview origin — so it is deliberately left out rather than shown as a broken
   * image that would read as "the upload failed".
   */
  const avatarThumbnail = croppedPreview
    || (/^(https?:|data:)/i.test(settings.author.avatar) ? settings.author.avatar : '');

  /** Add one contact, pre-filled from the platform the user picked. */
  const addPlatform = (platform: SocialPlatform) => {
    setSettings((current) => ({
      ...current,
      social: current.social.length < 16 ? [...current.social, { label: platform.label, url: '', id: '', icon: platform.key }] : current.social,
    }));
  };

  const addBlankContact = () => {
    setSettings((current) => ({ ...current, social: current.social.length < 16 ? [...current.social, { label: '', url: '', id: '', icon: '' }] : current.social }));
  };

  /**
   * Point a contact at a platform.
   *
   * The label is only replaced when it is still EMPTY: a user who renamed "GitHub" to "我的 GitHub"
   * must not have that overwritten by picking the platform again, and one who picks a platform on a
   * blank row should not have to type the ordinary name.
   */
  const changePlatform = (index: number, key: string) => {
    const platform = findSocialPlatform(key);
    setSettings((current) => ({
      ...current,
      social: current.social.map((contact, position) => position === index
        ? { ...contact, icon: key, label: contact.label.trim() ? contact.label : platform?.label ?? '' }
        : contact),
    }));
  };

  const chooseAvatar = (event: ChangeEvent<HTMLInputElement>) => {
    const chosen = event.target.files?.[0];
    // Clearing the value first is what makes choosing the SAME file twice fire a change event.
    event.target.value = '';
    if (!chosen || avatarBusy || coverBusy || saving) return;
    setError('');
    setStatus('');
    if (!chosen.type.startsWith('image/')) {
      setError('请选择 PNG、JPEG、WebP 或 GIF 图片。');
      return;
    }
    if (chosen.size > MAX_AVATAR_SOURCE_BYTES) {
      setError(`这张图片是 ${(chosen.size / (1024 * 1024)).toFixed(1)} MiB，超过 ${MAX_AVATAR_SOURCE_BYTES / (1024 * 1024)} MiB 上限。`);
      return;
    }
    setCropFile(chosen);
  };

  /**
   * Cache and upload the cropped PNG through the attachment pool before changing the form URL.
   * Saving settings is still explicit, and a failed or late upload never replaces the old picture.
   */
  const applyCroppedAvatar = async (dataBase64: string) => {
    if (avatarBusy || coverBusy || saving) return;
    const scope = imageScope.current;
    setAvatarBusy(true);
    setError(''); setStatus('');
    try {
      const url = await api.saveSiteAvatar(dataBase64);
      if (scope !== imageScope.current) return;
      setSettings(current => ({ ...current, author: { ...current.author, avatar: url } }));
      setCroppedPreview(`data:image/png;base64,${dataBase64}`);
      setCropFile(null);
      setStatus('头像已加入附件池并上传至首选目标，请保存站点信息后应用。');
    } catch (caught) {
      if (scope === imageScope.current) setError(describeError(caught));
    } finally {
      if (scope === imageScope.current) setAvatarBusy(false);
    }
  };

  return (
    <>
    <Card className="settings-panel" title={mode === 'info' ? '个人主页信息' : ({ article: '文章与列表', seo: '搜索引擎优化', homepage: '首页与热力图', beautify: '站点美化', filing: '备案信息' })[activeSection]} description={mode === 'info' ? '修改后保存，再通过右侧真实站点预览检查效果。' : '配置仅在点击保存后应用到当前项目。'}>
      {mode === 'settings' && !onlySection && <div className="settings-subtabs" role="group" aria-label="站点设置分类">
        <button type="button" aria-pressed={activeSection === 'article'} className={activeSection === 'article' ? 'is-active' : ''} onClick={() => setSection('article')}>文章设置</button>
        <button type="button" aria-pressed={activeSection === 'seo'} className={activeSection === 'seo' ? 'is-active' : ''} onClick={() => setSection('seo')}>SEO 设置</button>
        <button type="button" aria-pressed={activeSection === 'homepage'} className={activeSection === 'homepage' ? 'is-active' : ''} onClick={() => setSection('homepage')}>首页与热力图</button>
      </div>}
      <Feedback message={error} tone="error" />
      <Feedback message={status} tone="success" />
      {!enabled ? <StateBlock tone="empty" title="请先打开项目" description="选择项目后即可编辑站点配置。" /> : !loaded ? <StateBlock tone={error ? 'error' : 'loading'} title={error ? '站点信息未能载入' : '正在读取站点信息…'} description={error ? '请重新打开项目后重试。' : undefined} /> : (
        <>
          {mode === 'info' && <div className="settings-form">
            <div className="settings-field settings-field--wide">
              <h3>个人主页</h3>
              <p className="muted">头图、头像和名称位于页首；站点描述与下方的个人介绍独立保存。热力图在「设置 → 站点设置 → 首页与热力图」配置。</p>
              <p className="muted">图片首选目标：附件管理「图片多目标同步」的第一项；未配置时，使用上传优先级最高的可见云存储。没有可用目标时不会静默改成本地保存，也不会上传其他备份目标。</p>
              <details><summary>保留的旧首页内容</summary><button type="button" onClick={onEditHome} disabled={!onEditHome}>查看旧首页 Markdown（不用于个人主页）</button></details>
            </div>
            <div className="settings-field"><label htmlFor="site-name">博客名称</label><input id="site-name" value={settings.name} onChange={(event) => update({ name: event.target.value })} /></div>
            <div className="settings-field"><label htmlFor="site-url">站点 URL</label><input id="site-url" type="url" placeholder="https://example.com" value={settings.url} onChange={(event) => update({ url: event.target.value })} /></div>
            <div className="settings-field settings-field--wide"><label htmlFor="info-description">站点描述</label><textarea id="info-description" rows={3} value={settings.description} onChange={(event) => update({ description: event.target.value })} /></div>
            <div className="settings-field"><label htmlFor="author-name">作者名称</label><input id="author-name" value={settings.author.name} onChange={(event) => update({ author: { ...settings.author, name: event.target.value } })} /></div>
            <div className="settings-field settings-field--wide">
              <label htmlFor="author-avatar">首页头像</label>
              <div className="avatar-field">
                {avatarThumbnail
                  ? <img className="avatar-thumb" src={avatarThumbnail} alt="" />
                  : <span className="avatar-thumb avatar-thumb--empty" aria-hidden="true">无</span>}
                <input id="author-avatar" placeholder="https://… 或 /assets/avatar.png" value={settings.author.avatar} onChange={(event) => update({ author: { ...settings.author, avatar: event.target.value } })} />
                <button type="button" onClick={() => avatarInputRef.current?.click()} disabled={!enabled || avatarBusy || coverBusy || saving}>{avatarBusy ? '正在入池并上传…' : '上传并裁切'}</button>
              </div>
              <small className="muted">图片裁成正方形后加入附件池，并上传至首选附件目标；成功后回填公开地址，仍需保存站点信息。也可直接填写已有地址。</small>
              <input ref={avatarInputRef} type="file" accept="image/png,image/jpeg,image/webp,image/gif" hidden onChange={chooseAvatar} />
            </div>
            <div className="settings-field settings-field--wide"><label htmlFor="home-cover">首页头图</label><div className="avatar-field"><input id="home-cover" value={homepage.cover} placeholder="https://… 或 /assets/home-cover.png" onChange={event => updateHomepage({ cover: event.target.value })} /><button type="button" disabled={!enabled || coverBusy || avatarBusy || saving} onClick={() => coverInputRef.current?.click()}>{coverBusy ? '正在入池并上传…' : '上传头图'}</button></div><small className="muted">横向图片居中裁切展示；留空显示默认渐变。处理后的图片加入附件池并上传至首选目标，上传失败不替换原图。</small><input ref={coverInputRef} type="file" hidden accept="image/png,image/jpeg,image/webp,image/gif" onChange={event => void chooseCover(event)} /></div>
            <div className="settings-field settings-field--wide"><label htmlFor="author-bio">个人介绍（独立于站点描述）</label><textarea id="author-bio" rows={6} placeholder="介绍自己、兴趣和正在做的事…" value={settings.author.bio} onChange={(event) => update({ author: { ...settings.author, bio: event.target.value } })} /><small className="muted">显示在个人主页下方，保留换行，不会替代站点描述。</small></div>
            <div className="settings-field settings-field--wide">
              <h3>联系方式 / 账号 ID</h3>
              <p className="muted">首页右侧每行最多 4 个图标、合计最多 16 个，按以下顺序展示；没有填写链接或 ID 的项目自动隐藏，全部为空时整块隐藏。填写公开信息，不要填写密码或令牌。链接可使用 https://、mailto:、tel: 或站内路径；只填 ID 时点击图标可复制。旧配置超过 16 个时不会删除数据，首页仅显示前 16 个。</p>
              {settings.social.length > 0 && (
                <div className="contact-list">
                  {settings.social.map((contact, index) => {
                    const platform = findSocialPlatform(typeof contact.icon === 'string' ? contact.icon : '');
                    return (
                      <div className="contact-row" key={index}>
                        <span className="contact-glyph" aria-hidden="true">
                          {platform ? <SocialGlyph platform={platform} /> : <span className="contact-glyph__plain" />}
                        </span>
                        <div className="contact-row__fields">
                          <label className="contact-field">
                            <span>平台</span>
                            <select value={platform?.key ?? ''} onChange={(event) => changePlatform(index, event.target.value)}>
                              <option value="">自定义</option>
                              {SOCIAL_PLATFORMS.map((option) => <option key={option.key} value={option.key}>{option.label}</option>)}
                            </select>
                          </label>
                          <label className="contact-field">
                            <span>名称</span>
                            <input placeholder="GitHub / 邮箱 / 微信" value={contact.label} onChange={(event) => updateContact(index, { label: event.target.value })} />
                          </label>
                          <label className="contact-field">
                            <span>账号 / ID</span>
                            <input placeholder="公开账号" value={contact.id ?? ''} onChange={(event) => updateContact(index, { id: event.target.value })} />
                          </label>
                          <label className="contact-field contact-field--wide">
                            <span>链接</span>
                            <input placeholder={platform?.urlHint ?? 'https://… 或 站内路径'} value={contact.url} onChange={(event) => updateContact(index, { url: event.target.value })} />
                          </label>
                        </div>
                        <div className="contact-order"><button type="button" disabled={index === 0} aria-label={`上移联系方式 ${index + 1}`} onClick={() => { const social = [...settings.social]; [social[index - 1], social[index]] = [social[index]!, social[index - 1]!]; update({ social }); }}>↑</button><button type="button" disabled={index === settings.social.length - 1} aria-label={`下移联系方式 ${index + 1}`} onClick={() => { const social = [...settings.social]; [social[index], social[index + 1]] = [social[index + 1]!, social[index]!]; update({ social }); }}>↓</button></div>
                        <button type="button" className="contact-remove" onClick={() => update({ social: settings.social.filter((_, position) => position !== index) })} aria-label={`删除联系方式 ${index + 1}`}>删除</button>
                      </div>
                    );
                  })}
                </div>
              )}
              <div className="contact-presets">
                <p className="muted">常用平台，点一下即可添加：</p>
                {(['domestic', 'international'] as const).map((region) => (
                  <div className="contact-presets__row" key={region}>
                    <span className="contact-presets__label">{region === 'domestic' ? '国内' : '国外'}</span>
                    <div className="contact-presets__grid">
                      {SOCIAL_PLATFORMS.filter((platform) => platform.region === region).map((platform) => (
                        <button
                          key={platform.key}
                          type="button"
                          className="contact-preset"
                          title={`添加 ${platform.label}`}
                          disabled={settings.social.length >= 16}
                          onClick={() => addPlatform(platform)}
                        >
                          <SocialGlyph platform={platform} size={16} />
                          <span>{platform.label}</span>
                        </button>
                      ))}
                    </div>
                  </div>
                ))}
                <button type="button" disabled={settings.social.length >= 16} onClick={addBlankContact}>添加自定义联系方式（{settings.social.length}/16）</button>
              </div>
            </div>
          </div>}
          {mode === 'settings' && activeSection === 'homepage' && <div className="settings-form">
            <div className="settings-field settings-field--wide"><h3>首页活动热力图</h3><p className="muted">首页只显示一张近 365 天热力图，两项都开启时用图表标题旁的按钮切换。博客统计公开文章（含公开 WIP）与知识条目的创建日期，不泄露未公开草稿；GitHub 展示公开可见的贡献活跃度，不把颜色等级当作提交数量。</p></div>
            <div className="settings-field"><label><input type="checkbox" checked={homepage.showBlogHeatmap} onChange={event => updateHomepage({ showBlogHeatmap: event.target.checked })} /> 显示博客创作热力图</label></div>
            <div className="settings-field"><label><input type="checkbox" checked={homepage.showGithubHeatmap} onChange={event => updateHomepage({ showGithubHeatmap: event.target.checked })} /> 显示 GitHub 热力图</label></div>
            <div className="settings-field settings-field--wide"><label htmlFor="home-github">关联 GitHub 用户名</label><input id="home-github" maxLength={39} placeholder="octocat（不填完整链接）" value={homepage.githubUsername} onChange={event => updateHomepage({ githubUsername: event.target.value.trim() })} /><small className="muted">公开主页：https://github.com/{homepage.githubUsername || '用户名'}。无需令牌；先同步再保存。换账号后不会复用旧账号数据。</small></div>
            <div className="settings-field settings-field--wide"><div><button type="button" disabled={githubBusy || !homepage.githubUsername} onClick={() => void syncGithub()}>{githubBusy ? '正在同步…' : '同步 GitHub 贡献日历'}</button></div><small className="muted">只在点击时连接 GitHub 官方公开日历，结果保存在项目内随网站发布；失败保留上次快照。静态网站不会自动更新，需再次同步并发布。GitHub 的公开 HTML 结构变化可能导致同步失败。</small></div>
          </div>}
          {mode === 'settings' && activeSection === 'article' && <div className="settings-form">
            <div className="settings-field"><label htmlFor="article-count">文章列表显示条数</label><input id="article-count" type="number" min={1} max={100} value={settings.articleDisplayCount} onChange={(event) => update({ articleDisplayCount: Math.max(1, Number(event.target.value) || 1) })} /></div>
            <div className="settings-field"><label htmlFor="archive-count">归档页文章显示条数</label><input id="archive-count" type="number" min={1} max={100} value={settings.archiveDisplayCount} onChange={(event) => update({ archiveDisplayCount: Math.max(1, Number(event.target.value) || 1) })} /></div>
            <div className="settings-field"><label htmlFor="category-count">分类页文章显示条数</label><input id="category-count" type="number" min={1} max={100} value={settings.categoryDisplayCount} onChange={(event) => update({ categoryDisplayCount: Math.max(1, Number(event.target.value) || 1) })} /></div>
            <div className="settings-field"><label htmlFor="tag-count">标签页文章显示条数</label><input id="tag-count" type="number" min={1} max={100} value={settings.tagDisplayCount} onChange={(event) => update({ tagDisplayCount: Math.max(1, Number(event.target.value) || 1) })} /></div>
            <div className="settings-field"><label htmlFor="alias-policy">别名生成策略</label><select id="alias-policy" value={settings.aliasPolicy} onChange={(event) => update({ aliasPolicy: event.target.value as AliasPolicy })}><option value="redirect">重定向到规范地址</option><option value="canonical_only">仅使用规范地址</option><option value="disabled">禁用别名</option></select></div>
          </div>}
          {mode === 'settings' && activeSection === 'beautify' && <div className="settings-form settings-form--single">
            <div className="settings-field settings-field--wide">
              <h3>页面切换动效</h3>
              <div className="transition-options" role="radiogroup" aria-label="页面切换动效">
                {PAGE_TRANSITIONS.map((option) => (
                  <button
                    key={option.value}
                    type="button"
                    role="radio"
                    aria-checked={settings.pageTransition === option.value}
                    className={`transition-option${settings.pageTransition === option.value ? ' is-selected' : ''}`}
                    onClick={() => update({ pageTransition: option.value })}
                  >
                    <span className={`transition-preview transition-preview--${option.value}`} aria-hidden="true">
                      <span />
                    </span>
                    <span className="transition-option-copy">
                      <strong>{option.label}</strong>
                      <small>{option.description}</small>
                    </span>
                  </button>
                ))}
              </div>
            </div>
          </div>}
          {mode === 'settings' && activeSection === 'seo' && <div className="settings-form">
            <label className="settings-check"><input type="checkbox" checked={!settings.seo.allow_indexing} onChange={(event) => updateSeo({ allow_indexing: !event.target.checked })} /><span>屏蔽搜索引擎</span></label>
            <div className="settings-field settings-field--wide"><label htmlFor="seo-keywords">站点关键词</label><input id="seo-keywords" placeholder="用逗号分隔" value={(settings.seo as SiteSettings['seo'] & { keywords?: string }).keywords ?? ''} onChange={(event) => updateSeo({ keywords: event.target.value } as Partial<SiteSettings['seo']>)} /></div>
            <div className="settings-field settings-field--wide"><label htmlFor="site-description">站点描述</label><textarea id="site-description" rows={3} value={settings.description} onChange={(event) => update({ description: event.target.value })} /></div>
            <div className="settings-field settings-field--wide"><label htmlFor="seo-injection">SEO HTML 注入(Baidu / Google / Bing)</label><textarea id="seo-injection" rows={5} placeholder="粘贴验证平台提供的 meta 或 script 标签" value={(settings.seo as SiteSettings['seo'] & { html_injection?: string }).html_injection ?? ''} onChange={(event) => updateSeo({ html_injection: event.target.value } as Partial<SiteSettings['seo']>)} /></div>
          </div>}
          {mode === 'settings' && activeSection === 'filing' && <div className="settings-form">
            <div className="settings-field"><label htmlFor="icp">ICP 备案号</label><input id="icp" value={settings.filing.icp} onChange={(event) => updateFiling({ icp: event.target.value })} /></div>
            <div className="settings-field"><label htmlFor="icp-url">ICP 备案跳转链接</label><input id="icp-url" type="url" placeholder="https://beian.miit.gov.cn/" value={settings.filing.icpUrl} onChange={(event) => updateFiling({ icpUrl: event.target.value })} /></div>
            <div className="settings-field"><label htmlFor="police">公安备案号</label><input id="police" value={settings.filing.police} onChange={(event) => updateFiling({ police: event.target.value })} /></div>
            <div className="settings-field"><label htmlFor="police-url">公安备案跳转链接</label><input id="police-url" type="url" placeholder="https://beian.mps.gov.cn/" value={settings.filing.policeUrl} onChange={(event) => updateFiling({ policeUrl: event.target.value })} /></div>
            <div className="settings-field"><label htmlFor="moe">萌备案</label><input id="moe" value={settings.filing.moe} onChange={(event) => updateFiling({ moe: event.target.value })} /></div>
            <div className="settings-field"><label htmlFor="moe-url">萌备案跳转链接</label><input id="moe-url" type="url" placeholder="https://…" value={settings.filing.moeUrl} onChange={(event) => updateFiling({ moeUrl: event.target.value })} /></div>
          </div>}
          <div className="settings-actions"><button type="button" className="primary" disabled={saving || avatarBusy || coverBusy || githubBusy} onClick={() => void save()}>{saving ? '正在保存…' : mode === 'info' ? '保存站点信息' : '保存设置'}</button></div>
        </>
      )}
    </Card>
      {cropFile && (
        <AvatarCropDialog
          file={cropFile}
          busy={avatarBusy}
          onCancel={() => setCropFile(null)}
          onConfirm={(dataBase64) => void applyCroppedAvatar(dataBase64)}
        />
      )}
    </>
  );
}
