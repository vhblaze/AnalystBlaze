import { useCallback, useEffect, useState } from "react";
import { AlertTriangle, FileWarning, Video } from "lucide-react";
import { Button } from "@/components/ui/button";
import { StatusPill } from "@/components/ui/status-pill";
import { useTelemetry } from "@/hooks/useTelemetry";
import { useI18n } from "@/i18n";
import { formatMs, formatNumber, formatPercent } from "@/lib/format";
import {
  detectLiveModeStreamingApp,
  generateLiveModeIncidentReport,
  listenToLiveModeIncident,
  listenToLiveModeSample,
  startLiveMode,
  stopLiveMode,
  type BitrateRecommendation,
  type IncidentReport,
  type LiveModeSample,
} from "@/services/tauri/agent";

import { liveModeStatus } from "@/features/network/useLiveMode";

const LIVE_MODE_SAMPLE_HISTORY = 60;
const STREAMING_APP_POLL_MS = 15_000;

/**
 * Linha "Modo ao vivo": liga/desliga a amostragem do agente (a cada 2 s) e,
 * ligada, mostra a última amostra, o bitrate sugerido e o relatório de
 * incidente. Tudo o que o Modo Live da view antiga fazia.
 */
export function LiveMode({ isReady }: { isReady: boolean }) {
  const { t } = useI18n();
  const track = useTelemetry("network");
  const [active, setActive] = useState(false);
  const [busy, setBusy] = useState(false);
  const [samples, setSamples] = useState<LiveModeSample[]>([]);
  const [streamingApp, setStreamingApp] = useState<string | null>(null);
  const [bitrate, setBitrate] = useState<BitrateRecommendation | null>(null);
  const [incident, setIncident] = useState<IncidentReport | null>(null);
  const [reportBusy, setReportBusy] = useState(false);
  const latest = samples[samples.length - 1] ?? null;

  useEffect(() => {
    if (!isReady) return;
    let canceled = false;
    const poll = () => {
      detectLiveModeStreamingApp()
        .then((app) => {
          if (!canceled) setStreamingApp(app);
        })
        .catch(() => undefined);
    };
    poll();
    const id = window.setInterval(poll, STREAMING_APP_POLL_MS);
    return () => {
      canceled = true;
      window.clearInterval(id);
    };
  }, [isReady]);

  useEffect(() => {
    if (!isReady) return;
    liveModeStatus
      .refresh()
      .then((status) => {
        setActive(status.active);
        setSamples(status.samples.slice(-LIVE_MODE_SAMPLE_HISTORY));
        setBitrate(status.bitrateRecommendation ?? null);
        setIncident(status.lastIncident ?? null);
      })
      .catch(() => undefined);
  }, [isReady]);

  useEffect(() => {
    let disposeSample: (() => void) | undefined;
    let disposeIncident: (() => void) | undefined;
    let disposed = false;
    listenToLiveModeSample((sample) => {
      setSamples((current) => [...current.slice(-(LIVE_MODE_SAMPLE_HISTORY - 1)), sample]);
    }).then((dispose) => {
      if (disposed) dispose();
      else disposeSample = dispose;
    });
    listenToLiveModeIncident((report) => setIncident(report)).then((dispose) => {
      if (disposed) dispose();
      else disposeIncident = dispose;
    });
    return () => {
      disposed = true;
      disposeSample?.();
      disposeIncident?.();
    };
  }, []);

  useEffect(() => {
    if (!active) return;
    const id = window.setInterval(() => {
      liveModeStatus
        .refresh()
        .then((status) => {
          setBitrate(status.bitrateRecommendation ?? null);
          setIncident(status.lastIncident ?? null);
        })
        .catch(() => undefined);
    }, 5_000);
    return () => window.clearInterval(id);
  }, [active]);

  const toggle = useCallback(async () => {
    if (!isReady || busy) return;
    setBusy(true);
    try {
      if (active) {
        await stopLiveMode();
        setActive(false);
      } else {
        setSamples([]);
        await startLiveMode();
        setActive(true);
      }
      track("live_mode_toggled", { enabled: !active });
    } catch {
      track("live_mode_toggle_failed");
    } finally {
      setBusy(false);
    }
  }, [active, busy, isReady, track]);

  const generateReport = useCallback(async () => {
    if (reportBusy) return;
    setReportBusy(true);
    try {
      setIncident(await generateLiveModeIncidentReport());
      track("live_mode_incident_report_generated");
    } catch {
      track("live_mode_incident_report_failed");
    } finally {
      setReportBusy(false);
    }
  }, [reportBusy, track]);

  const metric = (value: number | null | undefined, kind: "ms" | "%") => {
    if (value == null || !Number.isFinite(value)) return t("mypc.network.band.noReading");
    if (kind === "%") return formatPercent(value);
    return value > 0 && value < 1 ? t("mypc.network.lessThanOneMs") : formatMs(value, t("mypc.network.lessThanOneMs"));
  };

  return (
    <section className="rounded-card border border-line bg-surface">
      <div className="flex min-h-12 flex-wrap items-center gap-x-3 gap-y-1 px-5 py-3">
        <span className="font-semibold text-fg">{t("mypc.network.live.title")}</span>
        <span className="min-w-0 flex-1 text-[13px] text-fg-3">{t("mypc.network.live.desc")}</span>
        <Button
          size="sm"
          variant="outline"
          role="switch"
          aria-checked={active}
          disabled={busy}
          disabledReason={isReady ? null : t("mypc.network.needsAgent")}
          onClick={() => void toggle()}
        >
          {busy ? t("mypc.network.working") : active ? t("mypc.network.live.off") : t("mypc.network.live.on")}
        </Button>
      </div>

      {streamingApp && !active && (
        <p className="flex items-start gap-2 border-t border-line-soft px-5 py-3 text-[13px] text-fg-2">
          <Video className="mt-0.5 h-4 w-4 shrink-0 text-accent" aria-hidden />
          {t("mypc.network.live.streamingDetected", { app: streamingApp })}
        </p>
      )}

      {active && (
        <div className="flex flex-col gap-4 border-t border-line px-5 py-4">
          <dl className="num grid grid-cols-3 gap-4">
            {(
              [
                ["ping", metric(latest?.pingMs, "ms")],
                ["jitter", metric(latest?.jitterMs, "ms")],
                ["loss", metric(latest?.packetLossPercent, "%")],
              ] as const
            ).map(([key, value]) => (
              <div key={key}>
                <dt className="label-mono">{t(`mypc.network.live.${key}`)}</dt>
                <dd className="mt-1 text-lg font-semibold text-fg">{latest ? value : t("mypc.network.live.waiting")}</dd>
              </div>
            ))}
          </dl>

          {bitrate && (
            <div className="border-t border-line-soft pt-3">
              <div className="label-mono">{t("mypc.network.live.bitrate")}</div>
              <div className="num mt-1 text-xl font-semibold text-fg">{formatNumber(bitrate.recommendedKbps)} kbps</div>
              <p className="mt-0.5 text-xs text-fg-3">
                {t("mypc.network.live.bitrateHint", { confidence: formatPercent(bitrate.confidence * 100) })}
              </p>
              {bitrate.reason && <p className="mt-1 text-[13px] text-fg-2">{bitrate.reason}</p>}
            </div>
          )}

          <div className="flex flex-wrap items-center justify-between gap-2 border-t border-line-soft pt-3">
            <span className="text-xs text-fg-3">{t("mypc.network.live.samples", { count: samples.length })}</span>
            <Button size="sm" variant="outline" disabled={reportBusy} onClick={() => void generateReport()}>
              <FileWarning aria-hidden />
              {reportBusy ? t("mypc.network.live.reportBusy") : t("mypc.network.live.report")}
            </Button>
          </div>

          {incident && (
            <div className="rounded-control border border-warn/40 bg-warn/5 p-4">
              <div className="label-mono flex items-center gap-2 text-warn">
                <AlertTriangle className="h-3.5 w-3.5" aria-hidden />
                {t("mypc.network.live.causes", { count: incident.sampleCount })}
              </div>
              <ul className="mt-2 flex flex-col gap-2">
                {incident.causes.map((cause) => (
                  <li key={cause.label} className="text-[13px] leading-relaxed text-fg-2">
                    <span className="font-semibold text-fg">{cause.label}</span>{" "}
                    <StatusPill tone="warn">{formatPercent(cause.confidence * 100)}</StatusPill>
                    <span className="block text-fg-3">{cause.evidence}</span>
                  </li>
                ))}
              </ul>
            </div>
          )}
        </div>
      )}
    </section>
  );
}
