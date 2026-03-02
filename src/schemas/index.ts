import { t } from "elysia";

// 🍽️ Feeder Schemas
export const FeedRequestSchema = t.Object({
  portion: t.Optional(
    t.Number({
      minimum: 1,
      maximum: 10,
      description: "Number of portions to feed (1-10)",
      default: 1,
    })
  ),
});

export const MealPlanSchema = t.Object({
  meal_plan: t.Array(
    t.Object({
      days_of_week: t.Array(t.String(), {
        description: "Days of the week for this meal",
        default: [
          "Monday",
          "Tuesday",
          "Wednesday",
          "Thursday",
          "Friday",
          "Saturday",
          "Sunday",
        ],
      }),
      time: t.String({
        pattern: "^([0-1]?[0-9]|2[0-3]):[0-5][0-9]$",
        description: "Time in HH:MM format (24h)",
        default: "08:00",
      }),
      portion: t.Number({
        minimum: 1,
        maximum: 10,
        description: "Number of portions for this meal",
        default: 1,
      }),
      status: t.Union([t.Literal("Enabled"), t.Literal("Disabled")], {
        description: "Whether this meal is enabled or disabled",
        default: "Enabled",
      }),
    }),
    { description: "Array of scheduled meals" }
  ),
});

// 🚽 Litter Box Schemas
export const LitterBoxSettingsSchema = t.Object({
  clean_delay: t.Optional(
    t.Number({
      minimum: 60,
      maximum: 1800,
      description: "Delay in seconds before cleaning (60-1800)",
      default: 120,
    })
  ),
  sleep_mode: t.Optional(
    t.Object({
      enabled: t.Optional(
        t.Boolean({ description: "Enable/disable sleep mode", default: false })
      ),
      start_time: t.Optional(
        t.String({
          pattern: "^([0-1]?[0-9]|2[0-3]):[0-5][0-9]$",
          description: "Start time in HH:MM format",
          default: "23:00",
        })
      ),
      end_time: t.Optional(
        t.String({
          pattern: "^([0-1]?[0-9]|2[0-3]):[0-5][0-9]$",
          description: "End time in HH:MM format",
          default: "07:00",
        })
      ),
    })
  ),
  preferences: t.Optional(
    t.Object({
      child_lock: t.Optional(
        t.Boolean({ description: "Enable/disable child lock", default: false })
      ),
      kitten_mode: t.Optional(
        t.Boolean({ description: "Enable/disable kitten mode", default: false })
      ),
      lighting: t.Optional(
        t.Boolean({ description: "Enable/disable lighting", default: true })
      ),
      prompt_sound: t.Optional(
        t.Boolean({
          description: "Enable/disable prompt sounds",
          default: true,
        })
      ),
      automatic_homing: t.Optional(
        t.Boolean({
          description: "Enable/disable automatic homing",
          default: true,
        })
      ),
    })
  ),
  actions: t.Optional(
    t.Object({
      reset_sand_level: t.Optional(
        t.Boolean({ description: "Reset sand level indicator", default: false })
      ),
      reset_factory_settings: t.Optional(
        t.Boolean({ description: "Reset to factory settings", default: false })
      ),
    })
  ),
});

// 📋 Response Schemas

// Base response schemas
export const BaseResponseSchema = t.Object(
  {
    success: t.Boolean({ description: "Whether the operation was successful" }),
    message: t.Optional(t.String({ description: "Response message" })),
    error: t.Optional(
      t.String({ description: "Error message if operation failed" })
    ),
  },
  { additionalProperties: true }
);

export const DeviceInfoSchema = t.Object({
  id: t.String({ description: "Device ID" }),
  name: t.String({ description: "Device name" }),
  type: t.Optional(
    t.Union(
      [
        t.Literal("feeder"),
        t.Literal("litter-box"),
        t.Literal("fountain"),
        t.Literal("unknown"),
      ],
      { description: "Device type" }
    )
  ),
  product_name: t.Optional(t.String({ description: "Product name" })),
  model: t.Optional(t.String({ description: "Device model" })),
  ip: t.Optional(t.String({ description: "Device IP address" })),
  version: t.Optional(t.String({ description: "Device version" })),
  connected: t.Optional(t.Boolean({ description: "Connection status" })),
  last_data: t.Optional(t.Any({ description: "Last received data" })),
  parsed_data: t.Optional(t.Any({ description: "Parsed device data" })),
});

