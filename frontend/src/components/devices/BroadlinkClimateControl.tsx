import { useEffect, useMemo, useState } from "react";
import { useMutation, useQuery } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { AirVent, Fan, Loader2, Power, RefreshCw, Snowflake, Waves, WifiOff } from "lucide-react";
import { broadlinkApi, type BroadlinkCode } from "@/lib/api";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader } from "@/components/ui/card";
import { Skeleton } from "@/components/ui/skeleton";
import { toast } from "@/hooks/use-toast";

interface BroadlinkClimateControlProps {
  defaultModel?: string;
  compact?: boolean;
  showRefresh?: boolean;
}

const COMMAND_ORDER = ["off", "on", "mode", "fan", "vane", "econo-cool", "too-cool", "too-warm", "timer", "time", "time-up", "time-down"];
const DISCOVERY_TIMEOUT_MS = 120_000;

export function BroadlinkClimateControl({
  defaultModel = "msz-hj5va",
  compact = false,
  showRefresh = true,
}: BroadlinkClimateControlProps) {
  const { t } = useTranslation();
  const [discoveryTimedOut, setDiscoveryTimedOut] = useState(false);
  const [forceRefreshToken, setForceRefreshToken] = useState(0);

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

  const codesQuery = useQuery({
    queryKey: ["broadlink", "mitsubishi-codes", defaultModel],
    queryFn: () => broadlinkApi.listMitsubishiCodes(defaultModel),
    refetchOnWindowFocus: false,
    staleTime: 60_000,
  });

  const remote = discoverQuery.data?.devices?.[0];
  const commands = useMemo(() => sortCommands(codesQuery.data?.codes ?? []), [codesQuery.data?.codes]);

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
                codesQuery.refetch();
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
          codesQuery.isLoading ? (
            <div className="grid gap-3 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4">
              <Skeleton className="h-16 rounded-2xl" />
              <Skeleton className="h-16 rounded-2xl" />
              <Skeleton className="h-16 rounded-2xl" />
              <Skeleton className="h-16 rounded-2xl" />
            </div>
          ) : commands.length > 0 ? (
            <div className="grid gap-3 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4">
              {commands.map((code) => {
                const sending = sendMutation.isPending && sendMutation.variables === code.command;
                return (
                  <button
                    key={code.id}
                    type="button"
                    className="group flex min-h-16 items-center gap-3 rounded-2xl border border-slate-200 bg-white px-4 py-3 text-left shadow-sm transition-all hover:-translate-y-0.5 hover:border-slate-300 hover:shadow-md disabled:cursor-not-allowed disabled:opacity-60"
                    disabled={sending || sendMutation.isPending}
                    onClick={() => sendMutation.mutate(code.command)}
                  >
                    <div className="flex h-10 w-10 shrink-0 items-center justify-center rounded-xl bg-slate-100 text-slate-700 transition-colors group-hover:bg-sky-100 group-hover:text-sky-700">
                      {sending ? <Loader2 className="h-4 w-4 animate-spin" /> : iconForCommand(code.command)}
                    </div>
                    <div className="min-w-0">
                      <div className="truncate text-sm font-medium text-slate-900">{code.name}</div>
                      <div className="truncate text-xs text-slate-500">{prettyCommand(code.command)}</div>
                    </div>
                  </button>
                );
              })}
            </div>
          ) : (
            <div className="rounded-2xl bg-slate-50 px-4 py-5 text-center text-sm text-slate-500">
              {t("climate.noCommands")}
            </div>
          )
        )}
      </CardContent>
    </Card>
  );
}

function sortCommands(codes: BroadlinkCode[]) {
  const order = new Map(COMMAND_ORDER.map((command, index) => [command, index]));
  return [...codes].sort((left, right) => {
    const leftIndex = order.get(left.command) ?? 999;
    const rightIndex = order.get(right.command) ?? 999;
    return leftIndex - rightIndex || left.command.localeCompare(right.command);
  });
}

function iconForCommand(command: string) {
  if (command === "off" || command === "on") return <Power className="h-4 w-4" />;
  if (command === "fan") return <Fan className="h-4 w-4" />;
  if (command === "vane") return <AirVent className="h-4 w-4" />;
  if (command === "too-warm") return <Waves className="h-4 w-4" />;
  return <Snowflake className="h-4 w-4" />;
}

function prettyCommand(command: string) {
  return command.replace(/-/g, " ");
}
