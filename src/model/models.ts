import type { ModelCombination } from "../bindings/ModelCombination";
import type { ModelInfo } from "../bindings/ModelInfo";

export function modelName(model: string | null, models: ModelInfo[] | null): string {
  if (!model) return "不明";
  return models?.find((entry) => entry.slug === model)?.display_name ?? (models?.length ? `${model}（一覧に無いモデル）` : model);
}

export function normalizeCombination(value: ModelCombination, models: ModelInfo[] | null): ModelCombination {
  const supported = models?.find((model) => model.slug === value.model)?.supported_reasoning_levels;
  return value.effort === null || supported?.some((level) => level.effort === value.effort) ? value : { ...value, effort: null };
}

export function combinationError(value: ModelCombination, models: ModelInfo[] | null): string | null {
  if (!value.model.trim() || /[\u0000-\u001f\u007f]/.test(value.model + (value.effort ?? ""))) return "モデル名を入力してください";
  const list = models?.length ? models : null;
  const model = list?.find((entry) => entry.slug === value.model);
  if (list && !model) return "一覧に無いモデルです。選び直してください";
  if (value.effort !== null && !model?.supported_reasoning_levels.some((level) => level.effort === value.effort))
    return "対応する推論の強さを選び直してください";
  return null;
}

export function resetTime(seconds: number, now: number): string {
  const date = new Date(seconds * 1000);
  const today = new Date(now);
  const time = `${String(date.getHours()).padStart(2, "0")}:${String(date.getMinutes()).padStart(2, "0")}`;
  const prefix = date.getFullYear() === today.getFullYear() && date.getMonth() === today.getMonth() && date.getDate() === today.getDate()
    ? "" : `${date.getMonth() + 1}/${date.getDate()} `;
  return `${prefix}${time} にリセット`;
}

export function switchConfirmation(agentName: string, value: ModelCombination): string {
  return `${agentName}の既定を ${value.model}（推論 ${value.effort ?? "変えない"}）にします`;
}
