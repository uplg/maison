import { useState, useEffect } from "react";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { feederApi, type MealPlanEntry } from "@/lib/api";
import { Button } from "@/components/ui/button";
import { Card, CardContent } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { Switch } from "@/components/ui/switch";
import { Label } from "@/components/ui/label";
import { Input } from "@/components/ui/input";
import { Slider } from "@/components/ui/slider";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from "@/components/ui/dialog";
import { toast } from "@/hooks/use-toast";
import { Clock, Plus, Trash2, Edit2, Save, Loader2, Calendar, Utensils } from "lucide-react";

const DAYS_OF_WEEK = [
  { short: "L", full: "Monday", key: "monday" },
  { short: "M", full: "Tuesday", key: "tuesday" },
  { short: "M", full: "Wednesday", key: "wednesday" },
  { short: "J", full: "Thursday", key: "thursday" },
  { short: "V", full: "Friday", key: "friday" },
  { short: "S", full: "Saturday", key: "saturday" },
  { short: "D", full: "Sunday", key: "sunday" },
];

interface MealPlanManagerProps {
  deviceId: string;
  initialMealPlan: MealPlanEntry[] | null;
}

function DaySelector({
  selectedDays,
  onChange,
}: {
  selectedDays: string[];
  onChange: (days: string[]) => void;
}) {
  const { t } = useTranslation();

  const toggleDay = (day: string) => {
    if (selectedDays.includes(day)) {
      onChange(selectedDays.filter((d) => d !== day));
    } else {
      onChange([...selectedDays, day]);
    }
  };

  const selectAll = () => {
    onChange(DAYS_OF_WEEK.map((d) => d.full));
  };

  const selectWeekdays = () => {
    onChange(DAYS_OF_WEEK.slice(0, 5).map((d) => d.full));
  };

  const selectWeekend = () => {
    onChange(DAYS_OF_WEEK.slice(5).map((d) => d.full));
  };

  return (
    <div className="space-y-3">
      <div className="flex flex-wrap gap-1 sm:gap-1">
        {DAYS_OF_WEEK.map((day) => (
          <button
            key={day.full}
            type="button"
            onClick={() => toggleDay(day.full)}
            className={`flex h-9 w-9 sm:h-10 sm:w-10 items-center justify-center rounded-full text-xs sm:text-sm font-medium transition-all ${
              selectedDays.includes(day.full)
                ? "bg-primary text-primary-foreground shadow-sm"
                : "bg-muted text-muted-foreground hover:bg-muted/80"
            }`}
          >
            {t(`mealPlan.days_${day.key}`)}
          </button>
        ))}
      </div>
      <div className="flex flex-wrap gap-2">
        <Button type="button" variant="ghost" size="sm" onClick={selectAll}>
          {t("common.all")}
        </Button>
        <Button type="button" variant="ghost" size="sm" onClick={selectWeekdays}>
          {t("mealPlan.weekdaysOnly")}
        </Button>
        <Button type="button" variant="ghost" size="sm" onClick={selectWeekend}>
          {t("mealPlan.weekend")}
        </Button>
      </div>
    </div>
  );
}

interface MealEditorProps {
  meal?: MealPlanEntry;
  onSave: (meal: MealPlanEntry) => void;
  onCancel: () => void;
}

function MealEditor({ meal, onSave, onCancel }: MealEditorProps) {
  const { t } = useTranslation();
  const [time, setTime] = useState(meal?.time || "08:00");
  const [portion, setPortion] = useState([meal?.portion || 1]);
  const [days, setDays] = useState<string[]>(meal?.days_of_week || DAYS_OF_WEEK.map((d) => d.full));
  const [enabled, setEnabled] = useState(meal?.status !== "Disabled");

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    if (days.length === 0) {
      toast({
        title: t("common.error"),
        description: t("mealPlan.selectAtLeastOneDay"),
        variant: "destructive",
      });
      return;
    }
    onSave({
      time,
      portion: portion[0],
      days_of_week: days,
      status: enabled ? "Enabled" : "Disabled",
    });
  };

  return (
    <form onSubmit={handleSubmit} className="space-y-6">
      {/* Time Picker */}
      <div className="space-y-2">
        <Label className="flex items-center gap-2">
          <Clock className="h-4 w-4" />
          {t("mealPlan.time")}
        </Label>
        <Input
          type="time"
          value={time}
          onChange={(e) => setTime(e.target.value)}
          className="text-2xl font-mono h-14 text-center"
        />
      </div>

      {/* Portions */}
      <div className="space-y-3">
        <Label className="flex items-center gap-2">
          <Utensils className="h-4 w-4" />
          {t("mealPlan.portions")}
        </Label>
        <div className="flex items-center gap-4">
          <Slider
            value={portion}
            onValueChange={setPortion}
            min={1}
            max={12}
            step={1}
            className="flex-1"
          />
          <Badge variant="secondary" className="text-lg min-w-12 justify-center">
            {portion[0]}
          </Badge>
        </div>
        <p className="text-xs text-muted-foreground">{t("mealPlan.portionsPerMeal")}</p>
      </div>

      {/* Days */}
      <div className="space-y-2">
        <Label className="flex items-center gap-2">
          <Calendar className="h-4 w-4" />
          {t("mealPlan.days")}
        </Label>
        <DaySelector selectedDays={days} onChange={setDays} />
      </div>

      {/* Enabled */}
      <div className="flex items-center justify-between rounded-lg border p-4">
        <div className="space-y-0.5">
          <Label>{t("mealPlan.enableMeal")}</Label>
          <p className="text-sm text-muted-foreground">{t("mealPlan.disableTemporarily")}</p>
        </div>
        <Switch checked={enabled} onCheckedChange={setEnabled} />
      </div>

      {/* Actions */}
      <div className="flex gap-3">
        <Button type="button" variant="outline" onClick={onCancel} className="flex-1">
          {t("common.cancel")}
        </Button>
        <Button type="submit" className="flex-1">
          <Save className="mr-2 h-4 w-4" />
          {t("common.save")}
        </Button>
      </div>
    </form>
  );
}

