import { useQuery } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { useNavigate } from "react-router-dom";
import { tempoApi } from "@/lib/api";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Skeleton } from "@/components/ui/skeleton";
import { Zap, Calendar, ArrowRight, Euro, TrendingUp } from "lucide-react";

type TempoColor = "BLUE" | "WHITE" | "RED" | null;

const colorStyles: Record<string, { bg: string; text: string; border: string; light: string }> = {
  BLUE: {
    bg: "bg-blue-500",
    text: "text-white",
    border: "border-blue-600",
    light: "bg-blue-50 text-blue-700",
  },
  WHITE: {
    bg: "bg-gray-100",
    text: "text-gray-800",
    border: "border-gray-300",
    light: "bg-gray-50 text-gray-700",
  },
  RED: {
    bg: "bg-red-500",
    text: "text-white",
    border: "border-red-600",
    light: "bg-red-50 text-red-700",
  },
};

function ColorBadge({
  color,
  label,
  isPrediction,
}: {
  color: TempoColor;
  label: string;
  isPrediction?: boolean;
}) {
  const { t } = useTranslation();

  if (!color) {
    return (
      <div className="flex flex-col items-center gap-1">
        <span className="text-xs text-muted-foreground">{label}</span>
        <div className="flex h-12 w-12 items-center justify-center rounded-full border-2 border-dashed border-muted-foreground/30 bg-muted/50">
          <span className="text-xs text-muted-foreground">?</span>
        </div>
        <span className="text-xs font-medium text-muted-foreground">{t("tempo.unknown")}</span>
      </div>
    );
  }

  const styles = colorStyles[color];

  return (
    <div className="flex flex-col items-center gap-1">
      <span className="text-xs text-muted-foreground">{label}</span>
      <div
        className={`flex h-12 w-12 items-center justify-center rounded-full border-2 ${styles.bg} ${styles.text} ${styles.border} shadow-sm ${isPrediction ? "border-dashed opacity-80" : ""}`}
      >
        {isPrediction ? <TrendingUp className="h-5 w-5" /> : <Zap className="h-5 w-5" />}
      </div>
      <span className={`text-xs font-medium ${color === "WHITE" ? "text-gray-700" : ""}`}>
        {t(`tempo.colors.${color.toLowerCase()}`)}
        {isPrediction && <span className="text-muted-foreground"> *</span>}
      </span>
    </div>
  );
}

function TempoSkeleton() {
  return (
    <Card>
      <CardHeader className="pb-2">
        <div className="flex items-center gap-2">
          <Skeleton className="h-8 w-8 rounded-lg" />
          <Skeleton className="h-5 w-24" />
        </div>
      </CardHeader>
      <CardContent>
        <div className="flex items-center justify-center gap-4">
          <div className="flex flex-col items-center gap-1">
            <Skeleton className="h-3 w-12" />
            <Skeleton className="h-12 w-12 rounded-full" />
            <Skeleton className="h-3 w-10" />
          </div>
          <Skeleton className="h-4 w-4" />
          <div className="flex flex-col items-center gap-1">
            <Skeleton className="h-3 w-12" />
            <Skeleton className="h-12 w-12 rounded-full" />
            <Skeleton className="h-3 w-10" />
          </div>
        </div>
      </CardContent>
    </Card>
  );
}

