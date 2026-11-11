import { useEffect, useRef, useState } from "react";
import { api, type LoginPrompt, type ProviderId } from "./api";
import { useI18n } from "./i18n";
import { PROVIDERS, providerInfo } from "./providers";

interface Props {
  initial: ProviderId | null;
  onClose: () => void;
}

export default function AddAccount({ initial, onClose }: Props) {
  const i18n = useI18n();
  const [provider, setProvider] = useState<ProviderId | null>(initial);
  const info = provider ? providerInfo(provider) : null;
  return (
    <div className="overlay" onClick={onClose}>
      <div className="sheet" onClick={(event) => event.stopPropagation()} role="dialog" aria-modal>
        <header className="sheet-head">
          {provider && !initial ? (
            <button className="ghost" onClick={() => setProvider(null)}>
              ‹ {i18n.t("back")}
            </button>
          ) : (
            <span />
          )}
          <strong>{info ? info.name : i18n.t("chooseProvider")}</strong>
          <button className="ghost" onClick={onClose}>
            {i18n.t("close")}
          </button>
        </header>
        {!info && (
          <div className="grid">
            {PROVIDERS.map((item) => (
              <button key={item.id} className="tile" onClick={() => setProvider(item.id)}>
                <span className="dot" style={{ background: item.color }} />
                {item.name}
              </button>
            ))}
          </div>
        )}
        {info?.auth === "key" && <KeyForm provider={info.id} onDone={onClose} />}
        {info?.auth === "browser" && <BrowserLogin key={info.id} provider={info.id} onDone={onClose} />}
      </div>
    </div>
  );
}

function KeyForm({ provider, onDone }: { provider: ProviderId; onDone: () => void }) {
  const i18n = useI18n();
  const info = providerInfo(provider);
  const [key, setKey] = useState("");
  const [name, setName] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const submit = async (event: React.FormEvent) => {
    event.preventDefault();
    setBusy(true);
    setError(null);
    try {
      await api.addApiKeyAccount(provider, key, name || undefined);
      onDone();
    } catch (failure) {
      setError(String(failure));
    } finally {
      setBusy(false);
    }
  };

  return (
    <form className="form" onSubmit={submit}>
      <label>
        {i18n.t("apiKey")}
        <input
          type="password"
          autoFocus
          autoComplete="off"
          spellCheck={false}
          dir="ltr"
          placeholder={info.keyHint ?? i18n.t("pasteKey")}
          value={key}
          onChange={(event) => setKey(event.target.value)}
        />
      </label>
      <label>
        {i18n.t("nameOptional")}
        <input value={name} onChange={(event) => setName(event.target.value)} />
      </label>
      {error && <div className="notice">{error}</div>}
      <div className="actions end">
        {info.keyUrl && (
          <button type="button" className="link" onClick={() => api.open(info.keyUrl!)}>
            {i18n.t("getKey")} ↗
          </button>
        )}
        <button className="primary" disabled={busy || !key.trim()}>
          {busy ? i18n.t("checking") : i18n.t("add")}
        </button>
      </div>
    </form>
  );
}

function BrowserLogin({ provider, onDone }: { provider: ProviderId; onDone: () => void }) {
  const i18n = useI18n();
  const [prompt, setPrompt] = useState<LoginPrompt | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [code, setCode] = useState("");
  const [copied, setCopied] = useState(false);
  const session = useRef<string | null>(null);
  const finished = useRef(false);

  useEffect(() => {
    let cancelled = false;
    const stop = api.onLoginFinished((result) => {
      if (result.sessionId !== session.current) return;
      finished.current = true;
      if (result.error) setError(result.error);
      else onDone();
    });
    api
      .beginLogin(provider)
      .then((started) => {
        if (cancelled) return void api.cancelLogin(started.sessionId);
        session.current = started.sessionId;
        setPrompt(started);
        api.open(started.url).catch(() => {});
      })
      .catch((failure) => !cancelled && setError(String(failure)));
    return () => {
      cancelled = true;
      stop();
      if (session.current && !finished.current) api.cancelLogin(session.current);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [provider]);

  const copy = async () => {
    if (!prompt?.userCode) return;
    await navigator.clipboard.writeText(prompt.userCode).catch(() => {});
    setCopied(true);
    setTimeout(() => setCopied(false), 1500);
  };

  const submit = async (event: React.FormEvent) => {
    event.preventDefault();
    if (!prompt || !code.trim()) return;
    try {
      await api.submitLoginCode(prompt.sessionId, code.trim());
    } catch (failure) {
      setError(String(failure));
    }
  };

  if (error) return <div className="form"><div className="notice">{error}</div></div>;
  if (!prompt) return <div className="form"><div className="skeleton" /></div>;

  return (
    <div className="form">
      {prompt.userCode ? (
        <>
          <p>{i18n.t("deviceCode")}</p>
          <div className="code-box">
            <code dir="ltr">{prompt.userCode}</code>
            <button className="ghost" onClick={copy}>
              {copied ? i18n.t("copied") : i18n.t("copy")}
            </button>
          </div>
        </>
      ) : (
        <p>{i18n.t("signInBrowser")}</p>
      )}
      <button className="ghost" onClick={() => api.open(prompt.url)}>
        {i18n.t("openPage")} ↗
      </button>
      {prompt.acceptsPaste && (
        <form className="paste" onSubmit={submit}>
          <label>
            {i18n.t("pasteCode")}
            <input dir="ltr" autoComplete="off" spellCheck={false} value={code} onChange={(event) => setCode(event.target.value)} />
          </label>
          <button className="primary small" disabled={!code.trim()}>
            {i18n.t("submit")}
          </button>
        </form>
      )}
      <p className="muted waiting">
        <span className="spinner" /> {i18n.t("waiting")}
      </p>
    </div>
  );
}
