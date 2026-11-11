import type { Account, MetricLine, ProviderId, Snapshot, ViewState } from "./api";
import { agoText, level, lineValue, resetText, usedPercent } from "./format";
import { useI18n } from "./i18n";
import { providerInfo } from "./providers";

interface Props {
  state: ViewState;
  now: number;
  onAdd: (provider?: ProviderId | null) => void;
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
  return (
    <div className="cards">
      {state.accounts.map((account) => (
        <AccountCard key={account.id} account={account} snapshot={state.snapshots[account.id]} now={now} onAdd={onAdd} />
      ))}
    </div>
  );
}

function AccountCard({ account, snapshot, now, onAdd }: { account: Account; snapshot?: Snapshot; now: number; onAdd: Props["onAdd"] }) {
  const i18n = useI18n();
  const provider = providerInfo(account.provider);
  const lines = snapshot?.lines ?? [];
  return (
    <section className="card">
      <header className="card-head">
        <span className="dot" style={{ background: provider.color }} />
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