export function MealPlanManager({ deviceId, initialMealPlan }: MealPlanManagerProps) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const [mealPlan, setMealPlan] = useState<MealPlanEntry[]>(initialMealPlan || []);
  const [editingIndex, setEditingIndex] = useState<number | null>(null);
  const [isAddDialogOpen, setIsAddDialogOpen] = useState(false);
  const [hasChanges, setHasChanges] = useState(false);

  useEffect(() => {
    if (initialMealPlan) {
      setMealPlan(initialMealPlan);
      setHasChanges(false);
    }
  }, [initialMealPlan]);

  const saveMutation = useMutation({
    mutationFn: (plan: MealPlanEntry[]) => feederApi.setMealPlan(deviceId, plan),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["feeder", deviceId, "meal-plan"] });
      setHasChanges(false);
      toast({
        title: t("mealPlan.mealPlanSaved"),
        description: t("mealPlan.mealsScheduled", { count: mealPlan.length }),
      });
    },
    onError: (error) => {
      toast({
        title: t("common.error"),
        description: error instanceof Error ? error.message : t("mealPlan.saveFailed"),
        variant: "destructive",
      });
    },
  });

  const addMeal = (meal: MealPlanEntry) => {
    if (mealPlan.length >= 10) {
      toast({
        title: "Limite atteinte",
        description: "Maximum 10 repas programmés",
        variant: "destructive",
      });
      return;
    }
    setMealPlan([...mealPlan, meal]);
    setIsAddDialogOpen(false);
    setHasChanges(true);
  };

  const updateMeal = (index: number, meal: MealPlanEntry) => {
    const newPlan = [...mealPlan];
    newPlan[index] = meal;
    setMealPlan(newPlan);
    setEditingIndex(null);
    setHasChanges(true);
  };

  const deleteMeal = (index: number) => {
    setMealPlan(mealPlan.filter((_, i) => i !== index));
    setHasChanges(true);
  };

  const toggleMealStatus = (index: number) => {
    const newPlan = [...mealPlan];
    newPlan[index] = {
      ...newPlan[index],
      status: newPlan[index].status === "Enabled" ? "Disabled" : "Enabled",
    };
    setMealPlan(newPlan);
    setHasChanges(true);
  };

  const formatDays = (days: string[]) => {
    if (days.length === 7) return t("mealPlan.everyday");
    if (days.length === 5 && !days.includes("Saturday") && !days.includes("Sunday")) {
      return t("mealPlan.weekdays");
    }
    if (days.length === 2 && days.includes("Saturday") && days.includes("Sunday")) {
      return t("mealPlan.weekend");
    }
    return days
      .map((d) => {
        const day = DAYS_OF_WEEK.find((day) => day.full === d);
        return day ? t(`mealPlan.days_${day.key}`) : d;
      })
      .join(", ");
  };

  const sortedMealPlan = [...mealPlan].sort((a, b) => a.time.localeCompare(b.time));

  return (
    <div className="space-y-4">
      <div className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
        <div className="space-y-1">
          <p className="text-sm text-muted-foreground">
            {mealPlan.length}/10{" "}
            {t("mealPlan.mealsScheduled", { count: mealPlan.length }).split(" ").slice(1).join(" ")}
          </p>
        </div>
        <div className="flex gap-2">
          <Dialog open={isAddDialogOpen} onOpenChange={setIsAddDialogOpen}>
            <DialogTrigger asChild>
              <Button
                variant="outline"
                disabled={mealPlan.length >= 10}
                className="flex-1 sm:flex-none"
              >
                <Plus className="mr-2 h-4 w-4" />
                {t("common.add")}
              </Button>
            </DialogTrigger>
            <DialogContent className="sm:max-w-md">
              <DialogHeader>
                <DialogTitle className="flex items-center gap-2">
                  <Clock className="h-5 w-5" />
                  {t("mealPlan.addMeal")}
                </DialogTitle>
                <DialogDescription>{t("feeder.mealScheduleDescription")}</DialogDescription>
              </DialogHeader>
              <MealEditor onSave={addMeal} onCancel={() => setIsAddDialogOpen(false)} />
            </DialogContent>
          </Dialog>

          <Button
            onClick={() => saveMutation.mutate(mealPlan)}
            disabled={!hasChanges || saveMutation.isPending}
            className="flex-1 sm:flex-none"
          >
            {saveMutation.isPending ? (
              <Loader2 className="mr-2 h-4 w-4 animate-spin" />
            ) : (
              <Save className="mr-2 h-4 w-4" />
            )}
            {t("mealPlan.saveChanges")}
          </Button>
        </div>
      </div>

      {hasChanges && (
        <div className="rounded-lg border border-amber-200 bg-amber-50 p-3 text-sm text-amber-800 dark:border-amber-900 dark:bg-amber-950 dark:text-amber-200">
          ⚠️ {t("mealPlan.unsavedChanges")}
        </div>
      )}

      {sortedMealPlan.length === 0 ? (
        <Card className="border-dashed">
          <CardContent className="flex flex-col items-center justify-center py-12">
            <Calendar className="h-12 w-12 text-muted-foreground mb-4" />
            <p className="text-muted-foreground text-center">{t("mealPlan.noMeals")}</p>
            <p className="text-sm text-muted-foreground text-center mt-1">
              {t("mealPlan.noMealsDescription")}
            </p>
          </CardContent>
        </Card>
      ) : (
        <div className="space-y-3">
          {sortedMealPlan.map((meal) => {
            const originalIndex = mealPlan.indexOf(meal);

            return (
              <Card
                key={`${meal.time}-${originalIndex}`}
                className={`transition-all ${meal.status === "Disabled" ? "opacity-60" : ""}`}
              >
                <CardContent className="p-3 sm:p-4">
                  <div className="flex flex-col gap-3 sm:flex-row sm:items-center sm:gap-4">
                    <div
                      className={`flex h-12 w-full sm:h-16 sm:w-16 flex-row sm:flex-col items-center justify-center gap-2 sm:gap-0 rounded-xl ${
                        meal.status === "Enabled"
                          ? "bg-primary/10 text-primary"
                          : "bg-muted text-muted-foreground"
                      }`}
                    >
                      <Clock className="h-4 w-4 sm:h-5 sm:w-5 sm:mb-1" />
                      <span className="text-base sm:text-lg font-bold">{meal.time}</span>
                    </div>

                    <div className="flex-1 min-w-0">
                      <div className="flex flex-wrap items-center gap-2 mb-1">
                        <Badge variant="outline">
                          <Utensils className="mr-1 h-3 w-3" />
                          {t("feeder.portion", { count: meal.portion })}
                        </Badge>
                        <Badge variant={meal.status === "Enabled" ? "success" : "secondary"}>
                          {meal.status === "Enabled" ? t("common.enabled") : t("common.disabled")}
                        </Badge>
                      </div>
                      <p className="text-sm text-muted-foreground truncate">
                        {formatDays(meal.days_of_week)}
                      </p>
                    </div>

                    <div className="flex items-center justify-between sm:justify-end gap-2 sm:gap-1 border-t pt-3 sm:border-0 sm:pt-0">
                      <Switch
                        checked={meal.status === "Enabled"}
                        onCheckedChange={() => toggleMealStatus(originalIndex)}
                      />

                      <div className="flex items-center gap-1">
                        <Dialog
                          open={editingIndex === originalIndex}
                          onOpenChange={(open) => setEditingIndex(open ? originalIndex : null)}
                        >
                          <DialogTrigger asChild>
                            <Button variant="ghost" size="icon">
                              <Edit2 className="h-4 w-4" />
                            </Button>
                          </DialogTrigger>
                          <DialogContent className="sm:max-w-md">
                            <DialogHeader>
                              <DialogTitle className="flex items-center gap-2">
                                <Edit2 className="h-5 w-5" />
                                {t("mealPlan.editMeal")}
                              </DialogTitle>
                              <DialogDescription>
                                {t("feeder.mealScheduleDescription")}
                              </DialogDescription>
                            </DialogHeader>
                            <MealEditor
                              meal={meal}
                              onSave={(updated) => updateMeal(originalIndex, updated)}
                              onCancel={() => setEditingIndex(null)}
                            />
                          </DialogContent>
                        </Dialog>

                        <Button
                          variant="ghost"
                          size="icon"
                          className="text-destructive hover:text-destructive"
                          onClick={() => deleteMeal(originalIndex)}
                        >
                          <Trash2 className="h-4 w-4" />
                        </Button>
                      </div>
                    </div>
                  </div>
                </CardContent>
              </Card>
            );
          })}
        </div>
      )}

      {mealPlan.length > 0 && (
        <Card className="bg-muted/50">
          <CardContent className="p-4">
            <div className="flex items-center justify-between text-sm">
              <span className="text-muted-foreground">{t("mealPlan.totalPortionsPerDay")}</span>
              <Badge variant="outline" className="text-base">
                ~
                {mealPlan
                  .filter((m) => m.status === "Enabled")
                  .reduce((acc, m) => acc + m.portion, 0)}{" "}
                {t("mealPlan.portions")}
              </Badge>
            </div>
          </CardContent>
        </Card>
      )}
    </div>
  );
}
