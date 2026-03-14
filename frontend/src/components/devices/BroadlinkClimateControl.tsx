import type { ReactNode } from "react";
import { useEffect, useMemo, useState } from "react";
import { useMutation, useQuery } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { Loader2, RefreshCw, WifiOff } from "lucide-react";
import { broadlinkApi } from "@/lib/api";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Skeleton } from "@/components/ui/skeleton";
import { Switch } from "@/components/ui/switch";
import { toast } from "@/hooks/use-toast";

interface BroadlinkClimateControlProps {
  defaultModel?: string;
  compact?: boolean;
  showRefresh?: boolean;
}

type ClimateMode = "cool" | "heat" | "dry" | "fan" | "auto";
type ClimateFan = "auto" | "1" | "2" | "3" | "4" | "silent";
type ClimateVane = "auto" | "highest" | "high" | "middle" | "low" | "lowest" | "swing";
type ClimateTimerMode = "none" | "stop";

interface StructuredState {
  power: boolean;
  mode: ClimateMode;
  temperature: number;
  fan: ClimateFan;
  vane: ClimateVane;
  econo: boolean;
  timerMode: ClimateTimerMode;
  stopHour: string;
  stopMinute: string;
}

const DISCOVERY_TIMEOUT_MS = 120_000;
const INITIAL_STATE: StructuredState = {
  power: true,
  mode: "cool",
  temperature: 20,
  fan: "auto",
  vane: "auto",
  econo: false,
  timerMode: "none",
  stopHour: "11",
  stopMinute: "00",
};

