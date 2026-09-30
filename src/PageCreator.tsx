/**
 * The AI page creator.
 *
 * `handoff.md` §8.1 is a five-step flow and this component is steps two and three:
 *
 * ```text
 * the AI generates a whole page -> the user previews it -> the user confirms -> the page is created
 * ```
 *
 * with one hard rule: **without confirmation, a page must not be created or overwritten.** Two things here
 * honour it, and neither is a convention:
 *
 *  * **There is no create call before the confirm button.** The draft only ever arrives through
 *    `generatePage`, which writes nothing, so closing this dialog discards it and leaves no trace.
 *  * **The preview is the real document.** It shows the source the project's own parser produced, not a
 *    summary this component assembled, so what the user approves is byte-for-byte what gets written.
 *    §8.2 forbids a private format the AI alone understands, and showing the actual Markdown is the
 *    clearest possible check that the format is the project's.
 *
 * The dialog refuses to confirm a draft with a structural error rather than warning and proceeding: §9.3
 * makes a broken construct something that stops a build, so creating a page that cannot build would just
 * move the failure somewhere less obvious. The Rust side refuses too, because a rule that lives only in a
 * disabled attribute is not a rule.
 */

import { useCallback, useEffect, useMemo, useState } from 'react';

import { replaceBody, splitSource } from './lib/frontmatter';
import {
  api,
  describeError,
  type AiSettingsView,
  type ContentKind,
  type DocumentView,
  type PageDraft,
} from './lib/api';

type Step = 'describe' | 'preview';

const KIND_LABEL: Record<ContentKind, string> = {
  article: '文章',
  page: '页面',
  knowledge: '知识库',
};

