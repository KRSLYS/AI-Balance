import type { ProviderId } from "./api";

export interface ProviderInfo {
  id: ProviderId;
  name: string;
  color: string;
  /** How an account is added. */
  auth: "browser" | "key";
  /** The key form also asks for a GitHub organization. */
  needsOrganization?: boolean;
  /** Where to create an API key. */
  keyUrl?: string;
  keyHint?: string;
}

export const PROVIDERS: ProviderInfo[] = [
  { id: "claude", name: "Claude", color: "#d97757", auth: "browser" },
  { id: "codex", name: "ChatGPT", color: "#10a37f", auth: "browser" },
  { id: "cursor", name: "Cursor", color: "#6b7280", auth: "browser" },
  { id: "copilot", name: "Copilot", color: "#8250df", auth: "browser" },
  { id: "kimi", name: "Kimi", color: "#1783ff", auth: "browser" },
  { id: "grok", name: "Grok", color: "#18181b", auth: "browser" },
  { id: "amp", name: "Amp", color: "#f0503a", auth: "browser" },
  { id: "opencode", name: "OpenCode", color: "#57534e", auth: "browser" },
  { id: "muse", name: "Muse Code", color: "#0064e0", auth: "browser" },
  { id: "devin", name: "Devin", color: "#1f9d74", auth: "browser" },
  {
    id: "copilotorg",
    name: "Copilot Organization",
    color: "#6e40c9",
    auth: "key",
    needsOrganization: true,
    keyUrl: "https://github.com/settings/personal-access-tokens/new",
    keyHint: "github_pat_…",
  },
  { id: "openrouter", name: "OpenRouter", color: "#6467f2", auth: "key", keyUrl: "https://openrouter.ai/settings/keys", keyHint: "sk-or-…" },
  { id: "zai", name: "Z.ai", color: "#2563eb", auth: "key", keyUrl: "https://z.ai/manage-apikey/apikey-list" },
  { id: "ollama", name: "Ollama", color: "#111827", auth: "key", keyUrl: "https://ollama.com/settings/keys" },
];

export const providerInfo = (id: ProviderId): ProviderInfo => PROVIDERS.find((provider) => provider.id === id) ?? PROVIDERS[0];
