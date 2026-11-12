import { useEffect, useState } from "react";
import { disable, enable, isEnabled } from "@tauri-apps/plugin-autostart";
import { isPermissionGranted, requestPermission } from "@tauri-apps/plugin-notification";
import { api, inApp, type Settings, type ViewState } from "./api";
import { useI18n } from "./i18n";
import { LANGUAGES } from "./locales";

const REPOSITORY = "https://github.com/KRSLYS/AI-Balance";
const WEBSITE = "https://aibalance.krslys.be/";
const SPONSOR = "https://github.com/sponsors/KRSLYS";
const PRIVACY = "https://aibalance.krslys.be/privacy.html";

export default function SettingsView({ state }: { state: ViewState }) {
  const i18n = useI18n();
  const settings = state.settings;
  const update = (patch: Partial<Settings>) => api.setSettings({ ...settings, ...patch });
  const desktop = inApp && !state.mobile;
  const [autostart, setAutostart] = useState(false);

  useEffect(() => {
    if (desktop) isEnabled().then(setAutostart).catch(() => {});
  }, [desktop]);

  // Phones and macOS ask the user before an app may notify; ask when the switch goes on.
  const setNotifications = async (on: boolean) => {
    if (on && inApp) {
      const allowed = (await isPermissionGranted().catch(() => true)) || (await requestPermission().catch(() => "denied")) === "granted";
      if (!allowed) return void update({ notifications: false });
    }
    update({ notifications: on });
  };

  const toggleAutostart = async (on: boolean) => {
    try {
      await (on ? enable() : disable());
      setAutostart(await isEnabled());
    } catch {
      setAutostart(false);
    }
  };

  return (
    <div className="cards">
      <section className="card settings">
        <label>
          {i18n.t("refreshEvery")}
          <select value={settings.refreshMinutes} onChange={(event) => update({ refreshMinutes: Number(event.target.value) })}>
            {[2, 5, 10, 15, 30, 60].map((minutes) => (
              <option key={minutes} value={minutes}>
                {i18n.t("minutes", { n: minutes })}
              </option>
            ))}
          </select>
        </label>
        <label>
          {i18n.t("notifications")}
          <input type="checkbox" checked={settings.notifications} onChange={(event) => setNotifications(event.target.checked)} />
        </label>
        <label>
          {i18n.t("notifyAt")}
          <select value={settings.notifyThreshold} disabled={!settings.notifications} onChange={(event) => update({ notifyThreshold: Number(event.target.value) })}>
            {[70, 80, 90, 95, 100].map((percent) => (
              <option key={percent} value={percent}>
                {percent}%
              </option>
            ))}
          </select>
        </label>
        {state.edition === "full" && (
        <label>
          {i18n.t("checkUpdates")}
          <input type="checkbox" checked={settings.checkUpdates} onChange={(event) => update({ checkUpdates: event.target.checked })} />
        </label>
        )}
        {desktop && (
          <label>
            {i18n.t("launchAtLogin")}
            <input type="checkbox" checked={autostart} onChange={(event) => toggleAutostart(event.target.checked)} />
          </label>
        )}
      </section>
      <section className="card settings">
        <label>
          {i18n.t("language")}
          <select value={settings.language} onChange={(event) => update({ language: event.target.value })}>
            <option value="system">{i18n.t("system")}</option>
            {LANGUAGES.map((language) => (
              <option key={language.code} value={language.code}>
                {language.name}
              </option>
            ))}
          </select>
        </label>
        <label>
          {i18n.t("theme")}
          <select value={settings.theme} onChange={(event) => update({ theme: event.target.value })}>
            <option value="system">{i18n.t("system")}</option>
            <option value="light">{i18n.t("light")}</option>
            <option value="dark">{i18n.t("dark")}</option>
          </select>
        </label>
      </section>
      <section className="card about">
        <strong>AI Balance</strong>
        <span className="muted">
          {i18n.t("version")} {state.version}
        </span>
        <span className="muted">{i18n.t("madeBy")} · MIT</span>
        <span className="muted" dir="ltr">
          KBO/BCE 1026.902.574 · info@krslys.be
        </span>
        <button className="link" onClick={() => api.open(WEBSITE)}>
          {i18n.t("website")} ↗
        </button>
        <button className="link" onClick={() => api.open(REPOSITORY)}>
          {i18n.t("sourceCode")} ↗
        </button>
        {/* The app stores do not allow a link to donations outside their own payment systems. */}
        {state.edition === "full" && (
          <button className="link" onClick={() => api.open(SPONSOR)}>
            ♥ {i18n.t("support")} ↗
          </button>
        )}
        <button className="link" onClick={() => api.open(PRIVACY)}>
          {i18n.t("privacyPolicy")} ↗
        </button>
      </section>
    </div>
  );
}
