import type { Format, MetricLine } from "./api";
import type { I18n } from "./i18n";

const locale = (i18n: I18n) => i18n.lang;

export function formatNumber(value: number, format: Format, i18n: I18n): string {
  switch (format.type) {
    case "percent":
      return `${Math.round(value)}%`;
    case "currency":
      try {
        return new Intl.NumberFormat(locale(i18n), { style: "currency", currency: format.code, maximumFractionDigits: 2 }).format(value);
      } catch {
        return `${value.toFixed(2)} ${format.code}`;
      }
    case "count":
      return new Intl.NumberFormat(locale(i18n), { maximumFractionDigits: 1 }).format(value);
  }
}

/** The text at the end of a row: `42%`, `$12.40 / $50.00`, `12 / 100 searches`. */
export function lineValue(line: MetricLine, i18n: I18n): string {
  if (line.kind === "value") {
    const suffix = line.format.type === "count" ? ` ${line.format.suffix}` : "";
    return formatNumber(line.value, line.format, i18n) + suffix;
  }
  if (line.format.type === "percent") return formatNumber(line.used, line.format, i18n);
  const suffix = line.format.type === "count" ? ` ${line.format.suffix}` : "";
  return `${formatNumber(line.used, line.format, i18n)} / ${formatNumber(line.limit, line.format, i18n)}${suffix}`;
}

export function usedPercent(line: MetricLine): number | null {
  if (line.kind !== "progress" || line.limit <= 0) return null;
  return Math.max(0, Math.min(100, (line.used / line.limit) * 100));
}

export function level(percent: number): "ok" | "warn" | "danger" {
  return percent >= 90 ? "danger" : percent >= 70 ? "warn" : "ok";
}

/** `2d 4h`, `3h 12m`, `8m`. */
export function duration(ms: number): string {
  const minutes = Math.max(1, Math.round(ms / 60_000));
  const days = Math.floor(minutes / 1440);
  const hours = Math.floor((minutes % 1440) / 60);
  if (days > 0) return hours > 0 ? `${days}d ${hours}h` : `${days}d`;
  if (hours > 0) return minutes % 60 > 0 ? `${hours}h ${minutes % 60}m` : `${hours}h`;
  return `${minutes}m`;
}

export function resetText(resetsAt: number | null, now: number, i18n: I18n): string | null {
  if (resetsAt == null) return null;
  const left = resetsAt - now;
  return left <= 60_000 ? i18n.t("resetsSoon") : i18n.t("resetsIn", { t: duration(left) });
}

export function agoText(at: number, now: number, i18n: I18n): string {
  const minutes = Math.floor((now - at) / 60_000);
  if (minutes < 1) return i18n.t("justNow");
  if (minutes < 60) return i18n.t("minutesAgo", { n: minutes });
  return i18n.t("hoursAgo", { n: Math.floor(minutes / 60) });
}
