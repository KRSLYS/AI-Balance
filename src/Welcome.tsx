import { useEffect, useMemo, useState } from "react";
import { api, type Settings } from "./api";
import { makeI18n, resolveLang, type Lang } from "./i18n";
import { LANGUAGES } from "./locales";
import logo from "../assets/icon.svg";

/** The first-run screen: pick a language, then start. */
export default function Welcome({ settings }: { settings: Settings }) {
  const [lang, setLang] = useState<Lang>(() => resolveLang(settings.language));
  const i18n = useMemo(() => makeI18n(lang), [lang]);

  // The screen speaks the highlighted language at once, direction included.
  useEffect(() => {
    document.documentElement.lang = i18n.lang;
    document.documentElement.dir = i18n.rtl ? "rtl" : "ltr";
  }, [i18n]);

  return (
    <div className="app welcome">
      <img className="welcome-logo" src={logo} alt="" width={88} height={88} />
      <h1 key={`title-${lang}`}>{i18n.t("welcomeTitle")}</h1>
      <p key={`body-${lang}`} className="muted">
        {i18n.t("welcomeBody")}
      </p>
      <div className="languages" role="radiogroup" aria-label={i18n.t("language")}>
        {LANGUAGES.map((language, index) => (
          <button
            key={language.code}
            role="radio"
            aria-checked={language.code === lang}
            lang={language.code}
            dir={language.rtl ? "rtl" : "ltr"}
            className={`language ${language.code === lang ? "selected" : ""}`}
            style={{ animationDelay: `${120 + index * 35}ms` }}
            onClick={() => setLang(language.code)}
          >
            {language.name}
          </button>
        ))}
      </div>
      <button className="primary wide" onClick={() => api.setSettings({ ...settings, language: lang, onboarded: true })}>
        {i18n.t("continue")}
      </button>
    </div>
  );
}
