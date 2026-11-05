import { useEffect, useState, type ReactNode } from "react";
import { ExternalLink, TriangleAlert } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { ConfirmInline } from "@/components/ui/confirm-inline";
import { EmptyState } from "@/components/ui/empty-state";
import { Segmented } from "@/components/ui/segmented";
import { StatusPill } from "@/components/ui/status-pill";
import { Switch } from "@/components/ui/switch";
import { useTelemetry } from "@/hooks/useTelemetry";
import { useI18n } from "@/i18n";
import { formatMs } from "@/lib/format";
import {
  renewDhcpLease,
  type NetworkAdapterSummary,
  type NetworkDiagnostics,
  type NetworkTuneRequest,
  type NetworkTuneSession,
} from "@/services/tauri/agent";
import { identifyGateway } from "@/features/network/useLiveMode";

const BUFFERBLOAT_URL = "https://www.waveform.com/tools/bufferbloat";

type RunAction = (key: string, action: () => Promise<unknown>, done: string) => Promise<boolean>;

export type NetworkAdvancedProps = {
  diagnostics: NetworkDiagnostics | null;
  adapters: NetworkAdapterSummary[];
  allAdapters: NetworkAdapterSummary[];
  /** Ação em andamento na aba (props `busy` + releitura da rede + chave da ação). */
  locked: boolean;
  actionKey: string | null;
  /** Motivo para desabilitar o que pede o componente de sistema (useHelper). */
  helperReason: string | null;
  /** Motivo para desabilitar tudo o que fala com o agente. */
  agentReason: string | null;
  runAction: RunAction;
  onFlushDnsCache: () => Promise<unknown>;
  onResetWinsockCatalog: () => Promise<unknown>;
  adapterBusyName: string | null;
  onToggleAdapter: (adapterName: string, enabled: boolean) => void;
  dnsAdapterName: string;
  onDnsAdapterChange: (name: string) => void;
  onApplyDns: (adapterName: string, servers: string[]) => Promise<boolean>;
  tuneBusy: boolean;
  tuneError: string | null;
  tuneSession: NetworkTuneSession | null;
  onApplyTune: (request: NetworkTuneRequest) => Promise<void>;
};

/** Meu PC › Rede › Avançado (board Tela-MeuPC-Rede-Avancado): cards em 3 colunas, aviso único no topo. */
export function NetworkAdvanced(props: NetworkAdvancedProps) {
  const { t } = useI18n();
  return (
    <div className="flex flex-col gap-4">
      <p className="flex items-start gap-2 text-[13px] text-fg-2">
        <TriangleAlert className="mt-0.5 h-4 w-4 shrink-0 text-warn" aria-hidden />
        {t("mypc.network.advanced.warning")}
      </p>
      <div className="grid grid-cols-1 items-start gap-4 lg:grid-cols-3">
        <div className="flex flex-col gap-4">
          <AdaptersCard {...props} />
          <RouterCard {...props} />
        </div>
        <div className="flex flex-col gap-4">
          <RepairsCard {...props} />
          <ManualDnsCard {...props} />
        </div>
        <div className="flex flex-col gap-4">
          <TcpCard {...props} />
          <BufferbloatCard />
        </div>
      </div>
    </div>
  );
}

function adapterState(adapter: NetworkAdapterSummary): { enabled: boolean; labelKey: "up" | "down" | "disconnected" | null } {
  // "Up", "Disconnected", "Degraded"... = ligado no Windows; só "Disabled" é desligado.
  const raw = (adapter.status ?? "").toLowerCase();
  if (raw === "disabled") return { enabled: false, labelKey: "down" };
  if (raw === "up") return { enabled: true, labelKey: "up" };
  if (raw === "disconnected") return { enabled: true, labelKey: "disconnected" };
  return { enabled: true, labelKey: null };
}