// 📱 Device Management Response Schemas
export const DevicesListResponseSchema = t.Intersect([
  BaseResponseSchema,
  t.Object({
    devices: t.Optional(
      t.Array(DeviceInfoSchema, { description: "List of devices" })
    ),
    total: t.Optional(t.Number({ description: "Total number of devices" })),
  }),
]);

export const DeviceConnectionResponseSchema = BaseResponseSchema;

export const DeviceStatusResponseSchema = t.Intersect([
  BaseResponseSchema,
  t.Object({
    device: t.Optional(
      t.Object({
        id: t.String({ description: "Device ID" }),
        name: t.String({ description: "Device name" }),
        type: t.Union(
          [
            t.Literal("feeder"),
            t.Literal("litter-box"),
            t.Literal("fountain"),
            t.Literal("unknown"),
          ],
          { description: "Device type" }
        ),
        connected: t.Boolean({ description: "Connection status" }),
      })
    ),
    parsed_status: t.Optional(t.Any({ description: "Parsed device status" })),
    raw_dps: t.Optional(t.Any({ description: "Raw device DPS data" })),
  }),
]);

// 🍽️ Feeder Response Schemas
export const FeederFeedResponseSchema = t.Intersect([
  BaseResponseSchema,
  t.Object({
    device: t.Optional(
      t.Object({
        id: t.String({ description: "Device ID" }),
        name: t.String({ description: "Device name" }),
      })
    ),
  }),
]);

export const FeederStatusResponseSchema = t.Intersect([
  BaseResponseSchema,
  t.Object({
    device: t.Optional(
      t.Object({
        id: t.String({ description: "Device ID" }),
        name: t.String({ description: "Device name" }),
      })
    ),
    parsed_status: t.Optional(
      t.Object({
        feeding: t.Object({
          manual_feed_enabled: t.Any({
            description: "Manual feed enabled status",
          }),
          last_feed_size: t.Any({ description: "Last feed size description" }),
          last_feed_report: t.Any({ description: "Last feed report value" }),
          quick_feed_available: t.Any({
            description: "Quick feed availability",
          }),
        }),
        settings: t.Object({
          sound_enabled: t.Any({ description: "Sound enabled status" }),
          alexa_feed_enabled: t.Any({
            description: "Alexa feed enabled status",
          }),
        }),
        system: t.Object({
          fault_status: t.Any({ description: "Fault status" }),
          powered_by: t.Any({ description: "Power source" }),
          ip_address: t.Any({ description: "Device IP address" }),
        }),
        history: t.Any({ description: "Feed history data" }),
      })
    ),
    raw_dps: t.Optional(t.Any({ description: "Raw DPS data" })),
  }),
]);

export const MealPlanResponseSchema = t.Intersect([
  BaseResponseSchema,
  t.Object({
    device: t.Optional(
      t.Object({
        id: t.String({ description: "Device ID" }),
        name: t.String({ description: "Device name" }),
      })
    ),
    decoded: t.Optional(
      t.Union([
        t.Array(
          t.Object({
            days_of_week: t.Array(t.String()),
            time: t.String(),
            portion: t.Number(),
            status: t.String(),
          }),
          { description: "Decoded meal plan" }
        ),
        t.Null(),
      ])
    ),
    meal_plan: t.Optional(
      t.Union([t.String({ description: "Encoded meal plan" }), t.Null()])
    ),
  }),
]);

export const MealPlanUpdateResponseSchema = t.Intersect([
  BaseResponseSchema,
  t.Object({
    device: t.Optional(
      t.Object({
        id: t.String({ description: "Device ID" }),
        name: t.String({ description: "Device name" }),
      })
    ),
    encoded_base64: t.Optional(
      t.String({ description: "Encoded meal plan in Base64" })
    ),
    formatted_meal_plan: t.Optional(
      t.String({ description: "Formatted meal plan description" })
    ),
  }),
]);

