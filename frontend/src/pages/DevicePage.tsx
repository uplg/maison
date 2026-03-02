import { useParams, Link } from "react-router-dom";
import { useQuery } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { devicesApi } from "@/lib/api";
import { Button } from "@/components/ui/button";
import { Skeleton } from "@/components/ui/skeleton";
import { Badge } from "@/components/ui/badge";
import { ArrowLeft, Wifi, WifiOff } from "lucide-react";
import { FeederControl } from "@/components/devices/FeederControl";
import { FountainControl } from "@/components/devices/FountainControl";
import { LitterBoxControl } from "@/components/devices/LitterBoxControl";

export function DevicePage() {
  const { t } = useTranslation();
  const { deviceId } = useParams<{ deviceId: string }>();

  const { data: devicesData, isLoading: isLoadingDevices } = useQuery({
    queryKey: ["devices"],
    queryFn: devicesApi.list,
  });

  const device = devicesData?.devices.find((d) => d.id === deviceId);

  if (isLoadingDevices) {
    return (
      <div className="space-y-6">
        <div className="flex items-center gap-4">
          <Skeleton className="h-10 w-10" />
          <Skeleton className="h-8 w-50" />
        </div>
        <Skeleton className="h-100 w-full" />
      </div>
    );
  }

  if (!device) {
    return (
      <div className="flex flex-col items-center justify-center py-12">
        <p className="text-lg font-medium">{t("device.notFound")}</p>
        <p className="mt-2 text-muted-foreground">{t("device.notFoundDescription")}</p>
        <Link to="/">
          <Button className="mt-4">
            <ArrowLeft className="mr-2 h-4 w-4" />
            {t("device.backToDashboard")}
          </Button>
        </Link>
      </div>
    );
  }

  return (
    <div className="space-y-6">
      <div className="flex items-center gap-4">
        <Link to="/">
          <Button variant="outline" size="icon">
            <ArrowLeft className="h-4 w-4" />
          </Button>
        </Link>
        <div>
          <h1 className="flex items-center gap-3 text-2xl font-bold">
            {device.name}
            {device.connected ? (
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
          </h1>
          <p className="text-muted-foreground">
            {device.product_name} • {device.ip} • v{device.version}
          </p>
        </div>
      </div>

      {device.type === "feeder" && <FeederControl deviceId={device.id} />}
      {device.type === "fountain" && <FountainControl deviceId={device.id} />}
      {device.type === "litter-box" && <LitterBoxControl deviceId={device.id} />}
      {device.type === "unknown" && (
        <div className="rounded-lg border p-8 text-center">
          <p className="text-muted-foreground">{t("device.unknownType")}</p>
        </div>
      )}
    </div>
  );
}
