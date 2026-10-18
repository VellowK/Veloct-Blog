import type { S3Policy } from './lib/api';
import { Field } from './components/ui';
import type { StorageTargetDraft } from './lib/storageTargetForm';
import { s3Defaults, splitAddress, type S3FormDraft, type Protocol } from './lib/s3Form';

interface Props { draft: StorageTargetDraft; onChange: (draft: StorageTargetDraft) => void; disabled: boolean }
export default function S3ConfigFields({ draft, onChange, disabled }: Props) {
  const policy = draft.s3 ?? s3Defaults(true, draft.endpoint, draft.publicUrlPrefix);
  const patch = (value: Partial<StorageTargetDraft>) => onChange({ ...draft, s3: policy, ...value });
  const setPolicy = (value: Partial<S3FormDraft>) => patch({ s3: { ...policy, ...value } });
  const protocol = (key: 'endpointProtocol' | 'cdnProtocol') => (
    <select value={policy[key]} disabled={disabled} onChange={event => setPolicy({ [key]: event.target.value as Protocol })}>
      <option value="https">HTTPS</option><option value="http">HTTP（不加密）</option>
    </select>
  );
  return <fieldset className="s3-config" disabled={disabled}>
    <legend>策略配置 · S3 / Cloudflare R2</legend>
    <p className="card-note"><a href="https://github.com/halo-dev/plugin-s3#上传目录" target="_blank" rel="noreferrer">查看配置指南</a> · 空端点沿用 AWS；R2 使用账户端点、Path Style 和小写 auto。不会套用示例账户或自动上传。</p>
    <div className="form-grid">
      <Field label="在上传界面隐藏" hint="仅隐藏上传选择项，不删除策略，也不停止已配置的后台同步。">
        <span><input type="checkbox" checked={draft.hiddenFromUpload ?? false} onChange={e => patch({ hiddenFromUpload: e.target.checked })} /> 开启后，在上传界面隐藏该存储策略</span>
      </Field>
      <Field label="上传界面显示优先级" hint="数字越大，排序越靠前；相同优先级保持原顺序。">
        <input type="number" step="1" min="-2147483648" max="2147483647" value={draft.uploadPriority ?? 0} onChange={e => patch({ uploadPriority: Number(e.target.value) })} />
      </Field>
      <Field label="Bucket 桶名称 *"><input value={draft.bucket} placeholder="picbed" onChange={e => patch({ bucket: e.target.value })} spellCheck={false} /></Field>
      <Field label="Endpoint 访问协议 *">{protocol('endpointProtocol')}</Field>
      <Field label="Endpoint 访问风格 *">
        <select value={policy.access_style} onChange={e => setPolicy({ access_style: e.target.value as S3Policy['access_style'] })}>
          <option value="path">Path Style</option><option value="virtual_hosted">Virtual Hosted Style</option><option value="auto">兼容旧配置（自动）</option>
        </select>
      </Field>
      <Field label="EndPoint" hint="不含 http:// 或 https://；协议在上方设置。留空使用 AWS 区域端点。">
        <input value={splitAddress(draft.endpoint).host} placeholder="账户ID.r2.cloudflarestorage.com" spellCheck={false} onChange={e => patch({ endpoint: e.target.value })} />
      </Field>
      <Field label="Access Key ID *" hint="输入框遮蔽显示；ID 是配置项，Secret 只保存在操作系统凭据库。">
        <input type="password" value={draft.accessKey} autoComplete="off" spellCheck={false} onChange={e => patch({ accessKey: e.target.value })} />
      </Field>
      <Field label="Access Key Secret *" hint="修改时留空保留已保存的 Secret；验证不会保存凭据。">
        <input type="password" value={draft.credential} autoComplete="new-password" spellCheck={false} placeholder="输入密钥；修改时可留空" onChange={e => patch({ credential: e.target.value })} />
      </Field>
      <Field label="Region" hint="Cloudflare R2 必须填写小写 auto；其他服务填写对应区域。旧配置留空仍使用 us-east-1。">
        <input value={draft.region} placeholder="auto" spellCheck={false} onChange={e => patch({ region: e.target.value })} />
      </Field>
      <Field label="上传目录" hint="留空上传到根目录。支持 ${year}/${month}/${day}、UUID、时间戳及随机占位符；时间取附件添加时的 UTC，重试路径稳定。">
        <input value={draft.pathPrefix} placeholder="例如 images/${year}/${month}" spellCheck={false} onChange={e => patch({ pathPrefix: e.target.value })} />
      </Field>
      <Field label="上传时重命名文件方式 *" hint="仅作用于新上传；已记录的同内容附件仍复用原对象与链接。">
        <select value={policy.rename} onChange={e => setPolicy({ rename: e.target.value as S3Policy['rename'] })}>
          <option value="uuid">使用 UUID</option><option value="original">使用原文件名</option><option value="timestamp">使用毫秒时间戳</option>
          <option value="original_random">使用原文件名 + 随机字母</option><option value="date_random">使用日期 + 随机字母</option>
          <option value="date_time_random">使用日期时间 + 随机字母</option><option value="random">使用随机字母</option><option value="custom">自定义</option>
          <option value="legacy">兼容旧命名（原名 + 既有短后缀）</option>
        </select>
      </Field>
      {['original_random', 'date_random', 'date_time_random', 'random'].includes(policy.rename) && <Field label="随机字母长度" hint="1–64 个小写字母；同一附件重试保持一致。"><input type="number" min="1" max="64" value={policy.random_length} onChange={e => setPolicy({ random_length: Number(e.target.value) })} /></Field>}
      {policy.rename === 'custom' && <Field label="自定义文件名模板" hint="不填写扩展名；支持 ${origin-filename}、${uuid-no-dash} 和目录中的日期/随机占位符。"><input value={policy.filename_template} placeholder="${origin-filename}-${random-alphanumeric:5}" spellCheck={false} onChange={e => setPolicy({ filename_template: e.target.value })} /></Field>}
      <Field label="重复文件名处理方式 *" hint="新策略使用条件写入，永不覆盖；对象冲突最多追加后缀重试 16 次。内容去重优先于同名处理。">
        <select value={policy.duplicate} onChange={e => setPolicy({ duplicate: e.target.value as S3Policy['duplicate'] })}>
          <option value="random_alphanumeric">加随机字母数字后缀</option><option value="random_alphabetic">加随机字母后缀</option><option value="error">停止上传并报错</option><option value="legacy_reuse">兼容旧行为（沿用远程同名对象）</option>
        </select>
      </Field>
      <Field label="绑定域名协议 *">{protocol('cdnProtocol')}</Field>
      <Field label="绑定域名（CDN 域名）" hint="不含协议头，可带路径前缀；上传到公开图床需填写可公开访问的域名。">
        <input value={splitAddress(draft.publicUrlPrefix).host} placeholder="cdn.example.com" spellCheck={false} onChange={e => patch({ publicUrlPrefix: e.target.value })} />
      </Field>
    </div>
    <section className="s3-config__rules" aria-label="网址后缀">
      <h3>网址后缀</h3><p className="card-note">扩展名用逗号分隔，* 表示所有类型；从上到下只取首个匹配项。参数以 ? 或分隔符 !、@ 开头。</p>
      {policy.url_suffixes.map((rule, index) => <div className="s3-config__rule" key={index}>
        <Field label={`文件类型 ${index + 1}`}><input value={rule.extensions} placeholder="jpg,jpeg,png" onChange={e => setPolicy({ url_suffixes: policy.url_suffixes.map((r, i) => i === index ? { ...r, extensions: e.target.value } : r) })} /></Field>
        <Field label={`网址后缀 ${index + 1}`}><input value={rule.suffix} placeholder="?imageMogr2/format/webp" onChange={e => setPolicy({ url_suffixes: policy.url_suffixes.map((r, i) => i === index ? { ...r, suffix: e.target.value } : r) })} /></Field>
        <button type="button" aria-label={`上移后缀规则 ${index + 1}`} disabled={index === 0} onClick={() => { const rules = [...policy.url_suffixes]; [rules[index - 1], rules[index]] = [rules[index]!, rules[index - 1]!]; setPolicy({ url_suffixes: rules }); }}>上移</button>
        <button type="button" aria-label={`删除后缀规则 ${index + 1}`} onClick={() => setPolicy({ url_suffixes: policy.url_suffixes.filter((_, i) => i !== index) })}>删除</button>
      </div>)}
      <button type="button" disabled={policy.url_suffixes.length >= 32} onClick={() => setPolicy({ url_suffixes: [...policy.url_suffixes, { extensions: '', suffix: '' }] })}>添加网址后缀</button>
    </section>
    <div className="form-grid">
      <Field label="缩略图参数" hint="仅资源预览使用，不改变正文链接或对象路径。R2 本身不提供图片处理，默认选择无。">
        <select value={policy.thumbnail ? 'custom' : 'none'} onChange={e => setPolicy({ thumbnail: e.target.value === 'none' ? '' : '?width={width}' })}><option value="none">无</option><option value="custom">自定义参数</option></select>
      </Field>
      {policy.thumbnail && <Field label="自定义缩略图参数" hint="{width} 将替换为缩略图宽度（默认 400）；按服务商文档填写，必须由域名服务支持。"><input value={policy.thumbnail} placeholder="?width={width}" onChange={e => setPolicy({ thumbnail: e.target.value })} /></Field>}
    </div>
    {policy.endpointProtocol === 'http' && <p role="alert">HTTP 不加密。Access Key ID、签名与文件内容可能被旁路观察；建议使用 HTTPS。</p>}
  </fieldset>;
}