function AdaptersCard({ diagnostics, allAdapters, locked, helperReason, agentReason, adapterBusyName, onToggleAdapter }: NetworkAdvancedProps) {
  const { t } = useI18n();
  return (
    <Card label={t("mypc.network.advanced.adapters.label")}>
      {allAdapters.length === 0 ? (
        <EmptyState variant="inline" title={t("mypc.network.advanced.adapters.none")} />
      ) : (
        <ul>
          {allAdapters.map((adapter) => {
            const state = adapterState(adapter);
            const inUse = adapter.name === diagnostics?.adapter_name;
            const statusText = state.labelKey ? t(`mypc.network.advanced.adapters.${state.labelKey}`) : adapter.status ?? "";
            const summary = [statusText, inUse ? t("mypc.network.advanced.adapters.inUse") : adapter.description]
              .filter(Boolean)
              .join(" · ");
            const busyHere = adapterBusyName === adapter.name;
            return (
              <li key={adapter.name} className="flex items-center gap-3 border-b border-line-soft py-3 first:pt-0 last:border-b-0">
                <div className="min-w-0 flex-1">
                  <div className="truncate text-[15px] font-semibold text-fg">{adapter.name}</div>
                  <div className="truncate text-[13px] text-fg-3" title={summary}>
                    {summary}
                  </div>
                </div>
                <Button
                  size="sm"
                  variant="outline"
                  disabled={locked || busyHere}
                  disabledReason={agentReason ?? helperReason}
                  onClick={() => onToggleAdapter(adapter.name, !state.enabled)}
                >
                  {busyHere
                    ? t("mypc.network.working")
                    : state.enabled
                      ? t("mypc.network.advanced.adapters.disable")
                      : t("mypc.network.advanced.adapters.enable")}
                </Button>
              </li>
            );
          })}
        </ul>
      )}
      <p className="mt-3 text-xs text-fg-3">{t("mypc.network.advanced.adapters.foot")}</p>
    </Card>
  );
}

