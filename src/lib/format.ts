/**
 * Formatação única de números (doc 15 §2: "nenhum número é formatado dentro
 * de uma tela"). Um só arredondamento para o mesmo dado em todas as telas
 * (doc 14 Q4: 448 GB × 447 GB).
 *
 * Regras:
 * - Bytes em base 1024 com o rótulo que o Windows usa (KB, MB, GB, TB);
 *   abaixo de 10 uma casa decimal, de 10 para cima nenhuma.
 * - Separador decimal do idioma do app (vírgula em pt-BR).
 * - `MISSING` marca valor ausente. Telas novas trocam por <EmptyState/>;
 *   ele existe para as telas antigas e para ser fácil de achar.
 */

export const MISSING = "--";

let currentLocale = "pt-BR";

/** Chamado pelo I18nProvider quando o idioma muda. */
export function setFormatLocale(locale: string) {
  currentLocale = locale;
}

function isNumber(value: unknown): value is number {
  return typeof value === "number" && Number.isFinite(value);
}

function decimal(value: number, digits: number): string {
  return value.toLocaleString(currentLocale, { minimumFractionDigits: digits, maximumFractionDigits: digits });
}

/** Número com no máximo uma casa abaixo de 10 ("4,9", "28", "448"). */
export function formatNumber(value: number): string {
  return decimal(value, Math.abs(value) < 10 && !Number.isInteger(value) ? 1 : 0);
}

const BYTE_UNITS = ["B", "KB", "MB", "GB", "TB"] as const;

/** Bytes: "0 B", "512 MB", "4,8 GB", "448 GB". */
export function formatBytes(bytes: number | null | undefined, empty = "0 B"): string {
  if (!isNumber(bytes) || bytes <= 0) return empty;
  let value = bytes;
  let unit = 0;
  while (value >= 1024 && unit < BYTE_UNITS.length - 1) {
    value /= 1024;
    unit += 1;
  }
  const digits = unit === 0 || value >= 10 ? 0 : 1;
  return `${decimal(value, digits)} ${BYTE_UNITS[unit]}`;
}

/** Valor que já vem em MB do agente (memória, VRAM). */
export function formatMegabytes(mb: number | null | undefined, empty = "0 MB"): string {
  if (!isNumber(mb) || mb <= 0) return empty;
  return formatBytes(mb * 1024 * 1024, empty);
}

/** Valor que já vem em GB do agente (memória total, disco). */
export function formatGigabytes(gb: number | null | undefined, empty = "0 GB"): string {
  if (!isNumber(gb) || gb <= 0) return empty;
  return formatBytes(gb * 1024 * 1024 * 1024, empty);
}

/** Porcentagem inteira entre 0 e 100: "87%". */
export function formatPercent(value: number | null | undefined, empty = MISSING): string {
  if (!isNumber(value)) return empty;
  return `${Math.round(Math.max(0, Math.min(100, value)))}%`;
}

/** Variação com sinal e uma casa: "+54,0%", "-3,2%". */
export function formatSignedPercent(value: number | null | undefined, empty = MISSING): string {
  if (!isNumber(value)) return empty;
  const sign = value > 0 ? "+" : value < 0 ? "-" : "";
  return `${sign}${decimal(Math.abs(value), 1)}%`;
}

/** Latência: "4,2 ms" abaixo de 10, "38 ms" acima. Zero ou ausente = sem leitura. */
export function formatMs(value: number | null | undefined, empty = MISSING): string {
  if (!isNumber(value) || value <= 0) return empty;
  return `${formatNumber(value)} ms`;
}

/** Só o número da latência (o "ms" fica em outro elemento). */
export function formatMsValue(value: number | null | undefined, empty = MISSING): string {
  if (!isNumber(value)) return empty;
  return value < 10 ? decimal(value, 1) : decimal(value, 0);
}

/** Temperatura: "52 °C". Sem sensor = vazio (nunca 0 °C). */
export function formatTemperature(value: number | null | undefined, available = true, empty = MISSING): string {
  if (!available || !isNumber(value) || value <= 0) return empty;
  return `${Math.round(value)} °C`;
}

/** Frequência que vem em MHz: "4,90 GHz". */
export function formatGhz(mhz: number | null | undefined, empty = MISSING): string {
  if (!isNumber(mhz) || mhz <= 0) return empty;
  return `${decimal(mhz / 1000, 2)} GHz`;
}