// 🚽 Litter Box Response Schemas
export const LitterBoxStatusResponseSchema = t.Intersect([
  BaseResponseSchema,
  t.Object({
    device: t.Optional(
      t.Object({
        id: t.String({ description: "Device ID" }),
        name: t.String({ description: "Device name" }),
      })
    ),
    parsed_status: t.Optional(
      t.Object({
        clean_delay: t.Object({
          seconds: t.Any({ description: "Clean delay in seconds" }),
          formatted: t.String({ description: "Formatted clean delay time" }),
        }),
        sleep_mode: t.Object({
          enabled: t.Any({ description: "Sleep mode enabled status" }),
          start_time_minutes: t.Any({
            description: "Start time in minutes since midnight",
          }),
          start_time_formatted: t.String({
            description: "Formatted start time",
          }),
          end_time_minutes: t.Any({
            description: "End time in minutes since midnight",
          }),
          end_time_formatted: t.String({ description: "Formatted end time" }),
        }),
        sensors: t.Object({
          defecation_duration: t.Any({
            description: "Last defecation duration in seconds",
          }),
          defecation_frequency: t.Any({
            description: "Daily defecation count",
          }),
          fault_alarm: t.Any({ description: "Fault alarm code" }),
          litter_level: t.Any({ description: "Current litter level" }),
        }),
        system: t.Object({
          state: t.Any({ description: "Current system state" }),
          cleaning_in_progress: t.Any({
            description: "Cleaning cycle active status",
          }),
          maintenance_required: t.Any({
            description: "Maintenance required status",
          }),
        }),
        settings: t.Object({
          lighting: t.Any({ description: "Lighting enabled status" }),
          child_lock: t.Any({ description: "Child lock enabled status" }),
          prompt_sound: t.Any({ description: "Prompt sound enabled status" }),
          kitten_mode: t.Any({ description: "Kitten mode enabled status" }),
          automatic_homing: t.Any({
            description: "Automatic homing enabled status",
          }),
        }),
      })
    ),
    raw_dps: t.Optional(t.Any({ description: "Raw DPS data" })),
  }),
]);

export const LitterBoxCleanResponseSchema = t.Intersect([
  BaseResponseSchema,
  t.Object({
    device: t.Optional(
      t.Object({
        id: t.String({ description: "Device ID" }),
        name: t.String({ description: "Device name" }),
      })
    ),
  }),
]);

export const LitterBoxSettingsResponseSchema = t.Intersect([
  BaseResponseSchema,
  t.Object({
    device: t.Optional(
      t.Object({
        id: t.String({ description: "Device ID" }),
        name: t.String({ description: "Device name" }),
      })
    ),
    updated_settings: t.Optional(
      t.Number({ description: "Number of settings updated" })
    ),
  }),
]);

// 🔍 DPS Scan Response Schema
export const DpsScanResponseSchema = t.Intersect([
  BaseResponseSchema,
  t.Object({
    scan_range: t.Optional(t.String({ description: "DPS scan range" })),
    scanned_count: t.Optional(
      t.Number({ description: "Number of DPS scanned" })
    ),
    found_count: t.Optional(
      t.Number({ description: "Number of active DPS found" })
    ),
    available_dps: t.Optional(
      t.Record(
        t.String(),
        t.Object({
          value: t.Any({ description: "DPS value" }),
          type: t.String({ description: "Value type" }),
          length: t.Optional(
            t.Number({ description: "String length if applicable" })
          ),
        }),
        { description: "Available DPS data points" }
      )
    ),
    errors_count: t.Optional(
      t.Number({ description: "Number of errors encountered" })
    ),
    errors: t.Optional(
      t.Record(t.String(), t.String(), { description: "Error details" })
    ),
  }),
]);

// 📱 Device Schemas
export const ConnectDeviceSchema = t.Object({
  deviceId: t.String({ description: "Device ID to connect" }),
});