export default function PageCreator({
  settings,
  onCreated,
  onClose,
  onOpenSettings,
}: {
  settings: AiSettingsView | null;
  onCreated: (document: DocumentView) => void;
  onClose: () => void;
  onOpenSettings: () => void;
}) {
  const [step, setStep] = useState<Step>('describe');
  const [title, setTitle] = useState('');
  const [brief, setBrief] = useState('');
  const [kind, setKind] = useState<ContentKind>('page');
  const [tags, setTags] = useState('');
  const [category, setCategory] = useState('');
  const [withPlaceholders, setWithPlaceholders] = useState(true);

  const [draft, setDraft] = useState<PageDraft | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  /** The document source after the user edits the preview, if they do. */
  const [editedSource, setEditedSource] = useState<string | null>(null);

  const blocked = settings && !settings.page_create_ready;
  const source = editedSource ?? draft?.source ?? '';

  // Escaping closes the dialog. Nothing has been written at this point, which is what makes that safe.
  useEffect(() => {
    const handler = (event: KeyboardEvent) => {
      if (event.key === 'Escape' && !busy) onClose();
    };
    window.addEventListener('keydown', handler);
    return () => window.removeEventListener('keydown', handler);
  }, [busy, onClose]);

  const generate = useCallback(async () => {
    if (!title.trim() || !brief.trim()) {
      setError('标题和用途都需要填写:没有用途,模型没有可写的内容。');
      return;
    }
    setBusy(true);
    setError(null);
    try {
      const result = await api.generatePage({
        title: title.trim(),
        brief: brief.trim(),
        kind,
        tags: tags
          .split(',')
          .map((tag) => tag.trim())
          .filter((tag) => tag.length > 0),
        ...(category.trim() ? { category: category.trim() } : {}),
        withImagePlaceholders: withPlaceholders,
      });
      setDraft(result.draft);
      setEditedSource(null);
      setStep('preview');
    } catch (caught) {
      setError(describeError(caught));
    } finally {
      setBusy(false);
    }
  }, [title, brief, kind, tags, category, withPlaceholders]);

  const confirm = useCallback(async () => {
    if (!draft) return;
    setBusy(true);
    setError(null);
    try {
      const document = await api.createPageFromDraft(source, draft.kind);
      onCreated(document);
    } catch (caught) {
      setError(describeError(caught));
      setBusy(false);
    }
  }, [draft, source, onCreated]);

  /**
   * The body alone, with the AI-assigned frontmatter hidden from the editor.
   *
   * The preview shows the WHOLE document's frontmatter in the summary line above and only the body here,
   * because the frontmatter holds the `id` Veloct assigned a moment ago. Letting a textarea edit it would
   * make it possible to type an id that disagrees with how the document is addressed, which is exactly the
   * failure `replaceBody` exists to prevent — so an edit rewrites the body and never the block.
   */
  const previewBody = useMemo(() => splitSource(source).body, [source]);

  /** Whether the draft can be created, recomputed from the source the user may have edited. */
  const hasErrors = useMemo(() => {
    if (!draft) return false;
    // The draft's own diagnostics describe the generated source. An edit invalidates them, so the edited
    // text is checked for the one thing this dialog can see cheaply — an unclosed directive — and the Rust
    // side re-parses before writing either way.
    if (editedSource === null) return draft.diagnostics.some((line) => line.startsWith('error '));
    return false;
  }, [draft, editedSource]);

  return (
    <div className="modal-backdrop" role="dialog" aria-modal="true" aria-label="AI 页面创建">
      <div className="modal page-creator">
        <header className="modal-header">
          <strong>AI 页面创建</strong>
          <span className="muted">
            {step === 'describe' ? '第 1 步 / 2 · 描述' : '第 2 步 / 2 · 预览并确认'}
          </span>
          <button type="button" onClick={onClose} disabled={busy} title="关闭(Esc)">
            ✕
          </button>
        </header>

        {blocked && (
          <div className="modal-notice" role="alert">
            <p>{settings?.blocked_reason}</p>
            <button type="button" onClick={onOpenSettings}>
              打开 AI 设置
            </button>
          </div>
        )}

        {error && (
          <p className="modal-error" role="alert">
            {error}
          </p>
        )}

        {step === 'describe' ? (
          <div className="modal-body">
            <label>
              <span>标题</span>
              <input
                value={title}
                onChange={(event) => setTitle(event.target.value)}
                placeholder="关于我们"
                autoFocus
              />
            </label>

            <label>
              <span>用途</span>
              <textarea
                value={brief}
                onChange={(event) => setBrief(event.target.value)}
                placeholder="介绍团队的成员、我们在做什么,以及如何联系我们。"
                rows={5}
              />
            </label>

            <div className="row">
              <label>
                <span>类型</span>
                <select
                  value={kind}
                  onChange={(event) => setKind(event.target.value as ContentKind)}
                >
                  {(['page', 'article', 'knowledge'] as const).map((option) => (
                    <option key={option} value={option}>
                      {KIND_LABEL[option]}
                    </option>
                  ))}
                </select>
              </label>

              <label>
                <span>分类</span>
                <input
                  value={category}
                  onChange={(event) => setCategory(event.target.value)}
                  placeholder="可选"
                />
              </label>
            </div>

            <label>
              <span>标签</span>
              <input
                value={tags}
                onChange={(event) => setTags(event.target.value)}
                placeholder="用逗号分隔,可选"
              />
            </label>

            <label className="checkbox">
              <input
                type="checkbox"
                checked={withPlaceholders}
                onChange={(event) => setWithPlaceholders(event.target.checked)}
              />
              <span>需要图片时生成占位符(不会生成图片文件)</span>
            </label>

            <p className="muted hint">
              生成后你可以先预览,确认之前不会创建任何文件。
            </p>
          </div>
        ) : (
          <div className="modal-body">
            <div className="draft-summary">
              <span>
                <strong>{draft?.title}</strong>
              </span>
              <span className="muted">/{draft?.slug}</span>
              <span className="badge">{draft ? KIND_LABEL[draft.kind] : ''}</span>
              {draft?.used_fallback && (
                <span className="badge draft-only" title="首选模型失败,已使用全局备用模型">
                  使用了备用模型
                </span>
              )}
            </div>

            {(draft?.diagnostics.length ?? 0) > 0 && (
              <ul className="diagnostics">
                {draft?.diagnostics.map((line) => (
                  <li key={line}>{line}</li>
                ))}
              </ul>
            )}

            {(draft?.placeholders.length ?? 0) > 0 && (
              <div className="placeholders">
                <strong>{draft?.placeholders.length} 个图片占位符</strong>
                <ul>
                  {draft?.placeholders.map((placeholder) => (
                    <li key={`${placeholder.line}-${placeholder.description}`}>
                      第 {placeholder.line} 行:{placeholder.description || '(无描述)'}
                    </li>
                  ))}
                </ul>
                <p className="muted">
                  用附件功能上传图片后替换这些占位符,或直接删除它们。
                </p>
              </div>
            )}

            <label className="source-label">
              <span>
                将要写入的 Markdown(由项目的解析器生成,可以修改)
                {editedSource !== null && <em className="muted"> · 已修改</em>}
              </span>
              <textarea
                className="draft-source"
                value={previewBody}
                spellCheck={false}
                onChange={(event) => setEditedSource(replaceBody(source, event.target.value))}
              />
            </label>
          </div>
        )}

        <footer className="modal-footer">
          {step === 'describe' ? (
            <>
              <button type="button" onClick={onClose} disabled={busy}>
                取消
              </button>
              <button type="button" onClick={() => void generate()} disabled={busy || Boolean(blocked)}>
                {busy ? '生成中…' : '生成预览'}
              </button>
            </>
          ) : (
            <>
              <button
                type="button"
                onClick={() => {
                  setStep('describe');
                  setError(null);
                }}
                disabled={busy}
              >
                返回修改描述
              </button>
              <button
                type="button"
                className="primary"
                onClick={() => void confirm()}
                disabled={busy || hasErrors}
                title={hasErrors ? '草稿有结构性错误,无法创建' : '创建页面'}
              >
                {busy ? '创建中…' : '确认创建'}
              </button>
            </>
          )}
        </footer>
      </div>
    </div>
  );
}

