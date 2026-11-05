import { Loader2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { StatusPill } from "@/components/ui/status-pill";
import { useI18n } from "@/i18n";
import { formatMs } from "@/lib/format";
import { cn } from "@/lib/utils";
import type { TracerouteResult } from "@/services/tauri/agent";
import type { RouteVerdict } from "./diagnosis";

export function verdictTone(kind: RouteVerdict["kind"]): "ok" | "warn" | "muted" {
  if (kind === "normal") return "ok";
  if (kind === "inconclusive") return "muted";
  return "warn";
}

/** Diagnóstico de rota completo (traceroute), aberto dentro da aba pelo link do card "Caminho até a internet". */
export function RouteDetails({
  target,
  onTargetChange,
  result,
  verdict,
  busy,
  error,
  disabledReason,
  onRun,
  onClose,
}: {
  target: string;
  onTargetChange: (value: string) => void;
  result: TracerouteResult | null;
  verdict: RouteVerdict | null;
  busy: boolean;
  error: string | null;
  disabledReason: string | null;
  onRun: () => void;
  onClose: () => void;
}) {
  const { t } = useI18n();
  return (
    <Card
      id="network-route-details"
      label={t("mypc.network.route.title")}
      aside={
        <Button size="sm" variant="link" onClick={onClose}>
          {t("mypc.network.route.close")}
        </Button>
      }
    >
      <p className="text-[13px] text-fg-2">{t("mypc.network.route.desc")}</p>
      <p className="mt-0.5 text-xs text-fg-3">{t("mypc.network.route.subtitle")}</p>

      <form
        className="mt-4 flex flex-wrap items-center gap-2"
        onSubmit={(event) => {
          event.preventDefault();
          onRun();
        }}
      >
        <label className="sr-only" htmlFor="network-route-target">
          {t("mypc.network.route.target")}
        </label>
        <input
          id="network-route-target"
          value={target}
          onChange={(event) => onTargetChange(event.target.value)}
          placeholder="1.1.1.1"
          className="h-8 w-44 rounded-control border border-line bg-bg/40 px-3 text-[13px] text-fg outline-none transition-colors focus:border-accent/60"
        />
        <Button type="submit" size="sm" variant="outline" disabled={busy} disabledReason={disabledReason}>
          {busy ? t("mypc.network.route.tracing") : t("mypc.network.route.run")}
        </Button>
      </form>

      {busy && (
        <p className="mt-3 flex items-center gap-2 text-[13px] text-fg-3" aria-live="polite">
          <Loader2 className="h-4 w-4 animate-spin text-accent" aria-hidden />
          {t("mypc.network.route.tracingDesc")}
        </p>
      )}
      {error && <p className="mt-3 text-[13px] text-danger">{error}</p>}

      {verdict && (
        <div className="mt-4 rounded-control border border-line bg-surface-2 p-3">
          <div className="flex flex-wrap items-center gap-2">
            <span className="text-sm font-semibold text-fg">{t(`mypc.network.route.verdict.${verdict.kind}Title`)}</span>
            {verdict.culpritHop != null && (
              <StatusPill tone={verdictTone(verdict.kind)}>
                {t("mypc.network.route.culprit", { hop: verdict.culpritHop })}
                {verdict.culpritIp ? ` · ${verdict.culpritIp}` : ""}
              </StatusPill>
            )}
          </div>
          <p className="mt-1 text-[13px] leading-relaxed text-fg-2">{t(`mypc.network.route.verdict.${verdict.kind}Body`)}</p>
        </div>
      )}

      {result && (
        <ol className="mt-4 flex flex-col">
          {result.hops.map((hop) => {
            const culprit = verdict?.culpritHop === hop.hop;
            return (
              <li key={hop.hop} className="num flex items-center gap-3 border-b border-line-soft py-2 text-[13px] last:border-b-0">
                <span className="w-6 shrink-0 text-right font-mono text-fg-4">{hop.hop}</span>
                <span className={cn("min-w-0 flex-1 truncate font-mono", hop.timedOut ? "text-fg-4" : culprit ? "text-warn" : "text-fg")}>
                  {hop.timedOut ? t("mypc.network.route.hopTimedOut") : hop.ip}
                  {!hop.timedOut && hop.ip === result.target && (
                    <StatusPill tone="ok" className="ml-2">
                      {t("mypc.network.route.destination")}
                    </StatusPill>
                  )}
                </span>
                <span className="flex shrink-0 gap-2 font-mono text-fg-3">
                  {hop.rttMs.map((value, index) => (
                    <span key={index} className="w-16 text-right">
                      {value == null ? "*" : formatMs(value, t("mypc.network.lessThanOneMs"))}
                    </span>
                  ))}
                </span>
              </li>
            );
          })}
          {!result.reachedTarget && <li className="pt-2 text-[13px] text-warn">{t("mypc.network.route.incomplete")}</li>}
        </ol>
      )}
    </Card>
  );
}
