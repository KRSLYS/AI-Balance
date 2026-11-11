import { createContext, useContext } from "react";
import { LANGUAGES, type LangCode, type Locale } from "./locales";

export type Key = keyof Locale["strings"];
export type Lang = LangCode;

/** The language to show: the one chosen in Settings, or the device's when it is one the app speaks. */
export function resolveLang(setting: string): Lang {
  const wanted = (setting === "system" ? navigator.language : setting).toLowerCase().split("-")[0];
  return LANGUAGES.find((language) => language.code === wanted)?.code ?? "en";
}

export interface I18n {
  lang: Lang;
  rtl: boolean;
  t: (key: Key, vars?: Record<string, string | number>) => string;
  /** A metric label the providers report in English, in this language. Unknown labels stay as they are. */
  label: (label: string) => string;
}

export function makeI18n(lang: Lang): I18n {
  const language = LANGUAGES.find((candidate) => candidate.code === lang) ?? LANGUAGES[0];
  const { strings, labels } = language.locale;
  return {
    lang: language.code,
    rtl: language.rtl,
    t: (key, vars = {}) => Object.entries(vars).reduce((text, [name, value]) => text.replace(`{${name}}`, String(value)), strings[key] as string),
    label: (label) => (labels as Record<string, string>)[label] ?? label,
  };
}

export const I18nContext = createContext<I18n>(makeI18n("en"));
export const useI18n = () => useContext(I18nContext);