export function BroadlinkClimateControl({
  defaultModel = "msz-hj5va",
  compact = false,
  showRefresh = true,
}: BroadlinkClimateControlProps) {
  const { t } = useTranslation();
  const [discoveryTimedOut, setDiscoveryTimedOut] = useState(false);
  const [forceRefreshToken, setForceRefreshToken] = useState(0);
  const [structuredState, setStructuredState] = useState<StructuredState>(INITIAL_STATE);

  const discoverQuery = useQuery({
    queryKey: ["broadlink", "discover", "single-remote", forceRefreshToken],
    queryFn: () => broadlinkApi.discover(undefined, forceRefreshToken > 0),
    retry: true,
    retryDelay: 4000,
    refetchInterval: (query) => {
      const hasRemote = (query.state.data?.devices?.length ?? 0) > 0;
      return hasRemote || discoveryTimedOut ? false : 4000;
    },
    refetchOnWindowFocus: false,
    staleTime: Infinity,
  });

  const remote = discoverQuery.data?.devices?.[0];
  const structuredCommand = useMemo(() => buildStructuredCommand(structuredState), [structuredState]);

  useEffect(() => {
    if (remote || discoveryTimedOut) {
      return;
    }

    const timeout = window.setTimeout(() => {
      setDiscoveryTimedOut(true);
    }, DISCOVERY_TIMEOUT_MS);

    return () => window.clearTimeout(timeout);
  }, [remote, discoveryTimedOut]);

  useEffect(() => {
    if (remote?.host) {
      setDiscoveryTimedOut(false);
    }
  }, [remote]);

  const sendMutation = useMutation({
    mutationFn: (command: string) => broadlinkApi.sendMitsubishiCommand(remote?.host ?? "", command, defaultModel),
    onSuccess: (_, command) => {
      toast({
        title: t("climate.commandSent"),
        description: t("climate.commandSentDescription", {
          command,
          host: remote?.host ?? "RM4 Pro",
        }),
      });
    },
    onError: (error) => {
      toast({
        title: t("common.error"),
        description: error instanceof Error ? error.message : t("climate.commandFailed"),
        variant: "destructive",
      });
    },
  });

  return (
    <Card className="border-0 bg-transparent shadow-none">
      {showRefresh && (
        <CardHeader className={compact ? "pb-3" : "pb-4"}>
          <div className="flex items-center justify-end gap-4">
            <Button
              variant="ghost"
              size="icon"
              className="rounded-full text-slate-500 hover:bg-slate-100 hover:text-slate-900"
              onClick={() => {
                setDiscoveryTimedOut(false);
                setForceRefreshToken((value) => value + 1);
                discoverQuery.refetch();
              }}
              disabled={discoverQuery.isFetching || sendMutation.isPending}
            >
              {discoverQuery.isFetching ? <Loader2 className="h-4 w-4 animate-spin" /> : <RefreshCw className="h-4 w-4" />}
            </Button>
          </div>
        </CardHeader>
      )}

      <CardContent className="space-y-4 px-0 pb-0">
        {remote ? (
          <div className="rounded-2xl bg-emerald-50/90 px-4 py-3 text-sm text-emerald-800">
            <div className="font-medium">{t("climate.remoteConnected")}</div>
            <div className="mt-1 font-mono text-xs text-emerald-700">{remote.host}</div>
          </div>
        ) : discoveryTimedOut ? (
          <div className="rounded-2xl bg-slate-50 px-4 py-5 text-center text-sm text-slate-500">
            <WifiOff className="mx-auto mb-3 h-5 w-5 text-slate-400" />
            <div className="font-medium text-slate-700">{t("climate.noRemoteTitle")}</div>
            <div className="mt-1">{t("climate.noRemoteDescription")}</div>
          </div>
        ) : (
          <div className="rounded-2xl bg-slate-50/90 px-4 py-5">
            <div className="mb-4 flex items-center gap-3">
              <div className="flex h-10 w-10 items-center justify-center rounded-2xl bg-sky-100 text-sky-600">
                <Loader2 className="h-4 w-4 animate-spin" />
              </div>
              <div>
                <div className="font-medium text-slate-800">{t("climate.searchingTitle")}</div>
                <div className="text-sm text-slate-500">{t("climate.searchingDescription")}</div>
              </div>
            </div>
            <div className="grid gap-3 sm:grid-cols-2 lg:grid-cols-3">
              <Skeleton className="h-14 rounded-2xl bg-white" />
              <Skeleton className="h-14 rounded-2xl bg-white" />
              <Skeleton className="h-14 rounded-2xl bg-white" />
            </div>
          </div>
        )}

        {remote && (
          <div className="space-y-4">
            <div className="rounded-3xl border border-slate-200 bg-white p-4 shadow-sm">
              <div className="flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
                <div>
                  <div className="text-sm font-semibold text-slate-900">Structured Mitsubishi test</div>
                  <div className="mt-1 text-sm text-slate-500">Build a full `state-*` command and send it directly to the RM4 Pro.</div>
                </div>
                <Button variant="outline" className="rounded-2xl" onClick={() => setStructuredState(INITIAL_STATE)} disabled={sendMutation.isPending}>
                  Reset
                </Button>
              </div>

              <div className="mt-4 grid gap-3 md:grid-cols-2 xl:grid-cols-4">
                <ControlBlock label="Power">
                  <div className="flex gap-2">
                    <Button
                      type="button"
                      variant={structuredState.power ? "default" : "outline"}
                      className="flex-1 rounded-2xl"
                      onClick={() => setStructuredState((current) => ({ ...current, power: true }))}
                      disabled={sendMutation.isPending}
                    >
                      On
                    </Button>
                    <Button
                      type="button"
                      variant={!structuredState.power ? "default" : "outline"}
                      className="flex-1 rounded-2xl"
                      onClick={() => setStructuredState((current) => ({ ...current, power: false }))}
                      disabled={sendMutation.isPending}
                    >
                      Off
                    </Button>
                  </div>
                </ControlBlock>

                <ControlBlock label="Mode">
                  <Select
                    value={structuredState.mode}
                    onValueChange={(value: ClimateMode) =>
                      setStructuredState((current) => ({
                        ...current,
                        mode: value,
                        econo: value === "cool" ? current.econo : false,
                      }))
                    }
                  >
                    <SelectTrigger className="rounded-2xl">
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      <SelectItem value="cool">Cool</SelectItem>
                      <SelectItem value="heat">Heat</SelectItem>
                      <SelectItem value="dry">Dry</SelectItem>
                      <SelectItem value="fan">Fan</SelectItem>
                      <SelectItem value="auto">Auto</SelectItem>
                    </SelectContent>
                  </Select>
                </ControlBlock>

                <ControlBlock label="Temperature">
                  <Input
                    type="number"
                    min={16}
                    max={31}
                    value={structuredState.temperature}
                    className="rounded-2xl"
                    onChange={(event) => {
                      const value = Number(event.target.value);
                      setStructuredState((current) => ({
                        ...current,
                        temperature: Number.isFinite(value) ? Math.min(31, Math.max(16, value)) : current.temperature,
                      }));
                    }}
                  />
                </ControlBlock>

                <ControlBlock label="Fan">
                  <Select
                    value={structuredState.fan}
                    onValueChange={(value: ClimateFan) => setStructuredState((current) => ({ ...current, fan: value }))}
                  >
                    <SelectTrigger className="rounded-2xl">
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      <SelectItem value="auto">Auto</SelectItem>
                      <SelectItem value="1">Level 1</SelectItem>
                      <SelectItem value="2">Level 2</SelectItem>
                      <SelectItem value="3">Level 3</SelectItem>
                      <SelectItem value="4">Level 4</SelectItem>
                      <SelectItem value="silent">Silent</SelectItem>
                    </SelectContent>
                  </Select>
                </ControlBlock>

                <ControlBlock label="Vertical Vane">
                  <Select
                    value={structuredState.vane}
                    onValueChange={(value: ClimateVane) => setStructuredState((current) => ({ ...current, vane: value }))}
                  >
                    <SelectTrigger className="rounded-2xl">
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      <SelectItem value="auto">Auto</SelectItem>
                      <SelectItem value="highest">Highest</SelectItem>
                      <SelectItem value="high">High</SelectItem>
                      <SelectItem value="middle">Middle</SelectItem>
                      <SelectItem value="low">Low</SelectItem>
                      <SelectItem value="lowest">Lowest</SelectItem>
                      <SelectItem value="swing">Swing</SelectItem>
                    </SelectContent>
                  </Select>
                </ControlBlock>

                <ControlBlock label="Timer Mode">
                  <Select
                    value={structuredState.timerMode}
                    onValueChange={(value: ClimateTimerMode) => setStructuredState((current) => ({ ...current, timerMode: value }))}
                  >
                    <SelectTrigger className="rounded-2xl">
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      <SelectItem value="none">None</SelectItem>
                      <SelectItem value="stop">Stop only</SelectItem>
                    </SelectContent>
                  </Select>
                </ControlBlock>

                <ControlBlock label="Stop Time">
                  <div className="grid grid-cols-2 gap-2">
                    <Input value={structuredState.stopHour} className="rounded-2xl" onChange={(event) => setStructuredState((current) => ({ ...current, stopHour: sanitizeTimePart(event.target.value, 23) }))} />
                    <Input value={structuredState.stopMinute} className="rounded-2xl" onChange={(event) => setStructuredState((current) => ({ ...current, stopMinute: sanitizeMinutePart(event.target.value) }))} />
                  </div>
                </ControlBlock>
              </div>

              <div className="mt-4 grid gap-3 md:grid-cols-2">
                <ToggleCard
                  title="Econo cool"
                  description="Only meaningful in cool mode."
                  checked={structuredState.econo}
                  disabled={structuredState.mode !== "cool"}
                  onCheckedChange={(checked) => setStructuredState((current) => ({ ...current, econo: checked }))}
                />
              </div>

              <div className="mt-4 rounded-2xl bg-slate-50 px-4 py-3">
                <div className="text-xs font-medium uppercase tracking-wide text-slate-500">Generated command</div>
                <div className="mt-1 break-all font-mono text-sm text-slate-800">{structuredCommand}</div>
              </div>

              <div className="mt-4 flex flex-wrap gap-3">
                <Button
                  className="rounded-2xl"
                  disabled={sendMutation.isPending}
                  onClick={() => sendMutation.mutate(structuredCommand)}
                >
                  {sendMutation.isPending && sendMutation.variables === structuredCommand ? <Loader2 className="mr-2 h-4 w-4 animate-spin" /> : null}
                  Send structured command
                </Button>
                <Button
                  variant="outline"
                  className="rounded-2xl"
                  disabled={sendMutation.isPending}
                  onClick={() => sendMutation.mutate("state-off")}
                >
                  Send off
                </Button>
              </div>
            </div>

          </div>
        )}
      </CardContent>
    </Card>
  );
}