function RouterCard({ diagnostics, agentReason }: NetworkAdvancedProps) {
  const { t } = useI18n();
  const track = useTelemetry("network");
  const [identity, setIdentity] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const gateway = diagnostics?.gateway ?? null;

  const identify = async () => {
    if (!gateway) return;
    setBusy(true);
    try {
      const found = await identifyGateway(gateway);
      setIdentity(found ?? t("mypc.network.advanced.router.unavailable"));
      track("gateway_identity_probed", { found: Boolean(found) });
    } catch {
      setIdentity(t("mypc.network.advanced.router.unavailable"));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Card label={t("mypc.network.advanced.router.label")}>
      {gateway ? (
        <>
          <div className="flex items-center gap-3">
            <div className="min-w-0 flex-1">
              <div className="num truncate text-[15px] font-semibold text-fg">{gateway}</div>
              {diagnostics?.gateway_latency_ms != null && (
                <div className="text-[13px] text-fg-3">
                  {t("mypc.network.advanced.router.latency", {
                    value: formatMs(diagnostics.gateway_latency_ms, t("mypc.network.lessThanOneMs")),
                  })}
                </div>
              )}
            </div>
            <Button size="sm" variant="outline" disabled={busy} disabledReason={agentReason} onClick={() => void identify()}>
              {busy ? t("mypc.network.advanced.router.identifying") : t("mypc.network.advanced.router.identify")}
            </Button>
          </div>
          {identity && <p className="mt-3 text-[13px] text-fg-2">{identity}</p>}
        </>
      ) : (
        <EmptyState variant="inline" title={t("mypc.network.advanced.router.none")} reason={t("mypc.network.advanced.router.noneReason")} />
      )}
      <p className="mt-3 text-xs text-fg-3">{t("mypc.network.advanced.router.foot")}</p>
    </Card>
  );
}

function RepairRow({
  title,
  subtitle,
  desc,
  note,
  action,
}: {
  title: string;
  subtitle?: string;
  desc: string;
  note?: string;
  action: ReactNode;
}) {
  return (
    <div className="flex items-center gap-3 border-b border-line-soft py-3 first:pt-0 last:border-b-0">
      <div className="min-w-0 flex-1">
        <div className="text-[15px] font-semibold text-fg">
          {title}
          {subtitle && <span className="ml-1.5 text-xs font-normal text-fg-3">{subtitle}</span>}
        </div>
        <div className="text-[13px] text-fg-3">
          {desc}
          {note && <span className="font-semibold text-warn"> · {note}</span>}
        </div>
      </div>
      {action}
    </div>
  );
}

function RepairsCard({ locked, actionKey, helperReason, agentReason, runAction, onFlushDnsCache, onResetWinsockCatalog }: NetworkAdvancedProps) {
  const { t } = useI18n();
  const [confirmWinsock, setConfirmWinsock] = useState(false);
  const label = (key: string, idle: string) => (actionKey === key ? t("mypc.network.working") : idle);
  return (
    <Card label={t("mypc.network.advanced.repairs.label")}>
      <RepairRow
        title={t("mypc.network.advanced.repairs.flushTitle")}
        desc={t("mypc.network.advanced.repairs.flushDesc")}
        action={
          <Button
            size="sm"
            variant="outline"
            disabled={locked}
            disabledReason={agentReason}
            onClick={() => void runAction("flush", onFlushDnsCache, t("mypc.network.advanced.repairs.flushDone"))}
          >
            {label("flush", t("mypc.network.advanced.repairs.flush"))}
          </Button>
        }
      />
      <RepairRow
        title={t("mypc.network.advanced.repairs.renewTitle")}
        subtitle={t("mypc.network.advanced.repairs.renewSub")}
        desc={t("mypc.network.advanced.repairs.renewDesc")}
        action={
          <Button
            size="sm"
            variant="outline"
            disabled={locked}
            disabledReason={agentReason}
            onClick={() => void runAction("renew", () => renewDhcpLease(), t("mypc.network.advanced.repairs.renewDone"))}
          >
            {label("renew", t("mypc.network.advanced.repairs.renew"))}
          </Button>
        }
      />
      <RepairRow
        title={t("mypc.network.advanced.repairs.winsockTitle")}
        subtitle="Winsock"
        desc={t("mypc.network.advanced.repairs.winsockDesc")}
        note={t("mypc.network.advanced.repairs.winsockReboot")}
        action={
          !confirmWinsock && (
            <Button
              size="sm"
              variant="danger"
              disabled={locked}
              disabledReason={agentReason ?? helperReason}
              onClick={() => setConfirmWinsock(true)}
            >
              {label("winsock", t("mypc.network.advanced.repairs.winsock"))}
            </Button>
          )
        }
      />
      {confirmWinsock && (
        <ConfirmInline
          className="mt-2"
          danger
          message={t("mypc.network.advanced.repairs.winsockConfirm")}
          confirmLabel={t("mypc.network.advanced.repairs.winsock")}
          busy={locked}
          onCancel={() => setConfirmWinsock(false)}
          onConfirm={() => {
            setConfirmWinsock(false);
            void runAction("winsock", onResetWinsockCatalog, t("mypc.network.advanced.repairs.winsockDone"));
          }}
        />
      )}
    </Card>
  );
}

function ManualDnsCard({ diagnostics, adapters, locked, helperReason, agentReason, dnsAdapterName, onDnsAdapterChange, onApplyDns }: NetworkAdvancedProps) {
  const { t } = useI18n();
  const [primary, setPrimary] = useState("");
  const [secondary, setSecondary] = useState("");
  const [missing, setMissing] = useState(false);
  const isMainAdapter = Boolean(dnsAdapterName) && dnsAdapterName === diagnostics?.adapter_name;
  const currentServers = diagnostics?.dns_servers ?? [];

  const apply = async () => {
    const servers = [primary, secondary].map((value) => value.trim()).filter(Boolean);
    if (!dnsAdapterName || servers.length === 0) {
      setMissing(true);
      return;
    }
    setMissing(false);
    if (await onApplyDns(dnsAdapterName, servers)) {
      setPrimary("");
      setSecondary("");
    }
  };

  const inputClass =
    "h-9 w-full rounded-control border border-line bg-bg/40 px-3 text-[13px] text-fg outline-none transition-colors placeholder:text-fg-4 focus:border-accent/60 disabled:opacity-50";

  return (
    <Card label={t("mypc.network.advanced.dnsManual.label")}>
      <div className="mb-1.5 text-xs text-fg-3">{t("mypc.network.advanced.dnsManual.adapter")}</div>
      {adapters.length === 0 ? (
        <EmptyState variant="inline" title={t("mypc.network.advanced.dnsManual.noAdapter")} />
      ) : (
        <Segmented
          aria-label={t("mypc.network.advanced.dnsManual.adapter")}
          size="sm"
          className="flex-wrap"
          options={adapters.map((adapter) => ({ value: adapter.name, label: adapter.name }))}
          value={dnsAdapterName}
          onChange={onDnsAdapterChange}
        />
      )}
      <form
        className="mt-3 flex flex-col gap-2"
        onSubmit={(event) => {
          event.preventDefault();
          void apply();
        }}
      >
        <input
          aria-label={t("mypc.network.advanced.dnsManual.primary")}
          value={primary}
          onChange={(event) => setPrimary(event.target.value)}
          placeholder={t("mypc.network.advanced.dnsManual.primary")}
          className={inputClass}
        />
        <input
          aria-label={t("mypc.network.advanced.dnsManual.secondary")}
          value={secondary}
          onChange={(event) => setSecondary(event.target.value)}
          placeholder={t("mypc.network.advanced.dnsManual.secondary")}
          className={inputClass}
        />
        <div className="mt-1 flex items-center gap-3">
          <span className="num min-w-0 flex-1 truncate text-xs text-fg-3">
            {isMainAdapter
              ? currentServers.length
                ? t("mypc.network.advanced.dnsManual.current", { servers: currentServers.join(", ") })
                : t("mypc.network.advanced.dnsManual.currentProvider")
              : null}
          </span>
          <Button
            type="submit"
            size="sm"
            variant="outline"
            disabled={locked || !dnsAdapterName}
            disabledReason={agentReason ?? helperReason}
          >
            {t("mypc.network.advanced.dnsManual.apply")}
          </Button>
        </div>
      </form>
      {missing && <p className="mt-2 text-[13px] text-warn">{t("mypc.network.advanced.dnsManual.missing")}</p>}
      <p className="mt-3 text-xs text-fg-3">{t("mypc.network.advanced.dnsManual.foot")}</p>
    </Card>
  );
}

type AutoTuning = "Normal" | "Disabled";
type Ecn = "Enabled" | "Disabled";

function TcpCard({ diagnostics, adapters, helperReason, agentReason, tuneBusy, tuneError, tuneSession, onApplyTune }: NetworkAdvancedProps) {
  const { t } = useI18n();
  // O agente não informa o valor atual: guardamos só o que a pessoa mudou e
  // enviamos só isso (o resto do TCP fica como está).
  const [autoTuning, setAutoTuning] = useState<AutoTuning | null>(null);
  const [ecn, setEcn] = useState<Ecn | null>(null);
  const [nagle, setNagle] = useState(false);
  const [throttling, setThrottling] = useState(false);
  const [nagleAdapter, setNagleAdapter] = useState("");

  useEffect(() => {
    setNagleAdapter((current) => {
      if (current && adapters.some((adapter) => adapter.name === current)) return current;
      return diagnostics?.adapter_name ?? adapters[0]?.name ?? "";
    });
  }, [adapters, diagnostics?.adapter_name]);

  const blocked = agentReason ?? helperReason;
  const sessionOpen = Boolean(tuneSession);

  const applyLive = async () => {
    await onApplyTune({ autoTuningLevel: autoTuning, ecnCapability: ecn });
  };
  const applyReboot = async () => {
    await onApplyTune({
      adapterName: nagle ? nagleAdapter : null,
      nagleDisabled: nagle ? true : null,
      throttlingDisabled: throttling ? true : null,
    });
  };

  return (
    <Card label={t("mypc.network.advanced.tcp.label")}>
      <div className="text-xs font-semibold text-fg-2">{t("mypc.network.advanced.tcp.liveTitle")}</div>
      <div className="mt-2 flex items-center gap-3 border-b border-line-soft py-2.5">
        <div className="min-w-0 flex-1">
          <div className="text-[15px] font-semibold text-fg">{t("mypc.network.advanced.tcp.autoTuning")}</div>
          <div className="text-xs text-fg-3">{t("mypc.network.advanced.tcp.autoTuningSub")}</div>
        </div>
        <Segmented<AutoTuning | "">
          aria-label={t("mypc.network.advanced.tcp.autoTuning")}
          size="sm"
          options={[
            { value: "Normal", label: t("mypc.network.advanced.tcp.normal"), disabled: Boolean(blocked) || tuneBusy },
            { value: "Disabled", label: t("mypc.network.advanced.tcp.off"), disabled: Boolean(blocked) || tuneBusy },
          ]}
          value={autoTuning ?? ""}
          onChange={(value) => setAutoTuning(value || null)}
        />
      </div>
      <div className="flex items-center gap-3 border-b border-line-soft py-2.5">
        <div className="min-w-0 flex-1">
          <div className="text-[15px] font-semibold text-fg">{t("mypc.network.advanced.tcp.ecn")}</div>
          <div className="text-xs text-fg-3">{t("mypc.network.advanced.tcp.ecnSub")}</div>
        </div>
        <Segmented<Ecn | "">
          aria-label={t("mypc.network.advanced.tcp.ecn")}
          size="sm"
          options={[
            { value: "Enabled", label: t("mypc.network.advanced.tcp.on"), disabled: Boolean(blocked) || tuneBusy },
            { value: "Disabled", label: t("mypc.network.advanced.tcp.off"), disabled: Boolean(blocked) || tuneBusy },
          ]}
          value={ecn ?? ""}
          onChange={(value) => setEcn(value || null)}
        />
      </div>
      <div className="mt-2.5 flex items-center gap-3">
        <span className="min-w-0 flex-1 text-xs text-fg-3">{t("mypc.network.advanced.tcp.applyLiveHint")}</span>
        <Button
          size="sm"
          variant="outline"
          disabled={tuneBusy || sessionOpen || (!autoTuning && !ecn)}
          disabledReason={blocked}
          onClick={() => void applyLive()}
        >
          {t("mypc.network.advanced.tcp.applyLive")}
        </Button>
      </div>

      <div className="mt-5 text-xs font-semibold text-fg-2">{t("mypc.network.advanced.tcp.rebootTitle")}</div>
      <label className="mt-2 flex items-center gap-3 border-b border-line-soft py-2.5">
        <span className="min-w-0 flex-1">
          <span className="block text-[15px] font-semibold text-fg">{t("mypc.network.advanced.tcp.nagle")}</span>
          <span className="block text-xs text-fg-3">{t("mypc.network.advanced.tcp.nagleSub")}</span>
        </span>
        <Switch checked={nagle} onCheckedChange={setNagle} disabled={Boolean(blocked) || tuneBusy} />
      </label>
      {nagle && adapters.length > 1 && (
        <div className="border-b border-line-soft py-2.5">
          <div className="mb-1.5 text-xs text-fg-3">{t("mypc.network.advanced.tcp.nagleAdapter")}</div>
          <Segmented
            aria-label={t("mypc.network.advanced.tcp.nagleAdapter")}
            size="sm"
            className="flex-wrap"
            options={adapters.map((adapter) => ({ value: adapter.name, label: adapter.name }))}
            value={nagleAdapter}
            onChange={setNagleAdapter}
          />
        </div>
      )}
      <label className="flex items-center gap-3 py-2.5">
        <span className="min-w-0 flex-1">
          <span className="block text-[15px] font-semibold text-fg">{t("mypc.network.advanced.tcp.throttling")}</span>
          <span className="block text-xs text-fg-3">{t("mypc.network.advanced.tcp.throttlingSub")}</span>
        </span>
        <Switch checked={throttling} onCheckedChange={setThrottling} disabled={Boolean(blocked) || tuneBusy} />
      </label>
      <div className="mt-1 flex items-center justify-end gap-2">
        <StatusPill tone="warn">{t("mypc.network.advanced.repairs.winsockReboot")}</StatusPill>
        <Button
          size="sm"
          variant="outline"
          disabled={tuneBusy || sessionOpen || (!nagle && !throttling) || (nagle && !nagleAdapter)}
          disabledReason={blocked}
          onClick={() => void applyReboot()}
        >
          {t("mypc.network.advanced.tcp.applyReboot")}
        </Button>
      </div>

      {tuneError && <p className="mt-3 text-[13px] text-danger">{tuneError}</p>}
      <p className="mt-3 text-xs text-fg-3">{t("mypc.network.advanced.tcp.foot")}</p>
    </Card>
  );
}

function BufferbloatCard() {
  const { t } = useI18n();
  return (
    <Card label={t("mypc.network.advanced.bufferbloat.label")}>
      <div className="flex items-center gap-3">
        <div className="min-w-0 flex-1">
          <div className="text-[15px] font-semibold text-fg">{t("mypc.network.advanced.bufferbloat.title")}</div>
          <div className="text-[13px] text-fg-3">{t("mypc.network.advanced.bufferbloat.desc")}</div>
        </div>
        <Button
          size="sm"
          variant="link"
          onClick={() => window.open(BUFFERBLOAT_URL, "_blank", "noopener,noreferrer")}
        >
          {t("mypc.network.advanced.bufferbloat.open")}
          <ExternalLink aria-hidden />
        </Button>
      </div>
    </Card>
  );
}