export const DisconnectDeviceSchema = t.Object({
  deviceId: t.String({ description: "Device ID to disconnect" }),
});

// 🔍 Device Debug Schemas
export const ScanDpsQuerySchema = t.Object({
  start: t.Optional(
    t.String({
      pattern: "^[0-9]+$",
      description: "Starting DPS number (default: 1)",
    })
  ),
  end: t.Optional(
    t.String({
      pattern: "^[0-9]+$",
      description: "Ending DPS number (default: 255)",
    })
  ),
  timeout: t.Optional(
    t.String({
      pattern: "^[0-9]+$",
      description: "Timeout in milliseconds per DPS (default: 3000)",
    })
  ),
});

// 💧 Fountain Schemas
export const FountainStatusResponseSchema = t.Object({
  success: t.Boolean(),
  device: t.Optional(
    t.Object({
      id: t.String(),
      name: t.String(),
    })
  ),
  parsed_status: t.Optional(t.Any()),
  message: t.Optional(t.String()),
  raw_dps: t.Optional(t.Any()),
  error: t.Optional(t.String()),
});

export const FountainResetResponseSchema = t.Object({
  success: t.Boolean(),
  message: t.Optional(t.String()),
  device: t.Optional(
    t.Object({
      id: t.String(),
      name: t.String(),
    })
  ),
  error: t.Optional(t.String()),
});

export const FountainUVSettingsSchema = t.Object({
  enabled: t.Optional(
    t.Boolean({
      description: "Enable or disable UV light",
      default: true,
    })
  ),
  runtime: t.Optional(
    t.Number({
      minimum: 0,
      maximum: 24,
      description: "UV runtime in hours (0-24)",
      default: 0,
    })
  ),
});

export const FountainUVSettingsResponseSchema = t.Object({
  success: t.Boolean(),
  message: t.Optional(t.String()),
  device: t.Optional(
    t.Object({
      id: t.String(),
      name: t.String(),
    })
  ),
  applied_settings: t.Optional(
    t.Object({
      enabled: t.Optional(t.Boolean()),
      runtime: t.Optional(t.Number()),
    })
  ),
  error: t.Optional(t.String()),
});

export const FountainEcoModeSchema = t.Object({
  mode: t.Number({
    minimum: 1,
    maximum: 2,
    description: "Eco mode setting: 1 or 2",
    default: 1,
  }),
});

export const FountainEcoModeResponseSchema = t.Object({
  success: t.Boolean(),
  message: t.Optional(t.String()),
  device: t.Optional(
    t.Object({
      id: t.String(),
      name: t.String(),
    })
  ),
  eco_mode: t.Optional(t.Number()),
  error: t.Optional(t.String()),
});

export const FountainPowerSchema = t.Object({
  enabled: t.Boolean({
    description: "Turn light on (true) or off (false)",
    default: true,
  }),
});

export const FountainPowerResponseSchema = t.Object({
  success: t.Boolean(),
  message: t.Optional(t.String()),
  device: t.Optional(
    t.Object({
      id: t.String(),
      name: t.String(),
    })
  ),
  power: t.Optional(t.Boolean()),
  error: t.Optional(t.String()),
});

// 💡 Hue Lamp Schemas

export const HueLampStateSchema = t.Object({
  isOn: t.Boolean({
    description: "Turn lamp on (true) or off (false)",
  }),
  brightness: t.Optional(
    t.Number({
      minimum: 1,
      maximum: 100,
      description: "Brightness percentage (1-100)",
    })
  ),
});

export const HueLampPowerSchema = t.Object({
  enabled: t.Boolean({
    description: "Turn lamp on (true) or off (false)",
  }),
});

export const HueLampBrightnessSchema = t.Object({
  brightness: t.Number({
    minimum: 1,
    maximum: 100,
    description: "Brightness percentage (1-100)",
    default: 100,
  }),
});

export const HueLampTemperatureSchema = t.Object({
  temperature: t.Number({
    minimum: 0,
    maximum: 100,
    description: "Color temperature percentage (0=warm/yellow, 100=cool/white)",
    default: 50,
  }),
});

