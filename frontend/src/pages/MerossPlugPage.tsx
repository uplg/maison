import { useParams, Link } from "react-router-dom";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { ArrowLeft } from "lucide-react";
import { MerossPlugControl } from "@/components/devices/MerossPlugControl";

export function MerossPlugPage() {
  const { t } = useTranslation();
  const { deviceId } = useParams<{ deviceId: string }>();

  if (!deviceId) {
    return (
      <div className="flex flex-col items-center justify-center py-12">
        <p className="text-lg font-medium">{t("meross.notFound")}</p>
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
      {/* Header */}
      <div className="flex items-center gap-4">
        <Link to="/">
          <Button variant="ghost" size="icon">
            <ArrowLeft className="h-5 w-5" />
          </Button>
        </Link>
        <h1 className="text-2xl font-bold">{t("meross.plugControl")}</h1>
      </div>

      {/* Plug Control */}
      <MerossPlugControl deviceId={deviceId} />
    </div>
  );
}
