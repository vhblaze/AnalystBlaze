// Traduções por tela (doc 15 §2): src/i18n/pt/*.ts e src/i18n/en/*.ts.
// A checagem do CI (scripts/check-i18n.mjs) falha se pt e en divergirem, se o
// código usar uma chave que não existe ou se sobrar chave que ninguém usa.
import { common as pt_common } from "./pt/common";
import { shell as pt_shell } from "./pt/shell";
import { dashboard as pt_dashboard } from "./pt/dashboard";
import { history as pt_history } from "./pt/history";
import { insights as pt_insights } from "./pt/insights";
import { controls as pt_controls } from "./pt/controls";
import { mypc as pt_mypc } from "./pt/mypc";
import { settings as pt_settings } from "./pt/settings";
import { common as en_common } from "./en/common";
import { shell as en_shell } from "./en/shell";
import { dashboard as en_dashboard } from "./en/dashboard";
import { history as en_history } from "./en/history";
import { insights as en_insights } from "./en/insights";
import { controls as en_controls } from "./en/controls";
import { mypc as en_mypc } from "./en/mypc";
import { settings as en_settings } from "./en/settings";

export type Locale = "pt-BR" | "en-US";

export const DEFAULT_LOCALE: Locale = "pt-BR";
export const LOCALE_STORAGE_KEY = "analystblaze.locale";

export const localeLabels: Record<Locale, string> = {
  "pt-BR": "Português",
  "en-US": "English"
};

export const translations = {
  "pt-BR": {
    ...pt_common,
    ...pt_shell,
    ...pt_dashboard,
    ...pt_history,
    ...pt_insights,
    ...pt_controls,
    ...pt_mypc,
    ...pt_settings,
  },
  "en-US": {
    ...en_common,
    ...en_shell,
    ...en_dashboard,
    ...en_history,
    ...en_insights,
    ...en_controls,
    ...en_mypc,
    ...en_settings,
  },
} as const;