/** Clock em MHz, atual e máximo: "2100 / 4900 MHz". */
export function formatMhzRange(current?: number | null, max?: number | null, empty = MISSING): string {
  if (!isNumber(current) && !isNumber(max)) return empty;
  if (isNumber(current) && isNumber(max)) return `${Math.round(current)} / ${Math.round(max)} MHz`;
  return `${Math.round((isNumber(current) ? current : max) ?? 0)} MHz`;
}

/** Nota de 0 a 100: "74/100". */
export function formatScore(value: number | null | undefined, empty = MISSING): string {
  if (!isNumber(value)) return empty;
  return `${Math.round(value)}/100`;
}

/** Minutos a partir de segundos: "60 min". */
export function formatMinutes(seconds: number | null | undefined): string {
  const minutes = isNumber(seconds) ? Math.max(0, Math.round(seconds / 60)) : 0;
  return `${minutes} min`;
}

/** Duração: "45 s", "12 min", "20 h 21 min", "3 d". */
export function formatDuration(seconds: number | null | undefined, empty = MISSING): string {
  if (!isNumber(seconds) || seconds < 0) return empty;
  if (seconds < 60) return `${Math.round(seconds)} s`;
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return `${minutes} min`;
  const hours = Math.floor(minutes / 60);
  if (hours < 48) return minutes % 60 ? `${hours} h ${minutes % 60} min` : `${hours} h`;
  return `${Math.floor(hours / 24)} d`;
}

/** Há quanto tempo, a partir de um Unix em segundos: "poucos segundos", "5 min", "3 h", "2 d". */
export function formatAgo(unixSeconds: number | null | undefined, empty = MISSING): string {
  if (!isNumber(unixSeconds) || unixSeconds <= 0) return empty;
  const delta = Math.max(0, Math.floor(Date.now() / 1000) - unixSeconds);
  if (delta < 60) return "poucos segundos";
  const minutes = Math.floor(delta / 60);
  if (minutes < 60) return `${minutes} min`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours} h`;
  return `${Math.floor(hours / 24)} d`;
}

/** Data e hora de um Unix em segundos, no idioma do app. */
export function formatDateTime(unixSeconds: number | null | undefined, empty = MISSING): string {
  if (!isNumber(unixSeconds) || unixSeconds <= 0) return empty;
  return new Date(unixSeconds * 1000).toLocaleString(currentLocale, { dateStyle: "short", timeStyle: "short" });
}

/** Só o número de uma métrica da telemetria, sem a unidade (o gráfico mostra a unidade ao lado). */
export function formatMetricNumber(value: number | null | undefined, unit: string, empty = MISSING): string {
  if (!isNumber(value)) return empty;
  if (unit === "ms") return formatMsValue(value, empty);
  return String(Math.round(value));
}

/** Strips vendor/marketing boilerplate off a raw CPU brand string so the
 * card shows "Ryzen 5 5600X" instead of "AMD Ryzen 5 5600X 6-Core
 * Processor" (or "i7-10700K" instead of "Intel(R) Core(TM) i7-10700K CPU
 * @ 3.80GHz") - a real user found the full string got cut off in the card. */
export function shortCpuName(raw: string | null | undefined, empty = MISSING) {
  if (!raw || !raw.trim()) return MISSING;
  return raw
    .trim()
    .replace(/^\d+(st|nd|rd|th)\s+Gen\s+/i, "")
    .replace(/^AMD\s+/i, "")
    .replace(/^Intel\(R\)\s*Core\(TM\)\s*/i, "")
    .replace(/^(Genuine\s+)?Intel\(R\)\s*/i, "")
    .replace(/\s*\d+-Core Processor\s*$/i, "")
    .replace(/\s*Processor\s*$/i, "")
    .replace(/\s*CPU\s*@\s*[\d.]+\s*GHz\s*$/i, "")
    .replace(/\s{2,}/g, " ")
    .trim() || raw.trim();
}

/** Same idea for GPU names - "RTX 3060" instead of "NVIDIA GeForce RTX 3060". */
export function shortGpuName(raw: string | null | undefined, empty = MISSING) {
  if (!raw || !raw.trim()) return MISSING;
  return raw
    .trim()
    .replace(/^NVIDIA\s+GeForce\s+/i, "")
    .replace(/^NVIDIA\s+/i, "")
    .replace(/^AMD\s+Radeon\s+/i, "")
    .replace(/^Radeon\s+/i, "")
    .replace(/\s+(Laptop|Notebook)\s+GPU$/i, "")
    .replace(/\s+GPU$/i, "")
    .replace(/\s{2,}/g, " ")
    .trim() || raw.trim();
}
