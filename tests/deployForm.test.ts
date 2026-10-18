import { describe, expect, it } from 'vitest';
import type { DeployOutcomeView } from '../src/lib/api';
import { deployFormKind, deployOutcomeNotice, isEdgeOneKind, validateEdgeOneProjects } from '../src/lib/deployForm';

const outcome: DeployOutcomeView = {
  release_id: 'fixture-release', targets: [], all_succeeded: false, needing_attention: [],
  pushed_files: 0, started_at: '', finished_at: null, seo_state: 'not_requested',
};

describe('explicit EdgeOne deployment form', () => {
  it('recognizes both stored region identifiers and both editable identifiers', () => {
    for (const kind of ['edgeone_cn', 'edgeone_mainland', 'edgeone_global', 'edgeone_international']) expect(isEdgeOneKind(kind)).toBe(true);
    for (const kind of ['cloudflare_pages', 'onepanel', 'edgeone']) expect(isEdgeOneKind(kind)).toBe(false);
    expect(deployFormKind('edgeone_cn')).toBe('edgeone_mainland');
    expect(deployFormKind('edgeone_global')).toBe('edgeone_international');
    expect(deployFormKind('onepanel')).toBe('onepanel');
  });
  it('accepts main-only or separate main and works project IDs', () => {
    expect(validateEdgeOneProjects('proj-123_a', '')).toBeNull();
    expect(validateEdgeOneProjects(' main-123 ', ' works-456 ')).toBeNull();
  });
  it('refuses absent IDs, pasted URLs and unsafe characters', () => {
    for (const project of ['', ' ', 'https://site.edgeone.app', 'proj/name', '../id', '名称', 'a'.repeat(129)]) expect(validateEdgeOneProjects(project, '')).not.toBeNull();
    expect(validateEdgeOneProjects('main', 'https://works.example.com')).not.toBeNull();
  });
  it('refuses using the same project for both roots to avoid overwrites', () => {
    expect(validateEdgeOneProjects('main', ' main ')).toContain('互相覆盖');
  });
  it('does not announce a failed, partial, disabled or empty deployment as success', () => {
    expect(deployOutcomeNotice(outcome)).toContain('未推送');
    const failed = { ...outcome, targets: [{ target_id: 'cn' } as DeployOutcomeView['targets'][number]] };
    expect(deployOutcomeNotice(failed)).toContain('未成功');
    expect(deployOutcomeNotice({ ...failed, all_succeeded: true })).toBe('所有已启用目标部署成功。');
  });
});
