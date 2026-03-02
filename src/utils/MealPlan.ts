export interface MealPlanEntry {
  days_of_week: string[];
  time: string;
  portion: number;
  status: "Enabled" | "Disabled";
}

export class MealPlan {
  private static readonly DAYS_OF_WEEK = [
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
    "Sunday",
  ];

  private static bitsTodays(bits: number): string[] {
    const days: string[] = [];
    for (let i = 0; i < 7; i++) {
      if (bits & (1 << i)) {
        days.push(this.DAYS_OF_WEEK[i]);
      }
    }
    return days;
  }

  private static daysToBits(daysList: string[]): number {
    let bits = 0;
    for (const day of daysList) {
      const index = this.DAYS_OF_WEEK.indexOf(day);
      if (index !== -1) {
        bits |= 1 << index;
      }
    }
    return bits;
  }

  static decode(encodedString: string): MealPlanEntry[] {
    try {
      const decodedBytes = Buffer.from(encodedString, "base64");
      const mealPlan: MealPlanEntry[] = [];

      for (let i = 0; i < decodedBytes.length; i += 5) {
        if (i + 4 >= decodedBytes.length) break;

        const daysOfWeekBits = decodedBytes[i];
        const timeHour = decodedBytes[i + 1];
        const timeMinute = decodedBytes[i + 2];
        const portionSize = decodedBytes[i + 3];
        const statusByte = decodedBytes[i + 4];

        const mealTime = `${timeHour.toString().padStart(2, "0")}:${timeMinute
          .toString()
          .padStart(2, "0")}`;
        const status = statusByte === 1 ? "Enabled" : "Disabled";

        mealPlan.push({
          days_of_week: this.bitsTodays(daysOfWeekBits),
          time: mealTime,
          portion: portionSize,
          status: status,
        });
      }

      return mealPlan;
    } catch (error) {
      console.error("Error while decoding meal plan:", error);
      throw new Error("Impossible to decode the meal plan");
    }
  }

  static encode(mealPlan: MealPlanEntry[]): string {
    try {
      const encodedBytes: number[] = [];

      for (const meal of mealPlan) {
        const daysOfWeekBits = this.daysToBits(meal.days_of_week);

        const [timeHour, timeMinute] = meal.time.split(":").map(Number);

        if (timeHour < 0 || timeHour > 23 || timeMinute < 0 || timeMinute > 59) {
          throw new Error(`Invalid time: ${meal.time}`);
        }

        if (meal.portion < 0 || meal.portion > 12) {
          throw new Error(`Invalid portion: ${meal.portion}`);
        }

        const statusByte = meal.status === "Enabled" ? 1 : 0;

        encodedBytes.push(daysOfWeekBits, timeHour, timeMinute, meal.portion, statusByte);
      }

      const buffer = Buffer.from(encodedBytes);
      return buffer.toString("base64");
    } catch (error) {
      console.error("Error while encoding meal plan:", error);
      throw new Error("Unable to encode the meal plan");
    }
  }

  static validate(entry: MealPlanEntry): boolean {
    if (!Array.isArray(entry.days_of_week) || entry.days_of_week.length === 0) {
      return false;
    }

    for (const day of entry.days_of_week) {
      if (!this.DAYS_OF_WEEK.includes(day)) {
        return false;
      }
    }

    const timeRegex = /^([0-1]?[0-9]|2[0-3]):[0-5][0-9]$/;
    if (!timeRegex.test(entry.time)) {
      return false;
    }

    if (typeof entry.portion !== "number" || entry.portion < 0 || entry.portion > 12) {
      return false;
    }

    if (entry.status !== "Enabled" && entry.status !== "Disabled") {
      return false;
    }

    return true;
  }

  static format(mealPlan: MealPlanEntry[]): string {
    return mealPlan
      .map((meal, index) => {
        const days = meal.days_of_week.join(", ");
        return `${index + 1}. ${days} à ${meal.time} - ${meal.portion} serving(s) - ${meal.status}`;
      })
      .join("\n");
  }
}
