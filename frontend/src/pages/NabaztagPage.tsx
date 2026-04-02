import { Link } from "react-router-dom";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { ArrowLeft } from "lucide-react";
import { NabaztagFullControl } from "@/components/devices/NabaztagControl";

export function NabaztagPage() {
  const { t } = useTranslation();

  return (
    <div className="space-y-6">
      {/* Header */}
      <div className="flex items-center gap-4">
        <Link to="/">
          <Button variant="ghost" size="icon">
            <ArrowLeft className="h-5 w-5" />
          </Button>
        </Link>
        <h1 className="text-2xl font-bold">{t("nabaztag.pageTitle")}</h1>
      </div>

      {/* Full Control */}
      <NabaztagFullControl />
    </div>
  );
}
