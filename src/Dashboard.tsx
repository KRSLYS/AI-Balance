import type { Account, MetricLine, ProviderId, Snapshot, ViewState } from "./api";
import { agoText, level, lineValue, resetText, usedPercent } from "./format";
import { useI18n } from "./i18n";
import { providerInfo } from "./providers";

interface Props {
  state: ViewState;
  now: number;
  onAdd: (provider?: ProviderId | null) => void;
}

/** Accounts of one provider, kept together in the order the first of them appears. */
function groupByProvider(accounts: Account[]): Account[][] {
  const groups = new Map<ProviderId, Account[]>();
  for (const account of accounts) groups.set(account.provider, [...(groups.get(account.provider) ?? []), account]);
  return [...groups.values()];
}

export default function Dashboard({ state, now, onAdd }: Props) {
  const i18n = useI18n();
  if (state.accounts.length === 0) {
    return (
      <div className="empty">
        <h2>{i18n.t("emptyTitle")}</h2>
        <p>{i18n.t("emptyBody")}</p>
        <button className="primary" onClick={() => onAdd()}>
          {i18n.t("addAccount")}
        </button>
      </div>
    );
  }
  let index = 0;
  return (
    <div className="cards">
      <Summary state={state} />
      {groupByProvider(state.accounts).map((group) => {
        const provider = providerInfo(group[0].provider);
        return (
          <div className="group" key={provider.id}>
            {group.length > 1 && (
              <h2 className="group-title">
                {provider.name}
                <span className="chip">{i18n.t("accountsN", { n: group.length })}</span>
              </h2>
            )}
            {group.map((account) => (
              <AccountCard key={account.id} account={account} snapshot={state.snapshots[account.id]} now={now} onAdd={onAdd} order={index++} />
            ))}
          </div>
        );
      })}
    </div>
  );
}

/** The fullest limit across every account, and how many limits are close to running out. */
function Summary({ state }: { state: ViewState }) {
  const i18n = useI18n();
  let fullest: { percent: number; name: string } | null = null;
  let low = 0;
  for (const account of state.accounts) {
    for (const line of state.snapshots[account.id]?.lines ?? []) {
      const percent = usedPercent(line);
      if (percent == null) continue;
      if (percent >= 90) low += 1;
      if (!fullest || percent > fullest.percent) fullest = { percent, name: `${providerInfo(account.provider).name} · ${i18n.label(line.label)}` };
    }
  }
  if (!fullest) return null;
  const radius = 26;
  const circumference = 2 * Math.PI * radius;
  return (
    <section className={`summary ${level(fullest.percent)}`}>
      <div className="summary-text">
        <strong>{low > 0 ? i18n.t("runningLow", { n: low }) : i18n.t("allGood")}</strong>
        <span>{fullest.name}</span>
      </div>
      <div className="ring">
        <svg viewBox="0 0 64 64" width="64" height="64" aria-hidden>
          <circle cx="32" cy="32" r={radius} className="ring-track" />
          <circle
            cx="32"
            cy="32"
            r={radius}
            className="ring-fill"
            strokeDasharray={circumference}
            strokeDashoffset={circumference * (1 - fullest.percent / 100)}
            style={{ ["--ring-length" as string]: circumference }}
          />
        </svg>
        <span>{Math.round(fullest.percent)}%</span>
      </div>
    </section>
  );
}

function AccountCard({ account, snapshot, now, onAdd, order }: { account: Account; snapshot?: Snapshot; now: number; onAdd: Props["onAdd"]; order: number }) {
  const i18n = useI18n();
  const provider = providerInfo(account.provider);
  const lines = snapshot?.lines ?? [];
  return (
    <section className="card rise" style={{ animationDelay: `${Math.min(order, 8) * 60}ms` }}>
      <header className="card-head">
        <span className="avatar" style={{ background: provider.color }} aria-hidden>
          {provider.name[0]}
        </span>
        <div className="card-title">
          <strong>{provider.name}</strong>
          <span className="muted ellipsis">{account.label}</span>
        </div>
        {snapshot?.plan && <span className="chip">{snapshot.plan}</span>}
      </header>
      {snapshot?.error && (
        <div className="notice">
          <span>{snapshot.error}</span>
          {snapshot.needsLogin && (
            <button className="link" onClick={() => onAdd(account.provider)}>
              {i18n.t(provider.auth === "key" ? "replaceKey" : "signInAgain")}
            </button>
          )}
        </div>
      )}
      {lines.map((line) => (
        <Row key={line.label} line={line} now={now} />
      ))}
      {!snapshot && <div className="skeleton" />}
      {snapshot && lines.length === 0 && !snapshot.error && <p className="muted">{i18n.t("noUsage")}</p>}
      {snapshot?.fetchedAt && (
        <footer className="card-foot muted">
          {i18n.t("updated")} {agoText(snapshot.fetchedAt, now, i18n)}
        </footer>
      )}
    </section>
  );
}

function Row({ line, now }: { line: MetricLine; now: number }) {
  const i18n = useI18n();
  const percent = usedPercent(line);
  const reset = line.kind === "progress" ? resetText(line.resetsAt, now, i18n) : null;
  return (
    <div className="row">
      <div className="row-top">
        <span>{i18n.label(line.label)}</span>
        <span className="value">{lineValue(line, i18n)}</span>
      </div>
      {percent != null && (
        <div className="bar" role="progressbar" aria-valuenow={Math.round(percent)} aria-valuemin={0} aria-valuemax={100}>
          <div className={`fill ${level(percent)}`} style={{ width: `${percent}%` }} />
        </div>
      )}
      {reset && <div className="row-sub muted">{reset}</div>}
    </div>
  );
}