export const HueLampRenameSchema = t.Object({
  name: t.String({
    minLength: 1,
    maxLength: 32,
    description: "New name for the lamp",
  }),
});

const HueLampInfoSchema = t.Object({
  id: t.String({ description: "Lamp unique ID" }),
  name: t.String({ description: "Lamp name" }),
  address: t.String({ description: "Bluetooth address" }),
  model: t.Nullable(t.String({ description: "Lamp model" })),
  manufacturer: t.String({ description: "Manufacturer name" }),
  firmware: t.Nullable(t.String({ description: "Firmware version" })),
  connected: t.Boolean({ description: "Connection status" }),
  connecting: t.Boolean({ description: "Connecting in progress" }),
  reachable: t.Boolean({ description: "Lamp is reachable" }),
  state: t.Object({
    isOn: t.Boolean({ description: "Power state" }),
    brightness: t.Number({ description: "Brightness percentage (1-100)" }),
    temperature: t.Nullable(
      t.Number({ description: "Color temperature percentage" })
    ),
    temperatureMin: t.Nullable(
      t.Number({ description: "Minimum temperature the lamp supports" })
    ),
    temperatureMax: t.Nullable(
      t.Number({ description: "Maximum temperature the lamp supports" })
    ),
  }),
  lastSeen: t.Nullable(t.String({ description: "Last seen timestamp" })),
});

export const HueLampsListResponseSchema = t.Object({
  success: t.Boolean(),
  lamps: t.Array(HueLampInfoSchema),
  total: t.Number({ description: "Total number of lamps" }),
  connected: t.Number({ description: "Number of connected lamps" }),
  reachable: t.Number({ description: "Number of reachable lamps" }),
  message: t.String(),
});

export const HueLampStatusResponseSchema = t.Object({
  success: t.Boolean(),
  lamp: t.Optional(HueLampInfoSchema),
  message: t.Optional(t.String()),
  error: t.Optional(t.String()),
});

export const HueLampResponseSchema = t.Object({
  success: t.Boolean(),
  state: t.Optional(
    t.Object({
      isOn: t.Boolean(),
      brightness: t.Number(),
    })
  ),
  message: t.Optional(t.String()),
  error: t.Optional(t.String()),
});

// 🔌 Meross Smart Plug Schemas

export const MerossToggleSchema = t.Object({
  on: t.Boolean({
    description: "Turn plug on (true) or off (false)",
    default: true,
  }),
});

export const MerossDNDModeSchema = t.Object({
  enabled: t.Boolean({
    description: "Enable DND mode (LED off) or disable (LED on)",
    default: false,
  }),
});

export const MerossProvisionKeySchema = t.Object({
  key: t.String({
    description: "Pre-shared signing key for the device",
  }),
  userId: t.String({
    description: "User ID for MQTT authentication",
    default: "1",
  }),
  mqttHost: t.Optional(
    t.String({
      description: "MQTT broker hostname",
      default: "localhost",
    }),
  ),
  mqttPort: t.Optional(
    t.Number({
      description: "MQTT broker port",
      default: 8883,
    }),
  ),
  deviceIp: t.Optional(
    t.String({
      description: "Device IP (default: 10.10.10.1 when on device AP)",
      default: "10.10.10.1",
    }),
  ),
});

export const MerossProvisionWifiSchema = t.Object({
  ssid: t.String({
    description: "WiFi network SSID",
  }),
  password: t.String({
    description: "WiFi network password",
  }),
  bssid: t.String({
    description: "WiFi network BSSID (MAC address of AP)",
  }),
  channel: t.Number({
    description: "WiFi channel number",
  }),
  encryption: t.Optional(
    t.Number({
      description: "WiFi encryption type (default: 6 = WPA2)",
      default: 6,
    }),
  ),
  cipher: t.Optional(
    t.Number({
      description: "WiFi cipher type (default: 3 = AES)",
      default: 3,
    }),
  ),
  deviceIp: t.Optional(
    t.String({
      description: "Device IP (default: 10.10.10.1 when on device AP)",
      default: "10.10.10.1",
    }),
  ),
});
