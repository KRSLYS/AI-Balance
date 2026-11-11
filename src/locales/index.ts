import { ar } from "./ar";
import { de } from "./de";
import { en, type Locale } from "./en";
import { es } from "./es";
import { fr } from "./fr";
import { it } from "./it";
import { nl } from "./nl";
import { pl } from "./pl";
import { pt } from "./pt";
import { ru } from "./ru";
import { tr } from "./tr";
import { zh } from "./zh";

export type { Locale };

/** Every language the app speaks, with the name its own speakers use. To add one, add its file and a line here. */
export const LANGUAGES = [
  { code: "en", name: "English", locale: en, rtl: false },
  { code: "ar", name: "العربية", locale: ar, rtl: true },
  { code: "zh", name: "简体中文", locale: zh, rtl: false },
  { code: "fr", name: "Français", locale: fr, rtl: false },
  { code: "de", name: "Deutsch", locale: de, rtl: false },
  { code: "es", name: "Español", locale: es, rtl: false },
  { code: "it", name: "Italiano", locale: it, rtl: false },
  { code: "pt", name: "Português", locale: pt, rtl: false },
  { code: "nl", name: "Nederlands", locale: nl, rtl: false },
  { code: "pl", name: "Polski", locale: pl, rtl: false },
  { code: "ru", name: "Русский", locale: ru, rtl: false },
  { code: "tr", name: "Türkçe", locale: tr, rtl: false },
] as const;

export type LangCode = (typeof LANGUAGES)[number]["code"];
