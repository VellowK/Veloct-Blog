import type { DeployOutcomeView } from './api';

export function isEdgeOneKind(kind: string): boolean {
  return ['edgeone_mainland', 'edgeone_cn', 'edgeone_international', 'edgeone_global'].includes(kind);
}

export function deployFormKind(kind: string): string {
  return kind === 'edgeone_cn' ? 'edgeone_mainland' : kind === 'edgeone_global' ? 'edgeone_international' : kind;
}

export function validateEdgeOneProjects(project: string, worksProject: string): string | null {
  const valid = (value: string) => /^[A-Za-z0-9_-]{1,128}$/.test(value);
  if (!valid(project.trim())) return 'EdgeOne 需要主站项目 ID，不是项目名称或网址。';
  if (worksProject.trim() && !valid(worksProject.trim())) return '作品站需要有效的 EdgeOne 项目 ID。';
  if (worksProject.trim() && project.trim() === worksProject.trim()) return '主站与作品站必须使用不同的项目 ID，避免互相覆盖。';
  return null;
}

export function deployOutcomeNotice(outcome: DeployOutcomeView): string {
  if (outcome.targets.length === 0) return '未推送任何目标：请检查部署开关和目标配置。';
  if (outcome.all_succeeded) return '所有已启用目标部署成功。';
  return '部署检查已结束，部分目标未成功或需要手动上传；请查看下方结果。';
}