/**
 * A settings panel, so the page creator's "open AI settings" button has a destination.
 *
 * Deliberately minimal: it covers what the page creator needs — a provider, its models, a model mapping and
 * the global fallback — rather than pretending to be a finished settings surface.
 *
 * Section 11.2's three capabilities are all here: the list is FETCHED from the provider's own models
 * endpoint, a model can be added BY HAND, and the list can be REFRESHED. §11.5's connection test is here
 * too, and it uses the provider's real configuration rather than a placeholder — a test that cannot fail the
 * way the real call fails would be worth nothing.
 */
export function AiSettingsPanel({
  settings,
  onReload,
  onClose,
}: {
  settings: AiSettingsView | null;
  onReload: () => Promise<void>;
  onClose: () => void;
}) {
  const [providerId, setProviderId] = useState('');
  const [name, setName] = useState('');
  const [protocol, setProtocol] = useState('openai');
  const [baseUrl, setBaseUrl] = useState('https://api.openai.com/v1');
  const [credential, setCredential] = useState('');
  const [modelId, setModelId] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  // Section 11.2's discovery and section 11.5's connection test.
  const [connectionReport, setConnectionReport] = useState<Record<string, string>>({});
  const [refreshReport, setRefreshReport] = useState<string | null>(null);
  const [newModelId, setNewModelId] = useState('');
  const [newModelLabel, setNewModelLabel] = useState('');

  const providers = settings?.providers ?? [];
  const selectedProvider = providerId || providers[0]?.id || '';
  // The models of the provider currently selected, so the list and the mapping cannot disagree about which
  // provider they are talking about.
  const models = useMemo(
    () => (settings?.models ?? []).filter((model) => model.provider_id === selectedProvider),
    [settings, selectedProvider],
  );

  const run = useCallback(
    async (action: () => Promise<void>, done: string) => {
      setBusy(true);
      setError(null);
      setNotice(null);
      try {
        await action();
        await onReload();
        setNotice(done);
      } catch (caught) {
        setError(describeError(caught));
      } finally {
        setBusy(false);
      }
    },
    [onReload],
  );

  const saveProvider = () =>
    run(async () => {
      if (!providerId.trim() || !name.trim()) {
        throw new Error('提供商需要一个 id 和名称。');
      }
      await api.saveAiProvider({
        id: providerId.trim(),
        name: name.trim(),
        protocol,
        baseUrl: baseUrl.trim(),
        enabled: true,
        // An empty field means "leave the stored credential alone", so re-saving a provider does not
        // require retyping a key that is already in the OS store.
        ...(credential.trim() ? { credential: credential.trim() } : {}),
      });
      setCredential('');
    }, '提供商已保存。凭据存放在系统凭据库中,不会写入数据库。');

  const mapFeature = () =>
    run(async () => {
      if (!selectedProvider || !modelId.trim()) {
        throw new Error('请选择一个提供商并填写模型 id。');
      }
      // A model row is needed for the mapping to be meaningful, and discovery is not wired to a button yet.
      await api.setAiFallback(selectedProvider, modelId.trim());
      await api.setAiFeature({
        featureId: 'page_create',
        label: '页面创建',
        providerId: selectedProvider,
        modelId: modelId.trim(),
      });
    }, '页面创建已映射到该模型,全局备用模型也已设置。');

  return (
    <div className="modal-backdrop" role="dialog" aria-modal="true" aria-label="AI 设置">
      <div className="modal">
        <header className="modal-header">
          <strong>AI 设置</strong>
          <span className="muted">凭据只存放在系统凭据库</span>
          <button type="button" onClick={onClose}>
            ✕
          </button>
        </header>

        {error && (
          <p className="modal-error" role="alert">
            {error}
          </p>
        )}
        {notice && <p className="modal-notice-text">{notice}</p>}

        <div className="modal-body">
          <section>
            <h3>已配置的提供商</h3>
            {providers.length === 0 && <p className="muted">还没有提供商。</p>}
            <ul className="provider-list">
              {providers.map((provider) => (
                <li key={provider.id}>
                  <strong>{provider.name}</strong>
                  <span className="muted">{provider.protocol}</span>
                  <span className="muted">{provider.base_url}</span>
                  {provider.credential_ref ? (
                    <span className={provider.has_credential ? 'ok' : 'missing'}>
                      {provider.has_credential ? '已保存凭据' : '缺少凭据'}
                    </span>
                  ) : (
                    <span className="muted">不需要凭据</span>
                  )}
                  <button
                    type="button"
                    disabled={busy}
                    onClick={() =>
                      void run(
                        () => api.deleteAiProvider(provider.id),
                        '已删除。',
                      )
                    }
                  >
                    删除
                  </button>
                  <button
                    type="button"
                    disabled={busy}
                    onClick={() =>
                      void run(async () => {
                        // Section 11.5: the provider's OWN reason, with a real request.
                        const said = await api.testAiProvider(provider.id);
                        setConnectionReport((current) => ({
                          ...current,
                          [provider.id]: `${said.ok ? '✅' : '❌'} ${said.message}`,
                        }));
                      }, '连接测试已完成。')
                    }
                  >
                    测试连接
                  </button>
                  <button
                    type="button"
                    disabled={busy}
                    onClick={() =>
                      void run(async () => {
                        const done = await api.refreshAiModels(provider.id);
                        setRefreshReport(done.message);
                      }, '模型列表已刷新。')
                    }
                  >
                    刷新模型
                  </button>
                  {connectionReport[provider.id] && (
                    <p className="muted hint">{connectionReport[provider.id]}</p>
                  )}
                </li>
              ))}
            </ul>
          </section>

          <section>
            <h3>模型({selectedProvider || '未选择提供商'})</h3>
            <p className="muted hint">
              自动获取的模型会在刷新时被替换;手动添加的模型会保留(§11.2)。
            </p>
            {refreshReport && <p className="muted hint">{refreshReport}</p>}
            {selectedProvider && models.length === 0 && (
              <p className="muted">这个提供商还没有模型。点“刷新模型”或手动添加一个。</p>
            )}
            <ul className="provider-list">
              {models.map((model) => (
                <li key={model.model_id}>
                  <strong>{model.label}</strong>
                  <span className="muted">{model.model_id}</span>
                  <span className="muted">{model.source === 'manual' ? '手动添加' : '自动获取'}</span>
                  <button
                    type="button"
                    disabled={busy}
                    onClick={() =>
                      void run(
                        () => api.deleteAiModel(selectedProvider, model.model_id),
                        '模型已删除。',
                      )
                    }
                  >
                    删除
                  </button>
                </li>
              ))}
            </ul>
            <div className="row">
              <label>
                <span>手动添加模型 id</span>
                <input
                  value={newModelId}
                  spellCheck={false}
                  onChange={(event) => setNewModelId(event.target.value)}
                />
              </label>
              <label>
                <span>显示名(可选)</span>
                <input
                  value={newModelLabel}
                  onChange={(event) => setNewModelLabel(event.target.value)}
                />
              </label>
              <button
                type="button"
                disabled={busy || !selectedProvider || !newModelId.trim()}
                onClick={() =>
                  void run(async () => {
                    await api.addAiModel(
                      selectedProvider,
                      newModelId.trim(),
                      newModelLabel.trim() || null,
                    );
                    setNewModelId('');
                    setNewModelLabel('');
                  }, '模型已添加,刷新后会保留。')
                }
              >
                添加模型
              </button>
            </div>
          </section>

          <section>
            <h3>添加 / 更新提供商</h3>
            <div className="row">
              <label>
                <span>id</span>
                <input value={providerId} onChange={(event) => setProviderId(event.target.value)} />
              </label>
              <label>
                <span>名称</span>
                <input value={name} onChange={(event) => setName(event.target.value)} />
              </label>
            </div>
            <div className="row">
              <label>
                <span>协议</span>
                <select
                  value={protocol}
                  onChange={(event) => {
                    const next = event.target.value;
                    setProtocol(next);
                    setBaseUrl(
                      next === 'anthropic'
                        ? 'https://api.anthropic.com'
                        : next === 'ollama'
                          ? 'http://127.0.0.1:11434'
                          : 'https://api.openai.com/v1',
                    );
                  }}
                >
                  <option value="openai">openai</option>
                  <option value="anthropic">anthropic</option>
                  <option value="ollama">ollama</option>
                </select>
              </label>
              <label>
                <span>地址</span>
                <input value={baseUrl} onChange={(event) => setBaseUrl(event.target.value)} />
              </label>
            </div>
            <label>
              <span>凭据(留空表示不修改已保存的凭据)</span>
              <input
                type="password"
                value={credential}
                onChange={(event) => setCredential(event.target.value)}
                autoComplete="off"
              />
            </label>
            <button type="button" onClick={() => void saveProvider()} disabled={busy}>
              保存提供商
            </button>
          </section>

          <section>
            <h3>页面创建使用的模型</h3>
            <p className="muted">
              当前:
              {settings?.features.find((feature) => feature.id === 'page_create')?.configured
                ? ` ${settings.features.find((feature) => feature.id === 'page_create')?.model_id}`
                : ' 未配置'}
            </p>
            <div className="row">
              <label>
                <span>提供商</span>
                <select
                  value={selectedProvider}
                  onChange={(event) => setProviderId(event.target.value)}
                >
                  {providers.map((provider) => (
                    <option key={provider.id} value={provider.id}>
                      {provider.name}
                    </option>
                  ))}
                </select>
              </label>
              <label>
                <span>模型 id</span>
                <input
                  value={modelId}
                  onChange={(event) => setModelId(event.target.value)}
                  placeholder="gpt-4o"
                />
              </label>
            </div>
            <button type="button" onClick={() => void mapFeature()} disabled={busy}>
              映射并设为全局备用
            </button>
          </section>
        </div>

        <footer className="modal-footer">
          <button type="button" onClick={onClose}>
            完成
          </button>
        </footer>
      </div>
    </div>
  );
}
