import { useEffect, useMemo, useState } from "react";
import { api, type MetricLine, type ViewState } from "./api";
import { duration, level, lineValue, usedPercent } from "./format";
import { I18nContext, makeI18n, resolveLang, useI18n } from "./i18n";
import { providerInfo } from "./providers";

type Tab = "priority" | "all";

/** The small panel that opens beside the tray icon: every limit at a glance. */
export default function Panel() {
  const [state, setState] = useState<ViewState | null>(null);
  const [tab, setTab] = useState<Tab>("priority");
  const [now, setNow] = useState(Date.now());
  const [refreshing, setRefreshing] = useState(false);

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

  if (!state) return <div className="panel" />;

  const refresh = async () => {
    setRefreshing(true);
    try {
      await api.refresh();
      setNow(Date.now());
    } finally {
      setRefreshing(false);
    }
  };

  return (
    <I18nContext.Provider value={i18n}>
      <div className="panel">
        <div className="segments" role="tablist">
          {(["priority", "all"] as const).map((id) => (
            <button key={id} role="tab" aria-selected={tab === id} className={tab === id ? "active" : ""} onClick={() => setTab(id)}>
              {i18n.t(id === "priority" ? "priority" : "allUsage")}
            </button>
          ))}
        </div>
        <div className="panel-body" key={tab}>
          {state.accounts.length === 0 && <p className="muted center">{i18n.t("emptyTitle")}</p>}
          {tab === "priority" ? <Priority state={state} now={now} /> : <All state={state} now={now} />}
        </div>
        <footer className="panel-foot">
          <button className="link" onClick={() => api.showMain()}>
            {i18n.t("openApp")}
          </button>
          <button className={`ghost refresh ${refreshing ? "spinning" : ""}`} onClick={refresh} disabled={refreshing} aria-label={i18n.t("refresh")} title={i18n.t("refresh")}>
            <svg viewBox="0 0 24 24" width="15" height="15" fill="none" stroke="currentColor" strokeWidth="2.2" strokeLinecap="round" strokeLinejoin="round" aria-hidden>
              <path d="M20 11a8 8 0 10-2.3 6.3M20 4v7h-7" />
            </svg>
          </button>
        </footer>
      </div>
    </I18nContext.Provider>
  );
}

/** The limits closest to running out, across every account. */
function Priority({ state, now }: { state: ViewState; now: number }) {
  const i18n = useI18n();
  const limits = state.accounts
    .flatMap((account) =>
      (state.snapshots[account.id]?.lines ?? []).map((line) => ({ account, line, percent: usedPercent(line) })),
    )
    .filter((entry): entry is typeof entry & { percent: number } => entry.percent != null)
    .sort((a, b) => b.percent - a.percent)
    .slice(0, 8);
  const needLogin = state.accounts.filter((account) => state.snapshots[account.id]?.needsLogin);
  return (
    <>
      {limits.map(({ account, line }) => (
        <CompactRow key={`${account.id}/${line.label}`} line={line} now={now} title={`${providerInfo(account.provider).name} · ${i18n.label(line.label)}`} hint={account.label} />
      ))}
      {needLogin.map((account) => (
        <div key={account.id} className="notice">
          {providerInfo(account.provider).name} · {account.label}: {i18n.t("signInAgain")}
        </div>
      ))}
    </>
  );
}

function All({ state, now }: { state: ViewState; now: number }) {
  const i18n = useI18n();
  return (
    <>
      {state.accounts.map((account) => {
        const snapshot = state.snapshots[account.id];
        const provider = providerInfo(account.provider);
        return (
          <section key={account.id} className="panel-group">
            <header>
              <span className="avatar small" style={{ background: provider.color }} aria-hidden>
                {provider.name[0]}
              </span>
              <strong>{provider.name}</strong>
              <span className="muted ellipsis">{account.label}</span>
              {snapshot?.plan && <span className="chip">{snapshot.plan}</span>}
            </header>
            {snapshot?.error && <div className="notice">{snapshot.error}</div>}
            {(snapshot?.lines ?? []).map((line) => (
              <CompactRow key={line.label} line={line} now={now} title={i18n.label(line.label)} />
            ))}
          </section>
        );
      })}
    </>
  );
}

/** One line: name, bar, value, time until the reset. */
function CompactRow({ line, now, title, hint }: { line: MetricLine; now: number; title: string; hint?: string }) {
  const i18n = useI18n();
  const percent = usedPercent(line);
  const left = line.kind === "progress" && line.resetsAt != null ? line.resetsAt - now : null;
  return (
    <div className="compact" title={hint}>
      <span className="compact-name ellipsis">{title}</span>
      {percent != null ? (
        <div className="bar">
          <div className={`fill ${level(percent)}`} style={{ width: `${percent}%` }} />
        </div>
      ) : (
        <span />
      )}
      <span className="value">{lineValue(line, i18n)}</span>
      <span className="muted compact-reset">{left != null && left > 0 ? duration(left) : ""}</span>
    </div>
  );
}
