import type { AssessmentMode } from './server/integrity/statistics';
export const committeeEnabled = (mode: AssessmentMode) =>
	mode === 'statistical' || mode === 'statistical_shadow';
export const longModelEnabled = (mode: AssessmentMode) =>
	mode === 'model_only' || mode === 'long_only';
export const shortModelEnabled = (mode: AssessmentMode) =>
	mode === 'model_only' || mode === 'short_only';
export const legacyEnabled = (mode: AssessmentMode) => mode === 'legacy';
export const engineNames: Record<AssessmentMode, string> = {
	disabled: '已关闭',
	legacy: '旧规则',
	statistical: '委员会裁决',
	statistical_shadow: '委员会影子评估',
	model_only: '短窗＋长时序模型',
	long_only: '仅长时序模型',
	short_only: '仅短窗模型'
};