export function TempoCard() {
  const { t } = useTranslation();
  const navigate = useNavigate();

  const { data, isLoading, error } = useQuery({
    queryKey: ["tempo"],
    queryFn: tempoApi.get,
    refetchInterval: 30 * 60 * 1000, // Refresh every 30 minutes
    staleTime: 10 * 60 * 1000, // Consider data stale after 10 minutes
    retry: 2,
  });

  // Fetch predictions as fallback when official colors are not available
  const { data: predictionsData } = useQuery({
    queryKey: ["tempo-predictions"],
    queryFn: tempoApi.getPredictions,
    refetchInterval: 30 * 60 * 1000,
    staleTime: 15 * 60 * 1000,
    // Only fetch if we might need predictions
    enabled: !!data?.success,
  });

  if (isLoading) {
    return <TempoSkeleton />;
  }

  // Don't render the card if RTE credentials are not configured
  if (error || !data?.success) {
    // Silently return null - Tempo is optional
    return null;
  }

  const formatDate = (dateStr: string | undefined) => {
    if (!dateStr) return "";
    const date = new Date(dateStr);
    return date.toLocaleDateString("fr-FR", { weekday: "short", day: "numeric", month: "short" });
  };

  const formatPrice = (price: number) => {
    return (price * 100).toFixed(2); // Convert €/kWh to c€/kWh
  };

  // Get prediction for a specific date
  const getPredictionForDate = (dateStr: string | undefined) => {
    if (!dateStr || !predictionsData?.predictions) return null;
    const targetDate = dateStr.split("T")[0]; // Normalize to YYYY-MM-DD
    return predictionsData.predictions.find((p) => p.date === targetDate);
  };

  // Determine colors to display (official or prediction fallback)
  const todayColor = data.today?.color as TempoColor;
  const todayPrediction = !todayColor ? getPredictionForDate(data.today?.date) : null;
  const displayTodayColor = todayColor || (todayPrediction?.predicted_color as TempoColor);
  const isTodayPrediction = !todayColor && !!todayPrediction;

  const tomorrowColor = data.tomorrow?.color as TempoColor;
  const tomorrowPrediction = !tomorrowColor ? getPredictionForDate(data.tomorrow?.date) : null;
  const displayTomorrowColor = tomorrowColor || (tomorrowPrediction?.predicted_color as TempoColor);
  const isTomorrowPrediction = !tomorrowColor && !!tomorrowPrediction;

  // Get current tariff based on today's color
  const getCurrentTarif = () => {
    if (!data.tarifs || !displayTodayColor) return null;
    const colorKey = displayTodayColor.toLowerCase() as "blue" | "white" | "red";
    return data.tarifs[colorKey];
  };

  const currentTarif = getCurrentTarif();

  return (
    <Card className="border-l-4 border-l-yellow-500">
      <CardHeader className="pb-2">
        <CardTitle
          className="flex items-center gap-2 text-base cursor-pointer hover:text-yellow-600 transition-colors group"
          onClick={() => navigate("/tempo-predictions")}
        >
          <div className="flex h-8 w-8 items-center justify-center rounded-lg bg-yellow-100 text-yellow-600">
            <Calendar className="h-4 w-4" />
          </div>
          <span className="flex-1">{t("tempo.title")}</span>
          <TrendingUp className="h-4 w-4 text-muted-foreground group-hover:text-yellow-600 transition-colors" />
        </CardTitle>
      </CardHeader>
      <CardContent className="space-y-4">
        {/* Colors section */}
        <div className="flex items-center justify-center gap-6">
          <ColorBadge
            color={displayTodayColor}
            label={`${t("tempo.today")} ${formatDate(data.today?.date)}`}
            isPrediction={isTodayPrediction}
          />
          <ArrowRight className="h-4 w-4 text-muted-foreground" />
          <ColorBadge
            color={displayTomorrowColor}
            label={`${t("tempo.tomorrow")} ${formatDate(data.tomorrow?.date)}`}
            isPrediction={isTomorrowPrediction}
          />
        </div>

        {/* Prediction notice */}
        {(isTodayPrediction || isTomorrowPrediction) && (
          <p className="text-center text-xs text-muted-foreground">
            * {t("tempo.predictionNotice")}
          </p>
        )}

        {/* Current tariff section */}
        {currentTarif && displayTodayColor && (
          <div
            className={`rounded-lg p-3 ${colorStyles[displayTodayColor]?.light || "bg-gray-50"}`}
          >
            <div className="flex items-center justify-center gap-2 mb-2">
              <Euro className="h-4 w-4" />
              <span className="text-sm font-medium">
                {t("tempo.currentTarif")}
                {isTodayPrediction && (
                  <span className="text-muted-foreground"> ({t("tempo.estimated")})</span>
                )}
              </span>
            </div>
            <div className="flex justify-center gap-6 text-sm">
              <div className="text-center">
                <div className="text-xs text-muted-foreground">{t("tempo.offPeak")}</div>
                <div className="font-semibold">{formatPrice(currentTarif.hc)} c€/kWh</div>
              </div>
              <div className="text-center">
                <div className="text-xs text-muted-foreground">{t("tempo.peak")}</div>
                <div className="font-semibold">{formatPrice(currentTarif.hp)} c€/kWh</div>
              </div>
            </div>
          </div>
        )}

        {/* All tariffs summary */}
        {data.tarifs && (
          <div className="text-xs text-muted-foreground">
            <details className="cursor-pointer">
              <summary className="flex items-center gap-1 hover:text-foreground">
                {t("tempo.allTarifs")}
              </summary>
              <div className="mt-2 grid grid-cols-3 gap-2 text-center">
                <div className="rounded bg-blue-50 p-2">
                  <div className="font-medium text-blue-700">{t("tempo.colors.blue")}</div>
                  <div className="text-blue-600">HC: {formatPrice(data.tarifs.blue.hc)}</div>
                  <div className="text-blue-600">HP: {formatPrice(data.tarifs.blue.hp)}</div>
                </div>
                <div className="rounded bg-gray-100 p-2">
                  <div className="font-medium text-gray-700">{t("tempo.colors.white")}</div>
                  <div className="text-gray-600">HC: {formatPrice(data.tarifs.white.hc)}</div>
                  <div className="text-gray-600">HP: {formatPrice(data.tarifs.white.hp)}</div>
                </div>
                <div className="rounded bg-red-50 p-2">
                  <div className="font-medium text-red-700">{t("tempo.colors.red")}</div>
                  <div className="text-red-600">HC: {formatPrice(data.tarifs.red.hc)}</div>
                  <div className="text-red-600">HP: {formatPrice(data.tarifs.red.hp)}</div>
                </div>
              </div>
            </details>
          </div>
        )}

        {data.cached && (
          <p className="mt-2 text-center text-xs text-muted-foreground">{t("tempo.cached")}</p>
        )}
      </CardContent>
    </Card>
  );
}
