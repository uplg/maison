import { useParams, Link } from "react-router-dom";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { ArrowLeft } from "lucide-react";
import { HueLampControl } from "@/components/devices/HueLampControl";

export function HueLampPage() {
  const { t } = useTranslation();
  const { lampId } = useParams<{ lampId: string }>();

  if (!lampId) {
    return (
      <div className="flex flex-col items-center justify-center py-12">
        <p className="text-lg font-medium">{t("hueLamps.notFound")}</p>
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
        <h1 className="text-2xl font-bold">{t("hueLamps.lampControl")}</h1>
      </div>

      {/* Lamp Control */}
      <HueLampControl lampId={lampId} />
    </div>
  );
}
