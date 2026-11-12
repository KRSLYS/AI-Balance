import type { ViewState } from "./api";

const HOUR = 3_600_000;

/** Sample data shown when the UI runs in a plain browser, outside the app. */
export function demoState(): ViewState {
  const now = Date.now();
  return {
    mobile: false,
    version: "demo",
    update: null,
    edition: "full",
    ads: false,
    providers: ["claude", "codex", "cursor", "copilot", "kimi", "grok", "amp", "opencode", "muse", "devin", "copilotorg", "claudeapi", "openrouter", "zai", "ollama"],
    settings: { refreshMinutes: 5, notifications: true, notifyThreshold: 90, language: "system", theme: "system", onboarded: !new URLSearchParams(location.search).has("welcome"), checkUpdates: true },
    accounts: [
      { id: "a", provider: "claude", label: "you@example.com", createdAt: now },
      { id: "a2", provider: "claude", label: "team@example.com", createdAt: now },
      { id: "b", provider: "codex", label: "you@example.com", createdAt: now },
      { id: "c", provider: "cursor", label: "you@example.com", createdAt: now },
      { id: "d", provider: "openrouter", label: "OpenRouter", createdAt: now },
      { id: "e", provider: "copilot", label: "octocat", createdAt: now },
    ],
    snapshots: {
      a: {
        accountId: "a", plan: "Max 5x", fetchedAt: now - 60_000, error: null, needsLogin: false,
        lines: [
          { kind: "progress", label: "Session", used: 93, limit: 100, format: { type: "percent" }, resetsAt: now + 1.2 * HOUR, periodMs: 5 * HOUR },
          { kind: "progress", label: "Weekly", used: 46, limit: 100, format: { type: "percent" }, resetsAt: now + 80 * HOUR, periodMs: 168 * HOUR },
          { kind: "progress", label: "Sonnet", used: 12, limit: 100, format: { type: "percent" }, resetsAt: now + 80 * HOUR, periodMs: 168 * HOUR },
          { kind: "progress", label: "Extra Usage", used: 12.4, limit: 50, format: { type: "currency", code: "USD" }, resetsAt: null, periodMs: null },
        ],
      },
      a2: {
        accountId: "a2", plan: "Team", fetchedAt: now - 60_000, error: null, needsLogin: false,
        lines: [
          { kind: "progress", label: "Session", used: 18, limit: 100, format: { type: "percent" }, resetsAt: now + 4.1 * HOUR, periodMs: 5 * HOUR },
          { kind: "progress", label: "Weekly", used: 71, limit: 100, format: { type: "percent" }, resetsAt: now + 52 * HOUR, periodMs: 168 * HOUR },
        ],
      },
      b: {
        accountId: "b", plan: "Plus", fetchedAt: now - 60_000, error: null, needsLogin: false,
        lines: [
          { kind: "progress", label: "Session", used: 28, limit: 100, format: { type: "percent" }, resetsAt: now + 3.5 * HOUR, periodMs: 5 * HOUR },
          { kind: "progress", label: "Weekly", used: 74, limit: 100, format: { type: "percent" }, resetsAt: now + 30 * HOUR, periodMs: 168 * HOUR },
          { kind: "value", label: "Credits", value: 120, format: { type: "count", suffix: "credits" } },
        ],
      },
      c: {
        accountId: "c", plan: "Pro", fetchedAt: now - 60_000, error: null, needsLogin: false,
        lines: [
          { kind: "progress", label: "Total Usage", used: 58, limit: 100, format: { type: "percent" }, resetsAt: now + 400 * HOUR, periodMs: 720 * HOUR },
          { kind: "progress", label: "On-Demand", used: 4.2, limit: 20, format: { type: "currency", code: "USD" }, resetsAt: now + 400 * HOUR, periodMs: 720 * HOUR },
        ],
      },
      d: {
        accountId: "d", plan: "Pay-as-you-go", fetchedAt: now - 60_000, error: null, needsLogin: false,
        lines: [
          { kind: "value", label: "Balance", value: 18.37, format: { type: "currency", code: "USD" } },
          { kind: "value", label: "Today", value: 0.42, format: { type: "currency", code: "USD" } },
          { kind: "value", label: "This Month", value: 6.9, format: { type: "currency", code: "USD" } },
        ],
      },
      e: {
        accountId: "e", plan: null, fetchedAt: null, error: "Sign-in expired. Sign in again.", needsLogin: true, lines: [],
      },
    },
  };
}