function ControlBlock({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="space-y-2">
      <div className="text-xs font-medium uppercase tracking-wide text-slate-500">{label}</div>
      {children}
    </div>
  );
}

function ToggleCard({
  title,
  description,
  checked,
  disabled,
  onCheckedChange,
}: {
  title: string;
  description: string;
  checked: boolean;
  disabled?: boolean;
  onCheckedChange: (checked: boolean) => void;
}) {
  return (
    <div className="flex items-center justify-between rounded-2xl border border-slate-200 px-4 py-3">
      <div>
        <div className="text-sm font-medium text-slate-900">{title}</div>
        <div className="text-xs text-slate-500">{description}</div>
      </div>
      <Switch checked={checked} disabled={disabled} onCheckedChange={onCheckedChange} />
    </div>
  );
}

function buildStructuredCommand(state: StructuredState) {
  if (!state.power) {
    return "state-off";
  }

  const parts = ["state", state.mode, String(state.temperature), "fan", state.fan, "vane", state.vane, "wide", "center"];

  if (state.econo) {
    parts.push("econo", "on");
  }

  if (state.timerMode === "stop") {
    parts.push("stop", state.stopHour.padStart(2, "0"), state.stopMinute.padStart(2, "0"));
  }

  return parts.join("-");
}

function sanitizeTimePart(value: string, max: number) {
  const digits = value.replace(/\D/g, "").slice(0, 2);
  if (!digits) return "";
  return String(Math.min(max, Number(digits))).padStart(2, "0");
}

function sanitizeMinutePart(value: string) {
  const digits = value.replace(/\D/g, "").slice(0, 2);
  if (!digits) return "";
  const numeric = Math.min(59, Number(digits));
  return String(numeric - (numeric % 10)).padStart(2, "0");
}
