import { useEffect, useRef, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { Link } from "react-router-dom";
import { zigbeeLampsApi, type ZigbeeLamp } from "@/lib/api";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { Skeleton } from "@/components/ui/skeleton";
import { Slider } from "@/components/ui/slider";
import { Switch } from "@/components/ui/switch";
import { Label } from "@/components/ui/label";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { toast } from "@/hooks/use-toast";
import {
  Lightbulb,
  LightbulbOff,
  Thermometer,
  Wifi,
  WifiOff,
  Sun,
  Radio,
  Loader2,
  Pencil,
} from "lucide-react";

interface ZigbeeLampControlProps {
  lampId: string;
}

export function ZigbeeLampControl({ lampId }: ZigbeeLampControlProps) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const [renameDialogOpen, setRenameDialogOpen] = useState(false);
  const [renameValue, setRenameValue] = useState("");
  const { data, isLoading } = useQuery({
    queryKey: ["zigbee-lamp", lampId],
    queryFn: () => zigbeeLampsApi.status(lampId),
    refetchInterval: 3000,
  });

  const lamp = data?.lamp;
  const [localBrightness, setLocalBrightness] = useState<number[]>([lamp?.state.brightness ?? 0]);
  const [localTemperature, setLocalTemperature] = useState<number[]>([lamp?.state.temperature ?? 50]);
  const targetBrightnessRef = useRef<number | null>(null);
  const targetTemperatureRef = useRef<number | null>(null);
  const powerCooldownRef = useRef<number>(0);

  useEffect(() => {
    if (lamp) {
      setRenameValue(lamp.name);
    }
  }, [lamp?.name]);

  const invalidateZigbeeQueries = () => {
    queryClient.invalidateQueries({ queryKey: ["zigbee-lamps"] });
    queryClient.invalidateQueries({ queryKey: ["zigbee-lamps-stats"] });
    queryClient.invalidateQueries({ queryKey: ["zigbee-lamp", lampId] });
  };

  const renameMutation = useMutation({
    mutationFn: (name: string) => zigbeeLampsApi.rename(lampId, name),
    onSuccess: () => {
      invalidateZigbeeQueries();
      setRenameDialogOpen(false);
      toast({
        title: t("zigbeeLamps.renameSuccess"),
        description: t("zigbeeLamps.renameSuccessDescription"),
      });
    },
    onError: (error) => {
      toast({
        title: t("common.error"),
        description: error instanceof Error ? error.message : t("zigbeeLamps.renameFailed"),
        variant: "destructive",
      });
    },
  });

  const powerMutation = useMutation({
    mutationFn: (enabled: boolean) => zigbeeLampsApi.power(lampId, enabled),
    onSuccess: (_, enabled) => {
      invalidateZigbeeQueries();
      toast({
        title: enabled ? t("zigbeeLamps.lampOn") : t("zigbeeLamps.lampOff"),
        description: t("zigbeeLamps.powerChanged"),
      });
    },
    onError: (error) => {
      toast({
        title: t("common.error"),
        description: error instanceof Error ? error.message : t("zigbeeLamps.powerFailed"),
        variant: "destructive",
      });
    },
  });

  const brightnessMutation = useMutation({
    mutationFn: (value: number) => zigbeeLampsApi.brightness(lampId, value),
    onSuccess: () => {
      invalidateZigbeeQueries();
    },
    onError: (error) => {
      targetBrightnessRef.current = null;
      toast({
        title: t("common.error"),
        description: error instanceof Error ? error.message : t("zigbeeLamps.brightnessFailed"),
        variant: "destructive",
      });
    },
  });

  const temperatureMutation = useMutation({
    mutationFn: (value: number) => zigbeeLampsApi.temperature(lampId, value),
    onSuccess: () => {
      invalidateZigbeeQueries();
    },
    onError: (error) => {
      targetTemperatureRef.current = null;
      toast({
        title: t("common.error"),
        description: error instanceof Error ? error.message : t("zigbeeLamps.temperatureFailed"),
        variant: "destructive",
      });
    },
  });

  useEffect(() => {
    if (lamp?.state.brightness === undefined) return;
    const now = Date.now();
    if (now - powerCooldownRef.current < 1000) return;
    const serverValue = lamp.state.brightness;
    const target = targetBrightnessRef.current;

    if (target !== null) {
      const isClose = Math.abs(serverValue - target) <= 2;
      if (isClose) {
        targetBrightnessRef.current = null;
        setLocalBrightness([serverValue]);
      }
    } else {
      setLocalBrightness([serverValue]);
    }
  }, [lamp?.state.brightness]);

  useEffect(() => {
    if (lamp?.state.temperature === undefined || lamp?.state.temperature === null) return;
    const serverValue = lamp.state.temperature;
    const target = targetTemperatureRef.current;

    if (target !== null) {
      const isClose = Math.abs(serverValue - target) <= 2;
      if (isClose) {
        targetTemperatureRef.current = null;
        setLocalTemperature([serverValue]);
      }
    } else {
      setLocalTemperature([serverValue]);
    }
  }, [lamp?.state.temperature]);

  if (isLoading) {
    return (
      <div className="space-y-4">
        <Card>
          <CardHeader>
            <Skeleton className="h-6 w-48" />
            <Skeleton className="h-4 w-64" />
          </CardHeader>
          <CardContent className="space-y-6">
            <Skeleton className="h-10 w-full" />
            <Skeleton className="h-10 w-full" />
          </CardContent>
        </Card>
      </div>
    );
  }

  if (!lamp) {
    return (
      <Card>
        <CardContent className="py-8 text-center">
          <LightbulbOff className="mx-auto h-12 w-12 text-muted-foreground" />
          <p className="mt-4 text-muted-foreground">{t("zigbeeLamps.notFound")}</p>
        </CardContent>
      </Card>
    );
  }

  const isConnected = lamp.reachable;

  return (
    <div className="space-y-4">
      <Card>
        <CardHeader>
          <div className="flex items-center justify-between gap-4">
            <div className="flex items-center gap-3">
              <div
                className={`flex h-12 w-12 items-center justify-center rounded-xl ${
                  lamp.state.isOn ? "bg-amber-100 text-amber-700" : "bg-slate-100 text-slate-400"
                }`}
              >
                {lamp.state.isOn ? <Lightbulb className="h-6 w-6" /> : <LightbulbOff className="h-6 w-6" />}
              </div>
              <div>
                <div className="flex items-center gap-2">
                  <CardTitle>{lamp.name}</CardTitle>
                  <Dialog open={renameDialogOpen} onOpenChange={setRenameDialogOpen}>
                    <DialogTrigger asChild>
                      <Button variant="ghost" size="icon" className="h-8 w-8">
                        <Pencil className="h-4 w-4" />
                      </Button>
                    </DialogTrigger>
                    <DialogContent>
                      <DialogHeader>
                        <DialogTitle>{t("zigbeeLamps.renameTitle")}</DialogTitle>
                        <DialogDescription>
                          {t("zigbeeLamps.renameDescription", { name: lamp.name })}
                        </DialogDescription>
                      </DialogHeader>
                      <Input value={renameValue} onChange={(event) => setRenameValue(event.target.value)} />
                      <DialogFooter>
                        <Button variant="outline" onClick={() => setRenameDialogOpen(false)}>
                          {t("common.cancel")}
                        </Button>
                        <Button
                          onClick={() => renameMutation.mutate(renameValue)}
                          disabled={renameMutation.isPending || renameValue.trim().length === 0}
                        >
                          {renameMutation.isPending ? <Loader2 className="mr-2 h-4 w-4 animate-spin" /> : null}
                          {t("zigbeeLamps.renameConfirm")}
                        </Button>
                      </DialogFooter>
                    </DialogContent>
                  </Dialog>
                </div>
                <CardDescription>
                  {lamp.model || t("zigbeeLamps.unknownModel")} - {lamp.manufacturer}
                </CardDescription>
              </div>
            </div>
            <Switch
              checked={lamp.state.isOn}
              onCheckedChange={(checked) => {
                powerCooldownRef.current = Date.now();
                powerMutation.mutate(checked);
              }}
              disabled={!isConnected || powerMutation.isPending}
            />
          </div>
        </CardHeader>
        <CardContent className="space-y-6">
          <div className="flex flex-wrap gap-2">
            {lamp.reachable ? (
              <Badge variant="success">
                <Wifi className="mr-1 h-3 w-3" />
                {t("common.connected")}
              </Badge>
            ) : (
              <Badge variant="secondary">
                <WifiOff className="mr-1 h-3 w-3" />
                {t("common.disconnected")}
              </Badge>
            )}
            <Badge variant="outline">
              <Radio className="mr-1 h-3 w-3" />
              {lamp.interviewCompleted ? t("zigbeeLamps.interviewComplete") : t("zigbeeLamps.interviewPending")}
            </Badge>
          </div>

          <div className="space-y-3">
            <div>
              <Label>{t("zigbeeLamps.brightness")}</Label>
              <p className="text-sm text-muted-foreground">{t("zigbeeLamps.brightnessDescription")}</p>
            </div>
            <div className="flex items-center gap-3">
              <Sun className="h-4 w-4 text-muted-foreground" />
              <Slider
                value={localBrightness}
                onValueChange={setLocalBrightness}
                onValueCommit={(value) => {
                  const newValue = value[0];
                  targetBrightnessRef.current = newValue;
                  setLocalBrightness([newValue]);
                  brightnessMutation.mutate(newValue);
                }}
                min={0}
                max={100}
                step={1}
                disabled={!isConnected || !lamp.state.isOn || brightnessMutation.isPending}
              />
              <span className="w-10 text-right text-sm text-muted-foreground">{localBrightness[0]}%</span>
            </div>
          </div>

          {lamp.state.temperature !== null && (
            <div className="space-y-3">
              <div>
                <Label>{t("zigbeeLamps.temperature")}</Label>
                <p className="text-sm text-muted-foreground">{t("zigbeeLamps.temperatureDescription")}</p>
              </div>
              <div className="flex items-center gap-3">
                <Thermometer className="h-4 w-4 text-muted-foreground" />
                <Slider
                  value={localTemperature}
                  onValueChange={setLocalTemperature}
                  onValueCommit={(value) => {
                    const newValue = value[0];
                    targetTemperatureRef.current = newValue;
                    setLocalTemperature([newValue]);
                    temperatureMutation.mutate(newValue);
                  }}
                  min={lamp.state.temperatureMin ?? 0}
                  max={lamp.state.temperatureMax ?? 100}
                  step={1}
                  disabled={!isConnected || !lamp.state.isOn || temperatureMutation.isPending}
                />
                <span className="w-10 text-right text-sm text-muted-foreground">{localTemperature[0]}%</span>
              </div>
            </div>
          )}
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>{t("zigbeeLamps.deviceInfo")}</CardTitle>
          <CardDescription>{t("zigbeeLamps.deviceInfoDescription")}</CardDescription>
        </CardHeader>
        <CardContent className="space-y-3 text-sm">
          <div className="flex justify-between gap-4">
            <span className="text-muted-foreground">{t("zigbeeLamps.friendlyName")}</span>
            <span className="font-mono text-right">{lamp.friendlyName}</span>
          </div>
          <div className="flex justify-between gap-4">
            <span className="text-muted-foreground">{t("zigbeeLamps.address")}</span>
            <span className="font-mono text-right">{lamp.address}</span>
          </div>
          <div className="flex justify-between gap-4">
            <span className="text-muted-foreground">{t("zigbeeLamps.linkQuality")}</span>
            <span>{lamp.linkQuality ?? "-"}</span>
          </div>
          <div className="flex justify-between gap-4">
            <span className="text-muted-foreground">{t("zigbeeLamps.lastSeen")}</span>
            <span>{lamp.lastSeen ? new Date(lamp.lastSeen).toLocaleString() : "-"}</span>
          </div>
        </CardContent>
      </Card>
    </div>
  );
}

