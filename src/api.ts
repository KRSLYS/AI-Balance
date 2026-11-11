import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { openUrl } from "@tauri-apps/plugin-opener";
import { demoState } from "./demo";

export type ProviderId = "claude" | "codex" | "copilot" | "cursor" | "kimi" | "openrouter" | "zai" | "ollama";

export type Format =
  | { type: "percent" }
  | { type: "currency"; code: string }
  | { type: "count"; suffix: string };

export type MetricLine =
  | { kind: "progress"; label: string; used: number; limit: number; format: Format; resetsAt: number | null; periodMs: number | null }
  | { kind: "value"; label: string; value: number; format: Format };

export interface Account {
  id: string;
  provider: ProviderId;
  label: string;
  createdAt: number;
}

export interface Snapshot {
  accountId: string;
  plan: string | null;
  lines: MetricLine[];
  fetchedAt: number | null;
  error: string | null;
  needsLogin: boolean;
}

export interface Settings {
  refreshMinutes: number;
  notifications: boolean;
  notifyThreshold: number;
  language: string;
  theme: string;
}

export interface ViewState {
  accounts: Account[];
  snapshots: Record<string, Snapshot>;
  settings: Settings;
  mobile: boolean;
  version: string;
}

export interface LoginPrompt {
  sessionId: string;
  url: string;
  userCode: string | null;
  acceptsPaste: boolean;
}

export interface LoginFinished {
  sessionId: string;
  account: Account | null;
  error: string | null;
}

/** False in a plain browser (`npm run dev`), where the app shows demo data. */
export const inApp = "__TAURI_INTERNALS__" in window;

let demo = demoState();
const demoListeners = new Set<(state: ViewState) => void>();
const demoChanged = () => demoListeners.forEach((listener) => listener(demo));

export const api = {
  getState: (): Promise<ViewState> => (inApp ? invoke("get_state") : Promise.resolve(demo)),
  refresh: (): Promise<void> => (inApp ? invoke("refresh") : new Promise((done) => setTimeout(done, 600))),
  addApiKeyAccount: (provider: ProviderId, key: string, label?: string): Promise<Account> =>
    inApp ? invoke("add_api_key_account", { provider, key, label }) : Promise.reject("Demo mode: run the app to add accounts."),
  beginLogin: (provider: ProviderId): Promise<LoginPrompt> =>
    inApp
      ? invoke("begin_login", { provider })
      : Promise.resolve({
          sessionId: "demo",
          url: `https://example.com/sign-in/${provider}?code=DEMO-1234`,
          userCode: provider === "copilot" ? "DEMO-1234" : null,
          acceptsPaste: provider === "claude",
        }),
  submitLoginCode: (sessionId: string, code: string): Promise<void> =>
    inApp ? invoke("submit_login_code", { sessionId, code }) : Promise.reject("Demo mode: run the app to add accounts."),
  cancelLogin: (sessionId: string): Promise<void> => (inApp ? invoke("cancel_login", { sessionId }) : Promise.resolve()),
  removeAccount: async (id: string): Promise<void> => {
    if (inApp) return invoke("remove_account", { id });
    demo = { ...demo, accounts: demo.accounts.filter((account) => account.id !== id) };
    demoChanged();
  },
  renameAccount: async (id: string, label: string): Promise<void> => {
    if (inApp) return invoke("rename_account", { id, label });
    demo = { ...demo, accounts: demo.accounts.map((account) => (account.id === id ? { ...account, label } : account)) };
    demoChanged();
  },
  moveAccount: async (id: string, offset: number): Promise<void> => {
    if (inApp) return invoke("move_account", { id, offset });
    const accounts = [...demo.accounts];
    const from = accounts.findIndex((account) => account.id === id);
    const to = Math.max(0, Math.min(accounts.length - 1, from + offset));
    accounts.splice(to, 0, ...accounts.splice(from, 1));
    demo = { ...demo, accounts };
    demoChanged();
  },
  setSettings: async (settings: Settings): Promise<Settings> => {
    if (inApp) return invoke("set_settings", { settings });
    demo = { ...demo, settings };
    demoChanged();
    return settings;
  },
  onState: (handler: (state: ViewState) => void): (() => void) => {
    if (!inApp) {
      demoListeners.add(handler);
      return () => demoListeners.delete(handler);
    }
    const stop = listen<ViewState>("state-changed", (event) => handler(event.payload));
    return () => void stop.then((unlisten) => unlisten());
  },
  onLoginFinished: (handler: (result: LoginFinished) => void): (() => void) => {
    if (!inApp) return () => {};
    const stop = listen<LoginFinished>("login-finished", (event) => handler(event.payload));
    return () => void stop.then((unlisten) => unlisten());
  },
  /** Opens a link in the browser. The demo opens nothing. */
  open: (url: string): Promise<void> => (inApp ? openUrl(url) : Promise.resolve()),
};
