import { useEffect, useMemo, useState } from "react";
import { api, inApp, type ProviderId, type ViewState } from "./api";
import { demoState } from "./demo";
import { I18nContext, makeI18n, resolveLang } from "./i18n";
import Dashboard from "./Dashboard";
import AccountsView from "./AccountsView";
import SettingsView from "./SettingsView";
import AddAccount from "./AddAccount";
import Welcome from "./Welcome";

type Tab = "usage" | "accounts" | "settings";

/** Which provider the add-account sheet starts on; `null` starts on the provider list. */
export type AddTarget = { provider: ProviderId | null };

export default function App() {
  const [state, setState] = useState<ViewState | null>(null);
  const [tab, setTab] = useState<Tab>("usage");
  const [adding, setAdding] = useState<AddTarget | null>(null);
  const [refreshing, setRefreshing] = useState(false);
  const [now, setNow] = useState(Date.now());
  // Sample data lets anyone, a store reviewer included, see the app working without an account.
  const [sample, setSample] = useState(false);

  useEffect(() => {
    api.getState().then(setState);
    const stop = api.onState(setState);
    const clock = setInterval(() => setNow(Date.now()), 30_000);
    return () => {
      stop();
      clearInterval(clock);
    };
  }, []);

  const language = state?.settings.language ?? "system";
  const theme = state?.settings.theme ?? "system";
  const i18n = useMemo(() => makeI18n(resolveLang(language)), [language]);

  useEffect(() => {
    document.documentElement.lang = i18n.lang;
    document.documentElement.dir = i18n.rtl ? "rtl" : "ltr";
  }, [i18n]);

  useEffect(() => {
    if (theme === "system") delete document.documentElement.dataset.theme;
    else document.documentElement.dataset.theme = theme;
  }, [theme]);

  const refresh = async () => {
    if (refreshing) return;
    setRefreshing(true);
    try {
      await api.refresh();
      setNow(Date.now());
    } finally {
      setRefreshing(false);
    }
  };

  if (!state) return <div className="app" />;
  if (!state.settings.onboarded) return <Welcome settings={state.settings} />;
  const add = (provider: ProviderId | null = null) => setAdding({ provider });
  const shown: ViewState = sample ? { ...demoState(), settings: state.settings, mobile: state.mobile, version: state.version } : state;
  const tabs: Tab[] = ["usage", "accounts", "settings"];

  return (
    <I18nContext.Provider value={i18n}>
      <div className="app">
        <header className="topbar">
          <h1>{i18n.t(tab)}</h1>
          {tab === "usage" && shown.accounts.length > 0 && !sample && (
            <button className={`ghost refresh ${refreshing ? "spinning" : ""}`} onClick={refresh} disabled={refreshing}>
              <svg viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="currentColor" strokeWidth="2.2" strokeLinecap="round" strokeLinejoin="round" aria-hidden>
                <path d="M20 11a8 8 0 10-2.3 6.3M20 4v7h-7" />
              </svg>
              {refreshing ? i18n.t("refreshing") : i18n.t("refresh")}
            </button>
          )}
          {tab === "accounts" && (
            <button className="primary small" onClick={() => add()}>
              + {i18n.t("addAccount")}
            </button>
          )}
        </header>
        {!inApp && !sample && <div className="banner">{i18n.t("demoBanner")}</div>}
        {sample && (
          <div className="banner">
            <span>{i18n.t("sampleBanner")}</span>
            <button className="link" onClick={() => setSample(false)}>
              {i18n.t("exitSample")}
            </button>
          </div>
        )}
        <main className="content" key={tab}>
          {tab === "usage" && <Dashboard state={shown} now={now} onAdd={add} onSample={sample ? undefined : () => setSample(true)} />}
          {tab === "accounts" && <AccountsView state={state} onAdd={add} />}
          {tab === "settings" && <SettingsView state={state} />}
        </main>
        <nav className="tabs">
          {tabs.map((id) => (
            <button key={id} className={tab === id ? "active" : ""} onClick={() => setTab(id)}>
              <TabIcon tab={id} />
              <span>{i18n.t(id)}</span>
            </button>
          ))}
        </nav>
        {adding && <AddAccount initial={adding.provider} onClose={() => setAdding(null)} />}
      </div>
    </I18nContext.Provider>
  );
}

function TabIcon({ tab }: { tab: Tab }) {
  const paths: Record<Tab, string> = {
    usage: "M4 19V10m6 9V5m6 14v-7",
    accounts: "M12 12a4 4 0 100-8 4 4 0 000 8zm-7 8a7 7 0 0114 0",
    settings: "M4 7h10m4 0h2M4 17h2m4 0h10M16 5v4M8 15v4",
  };
  return (
    <svg viewBox="0 0 24 24" width="22" height="22" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" aria-hidden>
      <path d={paths[tab]} />
    </svg>
  );
}