interface ZigbeeLampCardProps {
  lamp: ZigbeeLamp;
}

export function ZigbeeLampCard({ lamp }: ZigbeeLampCardProps) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const [localBrightness, setLocalBrightness] = useState([lamp.state.brightness]);
  const [localTemperature, setLocalTemperature] = useState([lamp.state.temperature ?? 50]);
  const targetBrightnessRef = useRef<number | null>(null);
  const targetTemperatureRef = useRef<number | null>(null);
  const powerCooldownRef = useRef<number>(0);

  const invalidate = () => {
    queryClient.invalidateQueries({ queryKey: ["zigbee-lamps"] });
    queryClient.invalidateQueries({ queryKey: ["zigbee-lamps-stats"] });
  };

  const powerMutation = useMutation({
    mutationFn: (enabled: boolean) => zigbeeLampsApi.power(lamp.id, enabled),
    onSuccess: invalidate,
    onError: (error) => {
      toast({
        title: t("common.error"),
        description: error instanceof Error ? error.message : t("zigbeeLamps.powerFailed"),
        variant: "destructive",
      });
    },
  });

  const brightnessMutation = useMutation({
    mutationFn: (value: number) => zigbeeLampsApi.brightness(lamp.id, value),
    onSuccess: invalidate,
  });

  const temperatureMutation = useMutation({
    mutationFn: (value: number) => zigbeeLampsApi.temperature(lamp.id, value),
    onSuccess: invalidate,
  });

  useEffect(() => {
    const now = Date.now();
    if (now - powerCooldownRef.current < 1000) return;
    const serverValue = lamp.state.brightness;
    const target = targetBrightnessRef.current;

    if (target !== null) {
      const isClose = Math.abs(serverValue - target) <= 2;
      if (isClose) {
        targetBrightnessRef.current = null;
        setLocalBrightness([serverValue]);
      }
    } else {
      setLocalBrightness([serverValue]);
    }
  }, [lamp.state.brightness]);

  useEffect(() => {
    if (lamp.state.temperature === null) return;
    const serverValue = lamp.state.temperature;
    const target = targetTemperatureRef.current;

    if (target !== null) {
      const isClose = Math.abs(serverValue - target) <= 2;
      if (isClose) {
        targetTemperatureRef.current = null;
        setLocalTemperature([serverValue]);
      }
    } else {
      setLocalTemperature([serverValue]);
    }
  }, [lamp.state.temperature]);

  return (
    <Card className="transition-shadow hover:shadow-lg">
      <CardHeader className="pb-2">
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-3">
            <div
              className={`flex h-10 w-10 items-center justify-center rounded-lg ${
                lamp.state.isOn ? "bg-amber-100 text-amber-700" : "bg-slate-100 text-slate-400"
              }`}
            >
              {lamp.state.isOn ? <Lightbulb className="h-5 w-5" /> : <LightbulbOff className="h-5 w-5" />}
            </div>
            <div>
              <Link to={`/zigbee-lamp/${lamp.id}`}>
                <CardTitle className="cursor-pointer text-base hover:underline">{lamp.name}</CardTitle>
              </Link>
              <CardDescription className="text-xs">{lamp.model || t("zigbeeLamps.unknownModel")}</CardDescription>
            </div>
          </div>
          <Switch
            checked={lamp.state.isOn}
            onCheckedChange={(checked) => {
              powerCooldownRef.current = Date.now();
              powerMutation.mutate(checked);
            }}
            disabled={!lamp.reachable || powerMutation.isPending}
          />
        </div>
      </CardHeader>
      <CardContent className="space-y-3">
        <div className="flex items-center justify-between text-xs">
          {lamp.reachable ? (
            <Badge variant="success" className="text-xs">
              <Wifi className="mr-1 h-2.5 w-2.5" />
              {t("common.connected")}
            </Badge>
          ) : (
            <Badge variant="secondary" className="text-xs">
              <WifiOff className="mr-1 h-2.5 w-2.5" />
              {t("common.disconnected")}
            </Badge>
          )}
          <span className="text-muted-foreground">{localBrightness[0]}%</span>
        </div>
        <div className="flex items-center gap-2">
          <Sun className="h-3 w-3 shrink-0 text-muted-foreground" />
          <Slider
            value={localBrightness}
            onValueChange={setLocalBrightness}
            onValueCommit={(value) => {
              const newValue = value[0];
              targetBrightnessRef.current = newValue;
              setLocalBrightness([newValue]);
              brightnessMutation.mutate(newValue);
            }}
            min={0}
            max={100}
            step={1}
            disabled={!lamp.reachable || !lamp.state.isOn}
          />
        </div>
        {lamp.state.temperature !== null && (
          <div className="flex items-center gap-2">
            <Thermometer className="h-3 w-3 shrink-0 text-muted-foreground" />
            <Slider
              value={localTemperature}
              onValueChange={setLocalTemperature}
              onValueCommit={(value) => {
                const newValue = value[0];
                targetTemperatureRef.current = newValue;
                setLocalTemperature([newValue]);
                temperatureMutation.mutate(newValue);
              }}
              min={lamp.state.temperatureMin ?? 0}
              max={lamp.state.temperatureMax ?? 100}
              step={1}
              disabled={!lamp.reachable || !lamp.state.isOn}
            />
          </div>
        )}
      </CardContent>
    </Card>
  );
}

