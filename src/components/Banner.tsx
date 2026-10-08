import type { Health } from "../bindings/Health";
export function Banner({ health }: { health: Health }) {
  const message = health.lastError || (health.pluginVersion && !health.versionSupported
    ? `codex-plugin-cc のバージョン（${health.pluginVersion}）は動作未確認です` : null);
  return message ? <div className="banner" role="alert">{message}</div> : null;
}
