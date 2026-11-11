import { useState } from "react";
import { api, type Account, type ProviderId, type ViewState } from "./api";
import { useI18n } from "./i18n";
import { providerInfo } from "./providers";

interface Props {
  state: ViewState;
  onAdd: (provider?: ProviderId | null) => void;
}

export default function AccountsView({ state, onAdd }: Props) {
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
      {state.accounts.map((account, index) => (
        <AccountRow
          key={account.id}
          account={account}
          first={index === 0}
          last={index === state.accounts.length - 1}
          needsLogin={state.snapshots[account.id]?.needsLogin ?? false}
          onAdd={onAdd}
        />
      ))}
      <p className="muted center">{i18n.t("privacy")}</p>
    </div>
  );
}

function AccountRow({ account, first, last, needsLogin, onAdd }: { account: Account; first: boolean; last: boolean; needsLogin: boolean; onAdd: Props["onAdd"] }) {
  const i18n = useI18n();
  const provider = providerInfo(account.provider);
  const [name, setName] = useState<string | null>(null);
  const [confirming, setConfirming] = useState(false);

  const saveName = async () => {
    if (name?.trim()) await api.renameAccount(account.id, name.trim());
    setName(null);
  };

  return (
    <section className="card">
      <header className="card-head">
        <span className="avatar" style={{ background: provider.color }} aria-hidden>
          {provider.name[0]}
        </span>
        <div className="card-title">
          <strong>{provider.name}</strong>
          {name === null ? (
            <span className="muted ellipsis">{account.label}</span>
          ) : (
            <input autoFocus value={name} onChange={(event) => setName(event.target.value)} onKeyDown={(event) => event.key === "Enter" && saveName()} />
          )}
        </div>
        <div className="order">
          <button className="icon" disabled={first} onClick={() => api.moveAccount(account.id, -1)} aria-label={i18n.t("moveUp")} title={i18n.t("moveUp")}>
            ↑
          </button>
          <button className="icon" disabled={last} onClick={() => api.moveAccount(account.id, 1)} aria-label={i18n.t("moveDown")} title={i18n.t("moveDown")}>
            ↓
          </button>
        </div>
      </header>
      <div className="actions">
        {name === null ? (
          <button className="ghost" onClick={() => setName(account.label)}>
            {i18n.t("rename")}
          </button>
        ) : (
          <>
            <button className="primary small" onClick={saveName}>
              {i18n.t("save")}
            </button>
            <button className="ghost" onClick={() => setName(null)}>
              {i18n.t("cancel")}
            </button>
          </>
        )}
        <button className={needsLogin ? "primary small" : "ghost"} onClick={() => onAdd(account.provider)}>
          {i18n.t(provider.auth === "key" ? "replaceKey" : "signInAgain")}
        </button>
        {confirming ? (
          <>
            <button className="danger" onClick={() => api.removeAccount(account.id)}>
              {i18n.t("removeConfirm")}
            </button>
            <button className="ghost" onClick={() => setConfirming(false)}>
              {i18n.t("cancel")}
            </button>
          </>
        ) : (
          <button className="ghost danger-text" onClick={() => setConfirming(true)}>
            {i18n.t("remove")}
          </button>
        )}
      </div>
    </section>
  );
}
