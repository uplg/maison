import { useState } from "react";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { feederApi } from "@/lib/api";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Slider } from "@/components/ui/slider";
import { Badge } from "@/components/ui/badge";
import { Skeleton } from "@/components/ui/skeleton";
import { Separator } from "@/components/ui/separator";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { toast } from "@/hooks/use-toast";
import { MealPlanManager } from "./MealPlanManager";
import { Utensils, Loader2, Clock, Battery, AlertTriangle, Calendar } from "lucide-react";

interface FeederControlProps {
  deviceId: string;
}

export function FeederControl({ deviceId }: FeederControlProps) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const [portions, setPortions] = useState([1]);

  const { data: statusData, isLoading: isLoadingStatus } = useQuery({
    queryKey: ["feeder", deviceId, "status"],
    queryFn: () => feederApi.status(deviceId),
    refetchInterval: 15000,
  });

  const { data: mealPlanData, isLoading: isLoadingMealPlan } = useQuery({
    queryKey: ["feeder", deviceId, "meal-plan"],
    queryFn: () => feederApi.getMealPlan(deviceId),
  });

  const feedMutation = useMutation({
    mutationFn: (portion: number) => feederApi.feed(deviceId, portion),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["feeder", deviceId] });
      toast({
        title: t("feeder.mealDistributed"),
        description: t("feeder.portionsDistributed", { count: portions[0] }),
      });
    },
    onError: (error) => {
      toast({
        title: t("common.error"),
        description: error instanceof Error ? error.message : t("feeder.distributionFailed"),
        variant: "destructive",
      });
    },
  });

  const parsedStatus = statusData?.parsed_status as
    | {
        food_level?: string;
        battery_level?: number;
        is_feeding?: boolean;
        error?: string;
      }
    | undefined;

  return (
    <Tabs defaultValue="control" className="space-y-4">
      <TabsList>
        <TabsTrigger value="control">
          <Utensils className="mr-2 h-4 w-4" />
          {t("feeder.control")}
        </TabsTrigger>
        <TabsTrigger value="schedule">
          <Calendar className="mr-2 h-4 w-4" />
          {t("feeder.schedule")}
        </TabsTrigger>
      </TabsList>

      <TabsContent value="control" className="space-y-4">
        <div className="grid gap-4 md:grid-cols-2">
          <Card>
            <CardHeader>
              <CardTitle className="flex items-center gap-2">
                <Utensils className="h-5 w-5" />
                {t("feeder.manualDistribution")}
              </CardTitle>
              <CardDescription>{t("feeder.manualDistributionDescription")}</CardDescription>
            </CardHeader>
            <CardContent className="space-y-6">
              <div className="space-y-4">
                <div className="flex items-center justify-between">
                  <span className="text-sm font-medium">{t("feeder.portions")}</span>
                  <Badge variant="outline" className="text-lg">
                    {portions[0]}
                  </Badge>
                </div>
                <Slider
                  value={portions}
                  onValueChange={setPortions}
                  min={1}
                  max={10}
                  step={1}
                  disabled={feedMutation.isPending}
                />
                <div className="flex justify-between text-xs text-muted-foreground">
                  <span>{t("feeder.portion", { count: 1 })}</span>
                  <span>{t("feeder.portion", { count: 10 })}</span>
                </div>
              </div>
              <Button
                onClick={() => feedMutation.mutate(portions[0])}
                disabled={feedMutation.isPending}
                className="w-full"
                size="lg"
              >
                {feedMutation.isPending ? (
                  <>
                    <Loader2 className="mr-2 h-5 w-5 animate-spin" />
                    {t("feeder.distributing")}
                  </>
                ) : (
                  <>
                    <Utensils className="mr-2 h-5 w-5" />
                    {t("feeder.distribute", { count: portions[0] })}
                  </>
                )}
              </Button>
            </CardContent>
          </Card>

          <Card>
            <CardHeader>
              <CardTitle>{t("feeder.feederStatus")}</CardTitle>
              <CardDescription>{t("feeder.realtimeInfo")}</CardDescription>
            </CardHeader>
            <CardContent>
              {isLoadingStatus ? (
                <div className="space-y-4">
                  <Skeleton className="h-6 w-full" />
                  <Skeleton className="h-6 w-full" />
                  <Skeleton className="h-6 w-full" />
                </div>
              ) : parsedStatus ? (
                <div className="space-y-4">
                  <div className="flex items-center justify-between">
                    <div className="flex items-center gap-2">
                      <Utensils className="h-4 w-4 text-muted-foreground" />
                      <span>{t("feeder.foodLevel")}</span>
                    </div>
                    <Badge
                      variant={
                        parsedStatus.food_level === "low"
                          ? "destructive"
                          : parsedStatus.food_level === "medium"
                            ? "warning"
                            : "success"
                      }
                    >
                      {parsedStatus.food_level === "low"
                        ? t("feeder.foodLevelLow")
                        : parsedStatus.food_level === "medium"
                          ? t("feeder.foodLevelMedium")
                          : t("feeder.foodLevelFull")}
                    </Badge>
                  </div>
                  <Separator />
                  {parsedStatus.battery_level !== undefined && (
                    <>
                      <div className="flex items-center justify-between">
                        <div className="flex items-center gap-2">
                          <Battery className="h-4 w-4 text-muted-foreground" />
                          <span>{t("feeder.battery")}</span>
                        </div>
                        <Badge
                          variant={
                            parsedStatus.battery_level < 20
                              ? "destructive"
                              : parsedStatus.battery_level < 50
                                ? "warning"
                                : "success"
                          }
                        >
                          {parsedStatus.battery_level}%
                        </Badge>
                      </div>
                      <Separator />
                    </>
                  )}
                  <div className="flex items-center justify-between">
                    <div className="flex items-center gap-2">
                      <Clock className="h-4 w-4 text-muted-foreground" />
                      <span>{t("common.status")}</span>
                    </div>
                    <Badge variant={parsedStatus.is_feeding ? "default" : "outline"}>
                      {parsedStatus.is_feeding ? t("feeder.feeding") : t("feeder.waiting")}
                    </Badge>
                  </div>
                  {parsedStatus.error && (
                    <>
                      <Separator />
                      <div className="flex items-center gap-2 text-destructive">
                        <AlertTriangle className="h-4 w-4" />
                        <span className="text-sm">{parsedStatus.error}</span>
                      </div>
                    </>
                  )}
                </div>
              ) : (
                <p className="text-muted-foreground">{t("feeder.statusFetchError")}</p>
              )}
            </CardContent>
          </Card>
        </div>
      </TabsContent>

      <TabsContent value="schedule" className="space-y-4">
        <Card>
          <CardHeader>
            <CardTitle className="flex items-center gap-2">
              <Calendar className="h-5 w-5" />
              {t("feeder.mealSchedule")}
            </CardTitle>
            <CardDescription>{t("feeder.mealScheduleDescription")}</CardDescription>
          </CardHeader>
          <CardContent>
            {isLoadingMealPlan ? (
              <div className="space-y-4">
                <Skeleton className="h-16 w-full" />
                <Skeleton className="h-16 w-full" />
                <Skeleton className="h-16 w-full" />
              </div>
            ) : (
              <MealPlanManager
                deviceId={deviceId}
                initialMealPlan={mealPlanData?.decoded || null}
              />
            )}
          </CardContent>
        </Card>
      </TabsContent>
    </Tabs>
  );
}