export function ZigbeePairingPanel() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const { data } = useQuery({
    queryKey: ["zigbee-pairing-status"],
    queryFn: zigbeeLampsApi.pairingStatus,
    refetchInterval: 1000,
  });

  const pairing = data?.pairing;

  const startPairingMutation = useMutation({
    mutationFn: zigbeeLampsApi.startPairing,
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["zigbee-pairing-status"] });
      toast({
        title: t("zigbeeLamps.pairingStarted"),
        description: t("zigbeeLamps.pairingStartedDescription"),
      });
    },
    onError: (error) => {
      toast({
        title: t("common.error"),
        description: error instanceof Error ? error.message : t("zigbeeLamps.pairingFailed"),
        variant: "destructive",
      });
    },
  });

  const stopPairingMutation = useMutation({
    mutationFn: zigbeeLampsApi.stopPairing,
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["zigbee-pairing-status"] });
      toast({
        title: t("zigbeeLamps.pairingStopped"),
        description: t("zigbeeLamps.pairingStoppedDescription"),
      });
    },
    onError: (error) => {
      toast({
        title: t("common.error"),
        description: error instanceof Error ? error.message : t("zigbeeLamps.pairingFailed"),
        variant: "destructive",
      });
    },
  });

  return (
    <div className="flex flex-wrap items-center gap-2">
      {pairing?.active ? (
        <Badge variant="success">{t("zigbeeLamps.pairingActive", { count: pairing.remainingSeconds })}</Badge>
      ) : (
        <Badge variant="secondary">{t("zigbeeLamps.pairingInactive")}</Badge>
      )}
      {pairing?.message && <span className="text-sm text-muted-foreground">{pairing.message}</span>}
      <Button
        variant="outline"
        size="sm"
        onClick={() => startPairingMutation.mutate()}
        disabled={startPairingMutation.isPending || pairing?.active === true}
      >
        {startPairingMutation.isPending ? <Loader2 className="mr-2 h-4 w-4 animate-spin" /> : null}
        {t("zigbeeLamps.startPairing")}
      </Button>
      <Button
        variant="secondary"
        size="sm"
        onClick={() => stopPairingMutation.mutate()}
        disabled={stopPairingMutation.isPending || pairing?.active !== true}
      >
        {stopPairingMutation.isPending ? <Loader2 className="mr-2 h-4 w-4 animate-spin" /> : null}
        {t("zigbeeLamps.stopPairing")}
      </Button>
    </div>
  );
}
