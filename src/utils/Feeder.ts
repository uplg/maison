/**
 * Feeder status parsing utilities
 * Based on DPS reference table for Smart Feeders
 */

import { DPSObject } from "tuyapi";

/**
 * Parse feeder status from raw DPS data
 * @param rawStatus - Raw DPS data from device
 * @returns Parsed feeder status object
 */
export function parseFeederStatus(status: DPSObject) {
  const rawDps = status.dps || {};

  // Parse feed history if available (DPS 104)
  const historyData = rawDps?.["104"];
  let history = null;
  if (typeof historyData === "string") {
    // NOTE: Format "R:0  C:2  T:1758445204"
    const parts = historyData.split("  ");
    history = {
      raw: historyData,
      parsed: {
        // servings to give
        remaining: parts[0]?.replace("R:", "") || null,
        // servings given
        count: parts[1]?.replace("C:", "") || null,
        // time last serving
        timestamp: parts[2]?.replace("T:", "") || null,
        timestamp_readable: "",
      },
    };

    if (history.parsed.timestamp) {
      const timestamp = parseInt(history.parsed.timestamp);
      if (!isNaN(timestamp)) {
        const date = new Date(timestamp > 1000000000000 ? timestamp : timestamp * 1000);
        history.parsed.timestamp_readable = date.toISOString();
      }
    }
  }

  // Parse feed size (DPS 101)
  let feedSize = "Unknown";
  if (rawDps["101"] !== undefined) {
    const sizeValue = rawDps["101"];
    if (typeof sizeValue === "number") {
      feedSize = `${sizeValue} portion${sizeValue > 1 ? "s" : ""}`;
    }
  }

  // Parse powered by status (DPS 105)
  let poweredBy = "Unknown";
  if (rawDps["105"] !== undefined) {
    const powerValue = rawDps["105"];
    if (powerValue === 0) {
      poweredBy = "AC Power";
    } else if (powerValue === 1) {
      poweredBy = "Battery";
    } else {
      poweredBy = `Mode ${powerValue}`;
    }
  }

  return {
    feeding: {
      manual_feed_enabled: rawDps["102"] ?? true, // DPS 102: Manual feed switch
      last_feed_size: feedSize, // DPS 101: Feed size distributed
      last_feed_report: rawDps["15"] ?? 0, // DPS 15: Portions distributed
      quick_feed_available: rawDps["2"] ?? false, // DPS 2: Quick feeding trigger
    },
    settings: {
      sound_enabled: rawDps["103"] ?? true, // DPS 103: Sound switch
      alexa_feed_enabled: rawDps["106"] ?? false, // DPS 106: Feed by Alexa switch
    },
    system: {
      fault_status: Boolean(rawDps["14"]), // DPS 14: Fault alarm (1 = fault, 0 = ok)
      powered_by: poweredBy, // DPS 105: Powered by (0 = AC, 1 = Battery ?)
      ip_address: rawDps["107"] ?? "Unknown", // DPS 107: IP address
    },
    history,
  };
}
