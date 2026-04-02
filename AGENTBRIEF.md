Comprehensive Project Architecture Summary
1. Project Identity
Name: "Maison" (internally cat-monitor)
Purpose: A self-hosted home automation dashboard for monitoring and controlling smart home devices -- with a particular focus on cat-related devices (feeders, fountains, litter boxes), smart lamps, smart plugs, IR climate control, and French energy tariff tracking (Tempo/RTE).
---
2. Overall Directory Structure
cat-monitor/
├── backend/                    # Rust backend (Axum web framework)
│   ├── Cargo.toml
│   ├── src/
│   │   ├── main.rs             # Entrypoint: tokio runtime, Axum server, graceful shutdown
│   │   ├── lib.rs              # AppState, app builder, module declarations
│   │   ├── config.rs           # Config struct, all env var parsing
│   │   ├── error.rs            # AppError enum (thiserror), IntoResponse impl
│   │   ├── auth.rs             # JWT claims, token decode, AuthenticatedUser extractor
│   │   ├── tempo.rs            # Tempo energy service (~1600 lines, largest module)
│   │   ├── tuya.rs             # Tuya local device protocol (feeder/fountain/litter-box)
│   │   ├── meross.rs           # Meross smart plug MQTT protocol
│   │   ├── hue.rs              # Philips Hue BLE lamp control (btleplug)
│   │   ├── hue_stub.rs         # No-op Hue stub when bluetooth feature disabled
│   │   ├── broadlink.rs        # Broadlink IR blaster (rbroadlink)
│   │   ├── mitsubishi_ir.rs    # Mitsubishi AC IR code encoder
│   │   ├── zigbee.rs           # Zigbee lamp manager (MQTT or native backend)
│   │   ├── zigbee_native.rs    # Native EZSP/EmberZNet Zigbee stack (serial)
│   │   ├── routes/
│   │   │   ├── mod.rs          # Route module declarations
│   │   │   ├── root.rs         # GET /api/ and GET /health
│   │   │   ├── auth.rs         # POST /api/auth/{login,verify,logout}
│   │   │   ├── devices.rs      # /api/devices/* -- Tuya devices (feeder, fountain, litter-box)
│   │   │   ├── hue.rs          # /api/hue-lamps/* -- Philips Hue BLE
│   │   │   ├── zigbee.rs       # /api/zigbee/* -- Zigbee lamps
│   │   │   ├── meross.rs       # /api/meross/* -- Meross smart plugs
│   │   │   ├── broadlink.rs    # /api/broadlink/* -- IR blaster and Mitsubishi AC
│   │   │   └── tempo.rs        # /api/tempo/* -- Energy tariff tracking and prediction
│   │   └── bin/
│   │       ├── hash_password.rs       # CLI: generate argon2id password hashes
│   │       ├── recalibrate_tempo.rs   # CLI: offline Tempo calibration
│   │       ├── tuya_probe.rs          # CLI: Tuya device debug probe
│   │       ├── zigbee_probe.rs        # CLI: Zigbee network probe
│   │       ├── zigbee_raw_probe.rs    # CLI: raw serial Zigbee probe
│   │       └── decode_mitsubishi_ir.rs # CLI: decode Mitsubishi IR packets
│   └── tests/
│       ├── tempo_regression.rs        # Integration tests for Tempo endpoints
│       ├── meross_regression.rs       # Integration tests for Meross
│       ├── tuya_regression.rs         # Integration tests for Tuya
│       ├── broadlink_manager.rs       # Integration tests for Broadlink
│       ├── auth_regression.rs         # Integration tests for auth
│       └── fixtures/                  # Test fixture data (tuya/, tempo/)
├── frontend/                   # React/Vite/TypeScript frontend
│   ├── package.json
│   ├── vite.config.ts
│   ├── index.html
│   └── src/
│       ├── main.tsx            # Root mount with QueryClientProvider, AuthProvider, i18n
│       ├── App.tsx             # React Router routes
│       ├── lib/
│       │   ├── api.ts          # Typed API client (all backend endpoints)
│       │   ├── utils.ts        # Tailwind merge utilities
│       │   └── pwa.ts          # PWA registration
│       ├── contexts/AuthContext.tsx
│       ├── hooks/use-toast.ts
│       ├── i18n/               # Internationalization (en + fr)
│       │   ├── index.ts
│       │   └── locales/{en,fr}.json
│       ├── pages/
│       │   ├── DashboardPage.tsx      # Main dashboard (all device types + Tempo)
│       │   ├── DevicePage.tsx         # Single Tuya device detail
│       │   ├── HueLampPage.tsx        # Single Hue lamp detail
│       │   ├── ZigbeeLampPage.tsx     # Single Zigbee lamp detail
│       │   ├── MerossPlugPage.tsx     # Single Meross plug detail
│       │   ├── TempoPredictionPage.tsx # Tempo predictions + calendar
│       │   └── LoginPage.tsx
│       └── components/
│           ├── Layout.tsx
│           ├── LanguageSwitcher.tsx
│           ├── dashboard/DashboardSectionHeader.tsx
│           ├── devices/
│           │   ├── TempoCard.tsx               # Dashboard Tempo widget
│           │   ├── TempoCalendar.tsx            # Full season calendar view
│           │   ├── FeederControl.tsx
│           │   ├── FountainControl.tsx
│           │   ├── LitterBoxControl.tsx
│           │   ├── MealPlanManager.tsx
│           │   ├── HueLampControl.tsx
│           │   ├── ZigbeeLampControl.tsx
│           │   ├── MerossPlugControl.tsx
│           │   └── BroadlinkClimateControl.tsx
│           └── ui/                              # shadcn/ui components
├── cache/tempo/                # Cached Tempo data (history, calibration, forecasts)
├── deploy/                     # Deployment configs (systemd, OpenRC, mosquitto)
├── docs/                       # Documentation (RPi1, Zigbee2MQTT, tempo calibration, Alpine)
├── scripts/                    # Shell helpers (cross-build, IR capture, headless flash)
├── mosquitto/                  # Mosquitto MQTT broker config
├── zigbee2mqtt/                # Zigbee2MQTT config (legacy/optional path)
├── vendor/                     # Vendored dependencies (if any)
├── docker-compose.yml          # Frontend + Mosquitto + optional Cloudflare tunnel
├── Makefile                    # Full lifecycle: backend, frontend, zigbee2mqtt, compose, deploy
├── deploy.sh                   # One-shot RPi1 deployment script
├── *.json                      # Runtime config files read by backend from repo root
└── .env / .env.example         # Environment configuration
---
3. Technology Stack
Backend (Rust)
Layer	Technology	Details
Runtime	Tokio	Multi-threaded async runtime
Web framework	Axum 0.8	Typed extractors, Router nesting, State sharing
Auth	JWT (jsonwebtoken) + Argon2	HttpOnly cookie sessions, rate-limited login
HTTP client	reqwest (rustls)	External API calls (RTE, Open-Meteo, data.gouv.fr)
MQTT	rumqttc	Meross plug protocol + Zigbee2MQTT bridge
Tuya protocol	rust-async-tuyapi (custom fork)	Direct LAN TCP to Tuya devices
Hue BLE	btleplug (optional bluetooth feature) + silizium	Bluetooth Low Energy lamp control
Zigbee native	ashv2 + ezsp (custom forks)	EmberZNet/EZSP serial protocol for Sonoff dongles
Broadlink IR	rbroadlink	Broadlink RM device discovery, learning, sending
Serialization	serde + serde_json	Throughout
Error handling	thiserror + custom AppError	Maps to HTTP status codes
Tracing	tracing + tracing-subscriber	Structured logging with env-filter
Static serving	tower-http (ServeDir)	Backend serves built frontend from frontend/dist
Frontend (TypeScript/React)
Layer	Technology
Framework	React 19 + React Router 7
Build	Vite 8 + Bun
Styling	Tailwind CSS 4 + shadcn/ui (Radix primitives)
State/data	TanStack React Query 5
i18n	i18next (English + French)
Linting	oxlint + oxfmt
PWA	manifest.json + registration
Infrastructure
Component	Technology
MQTT broker	Eclipse Mosquitto 2 (Docker or host-native)
Tunnel	Cloudflare Tunnel (optional Docker or host systemd)
Target deployment	Raspberry Pi 1 (Alpine Linux, OpenRC, musl)
Cross-compilation	zigbuild for arm-unknown-linux-musleabi
Database
There is no database. All persistent state is JSON files on disk:
- users.json -- user accounts with argon2 hashes
- devices.json / device-cache.json -- Tuya device registry and connection cache
- meross-devices.json -- Meross plug configuration
- hue-lamps.json / hue-lamps-blacklist.json -- Hue lamp registry
- zigbee-lamps.json -- Zigbee lamp registry
- broadlink-codes.json -- Learned IR codes
- cache/tempo/*.json -- Tempo history, calibration params, temperature data
---
4. Module / API Organization
The backend follows a strict two-layer architecture per domain:
1. Service module (backend/src/<module>.rs) -- Contains the Manager or Service struct, business logic, external protocol implementation, and data types.
2. Route module (backend/src/routes/<module>.rs) -- Contains the Axum router function, request/response types, and handler functions that delegate to the service.
Each service is instantiated once in build_app_parts_from_config() (in lib.rs) and stored as a field on AppState. Routes receive state through Axum's State extractor.
API route tree (all under /api):
Prefix	Route module	Service module	Purpose
/	root	--	Health check, API info
/auth	auth	auth	Login, verify, logout
/devices	devices	tuya	Tuya devices (feeder, fountain, litter-box)
/hue-lamps	hue	hue / hue_stub	Philips Hue BLE lamps
/zigbee	zigbee	zigbee + zigbee_native	Zigbee lamps (MQTT or native EZSP)
/meross	meross	meross	Meross smart plugs via MQTT
/broadlink	broadlink	broadlink + mitsubishi_ir	IR blaster, Mitsubishi AC
/tempo	tempo	tempo	French energy tariff tracking/prediction
All routes are authenticated (except /health and the root info endpoint). Authentication is enforced by the AuthenticatedUser extractor, which reads the JWT from an HttpOnly cookie or Authorization header.
---
5. Hardware Integrations
Integration	Protocol	Hardware
Tuya	Local TCP (Tuya protocol 3.x)	Cat feeder, water fountain, litter box
Philips Hue	Bluetooth Low Energy (GATT via btleplug + silizium)	Hue lamps
Zigbee (native)	Serial EZSP/EmberZNet (ashv2/ezsp crates)	Sonoff Zigbee 3.0 USB Dongle (MG21)
Zigbee (MQTT)	MQTT via Zigbee2MQTT bridge	Any Zigbee lamp via Z2M
Meross	MQTT (local broker)	Meross MSS310 smart plugs
Broadlink	UDP LAN discovery + IR send/learn (rbroadlink)	Broadlink RM/RM Mini IR blasters
Mitsubishi AC	IR code generation (programmatic encoding)	Mitsubishi MSZ-HJ5VA (via Broadlink)
The bluetooth Cargo feature gate allows compiling without btleplug for targets that lack Bluetooth (e.g., Raspberry Pi 1 with --no-default-features).
---
6. Tempo / Energy Functionality (Detailed)
The Tempo module is the most substantial single module (~1600 lines in tempo.rs). It implements a complete French RTE Tempo energy tariff tracker and predictor.
What "Tempo" is: RTE (French grid operator) assigns each day a color (BLUE=cheap, WHITE=medium, RED=expensive) affecting electricity prices. The system:
1. Fetches live data from the RTE public API (services-rte.com) -- today's and tomorrow's color.
2. Fetches tariff prices from data.gouv.fr -- current prices per kWh for each color in HC/HP mode.
3. Tracks history per season (Sep 1 to Aug 31) -- cached in cache/tempo/tempo_history_*.json. Multiple seasons available (2018-2026).
4. Predicts future colors using a calibrated consumption model:
   - Estimates national electricity consumption from temperature (Open-Meteo APII now have a comprehensive understanding of this codebase. Here is the full summary.
---
Comprehensive Architecture Summary: cat-monitor ("Maison")
1. Project Overview
This is a home automation dashboard called "Maison" that monitors and controls local smart devices (with a particular focus on cat-care devices like feeders, litter boxes, and fountains). It also manages lighting (Hue, Zigbee), smart plugs (Meross), IR climate control (Broadlink/Mitsubishi), and French electricity Tempo pricing/predictions. It is designed to run on modest hardware like a Raspberry Pi 1.
---
2. Top-Level Directory Structure
/Users/leonard/Github/cat-monitor/
|-- backend/                    # Rust backend (Axum web framework)
|-- frontend/                   # React + Vite + TypeScript frontend
|-- cache/
|   `-- tempo/                  # Tempo energy pricing history & calibration data
|-- deploy/
|   |-- mosquitto/              # Mosquitto deployment configs
|   |-- openrc/                 # Alpine Linux OpenRC service files
|   `-- systemd/                # systemd service files (Pi deployment)
|-- docs/                       # Documentation (Pi setup, Zigbee, Tempo calibration)
|-- mosquitto/                  # MQTT broker config (Mosquitto)
|-- scripts/                    # Helper scripts (Pi build, IR capture, Alpine flash)
|-- screenshots/                # UI screenshots
|-- vendor/                     # Vendored dependencies
|-- zigbee2mqtt/                # Zigbee2MQTT configuration
|-- docker-compose.yml          # Docker services (frontend, MQTT, Cloudflare tunnel)
|-- Makefile                    # Full lifecycle management (start/stop/build/deploy)
|-- package.json                # Root workspace convenience scripts
|-- *.json                      # Runtime config files (devices, users, lamps, codes...)
|-- .env / .env.example         # Environment configuration
|-- deploy.sh                   # One-shot deployment helper for Raspberry Pi
`-- README.md
---
3. Technology Stack
Backend (Rust)
Component	Technology
Language	Rust 2021 edition
Web framework	Axum 0.8
Async runtime	Tokio (multi-thread)
HTTP client	Reqwest 0.13 (rustls)
Auth	Argon2 password hashing, JWT (jsonwebtoken), HttpOnly cookies
MQTT	rumqttc 0.24 (for Meross plugs, Zigbee2MQTT)
Zigbee (native)	EZSP protocol via ezsp/ashv2 crates (Sonoff MG21 dongle), silizium for ZCL
Zigbee (MQTT)	Zigbee2MQTT compatibility mode
Bluetooth	btleplug 0.12 (optional, for Hue BLE lamps)
Tuya devices	rust-async-tuyapi (local protocol)
Broadlink IR	rbroadlink 0.4
Serial	tokio-serial 5.4
Serialization	serde + serde_json
Error handling	thiserror
Tracing/Logging	tracing + tracing-subscriber with env-filter
Chrono	chrono with clock and serde features, chrono-tz
Frontend
Component	Technology
Framework	React 19
Build tool	Vite 8
Language	TypeScript 5.9
Styling	Tailwind CSS 4
UI components	Radix UI primitives (dialog, select, switch, slider, tabs, toast, tooltip, etc.)
State management	TanStack React Query 5
Routing	React Router DOM 7
i18n	i18next + react-i18next (English + French)
Icons	Lucide React
Runtime	Bun (for devI now have a complete picture of the codebase. Here is the comprehensive summary.
---
## Comprehensive Architecture Summary: `cat-monitor` ("Maison")
### 1. Project Overview
**Maison** is a self-hosted smart home monitoring and control system. It is focused primarily on cat-related devices (feeders, fountains, litter boxes) but extends to general home automation: smart plugs, smart lamps (Hue/Zigbee), IR climate control (Broadlink/Mitsubishi AC), and French electricity Tempo tariff tracking/prediction.
---
2. Top-Level Directory Structure
cat-monitor/
├── backend/                 # Rust backend (Axum web framework)
│   ├── Cargo.toml
│   ├── src/
│   │   ├── main.rs          # Entrypoint: Tokio runtime, Axum server, graceful shutdown
│   │   ├── lib.rs           # AppState, app builder, module exports
│   │   ├── config.rs        # Config struct (all env-var-driven)
│   │   ├── error.rs         # Unified AppError type (thiserror + Axum IntoResponse)
│   │   ├── auth.rs          # JWT claims, token decode, AuthenticatedUser extractor
│   │   ├── broadlink.rs     # Broadlink IR device manager
│   │   ├── hue.rs           # Philips Hue BLE lamp manager (feature-gated on "bluetooth")
│   │   ├── hue_stub.rs      # No-op Hue stub when Bluetooth feature is disabled
│   │   ├── meross.rs        # Meross smart plug manager (MQTT-based)
│   │   ├── mitsubishi_ir.rs # Mitsubishi AC IR code generation/decoding
│   │   ├── tempo.rs         # Tempo energy service (RTE API, predictions, calibration)
│   │   ├── tuya.rs          # Tuya local device manager (feeders, litter boxes, fountains)
│   │   ├── zigbee.rs        # Zigbee lamp manager (MQTT backend for Zigbee2MQTT)
│   │   ├── zigbee_native.rs # Native Zigbee backend (EZSP/EmberZNet via serial dongle)
│   │   ├── routes/          # HTTP route modules (one per domain)
│   │   │   ├── mod.rs
│   │   │   ├── root.rs      # GET / and /health
│   │   │   ├── auth.rs      # POST /auth/login|verify|logout
│   │   │   ├── broadlink.rs # /broadlink/* routes
│   │   │   ├── devices.rs   # /devices/* routes (Tuya-I now have a thorough understanding of the entire codebase. Here is the comprehensive summary.
---
## Project Architecture: "Maison" (cat-monitor)
### 1. Overview
**Maison** is a self-hosted smart home control panel designed primarily around cat-care IoT devices (feeders, litter boxes, fountains) but extended to cover lighting (Hue/Zigbee), smart plugs (Meross), IR climate control (Broadlink/Mitsubishi), and French electricity tariff tracking (RTE Tempo). It is a two-component monorepo with a Rust backend and a React/TypeScript frontend, targeting both desktop Linux/macOS development and Raspberry Pi 1 (Alpine/musl) production deployments.
---
### 2. Directory Structure
cat-monitor/                         # Repository root ("source root" at runtime)
├── backend/                         # Rust backend (Axum web server)
│   ├── Cargo.toml                   # Crate manifest, deps, features
│   ├── src/
│   │   ├── main.rs                  # Entry point: Axum + Tokio server
│   │   ├── lib.rs                   # AppState, app builder, module re-exports
│   │   ├── config.rs                # Config from env vars
│   │   ├── error.rs                 # AppError enum (thiserror)
│   │   ├── auth.rs                  # JWT Claims, token extraction, AuthenticatedUser guard
│   │   ├── tuya.rs                  # Tuya device manager (feeders, litter boxes, fountains)
│   │   ├── meross.rs                # Meross smart plug manager (MQTT-based)
│   │   ├── hue.rs                   # Hue BLE manager (requires "bluetooth" feature)
│   │   ├── hue_stub.rs              # No-op Hue manager (when "bluetooth" feature disabled)
│   │   ├── zigbee.rs                # Zigbee lamp manager (MQTT or native backends)
│   │   ├── zigbee_native.rs         # Native EZSP/EmberZNet Zigbee via serial USB dongle
│   │   ├── broadlink.rs             # Broadlink IR blaster manager
│   │   ├── mitsubishi_ir.rs         # Mitsubishi AC IR protocol encoder
│   │   ├── tempo.rs                 # RTE Tempo energy tariff service (~1600 LOC)
│   │   ├── routes/                  # Axum route modules
│   │   │   ├── mod.rs               # Route module declarations
│   │   │   ├── root.rs              # GET /, /health
│   │   │   ├── auth.rs              # POST /auth/login, /verify, /logout
│   │   │   ├── devices.rs           # Tuya device CRUD + feeder/litter/fountain endpoints
│   │   │   ├── hue.rs               # Hue lamp CRUD
│   │   │   ├── zigbee.rs            # Zigbee lamp CRUD + pairing
│   │   │   ├── meross.rs            # Meross plug status/toggle/electricity/consumption
│   │   │   ├── broadlink.rs         # Broadlink discover/learn/send/codes + Mitsubishi
│   │   │   └── tempo.rs             # Tempo data/refresh/predictions/state/calendar/history/calibration
│   │   └── bin/                     # Standalone utility binaries
│   │       ├── hash_password.rs     # Argon2 password hasher for users.json
│   │       ├── recalibrate_tempo.rs # CLI tool to rebuild Tempo calibration
│   │       ├── tuya_probe.rs        # Tuya device probe
│   │       ├── zigbee_probe.rs      # Zigbee network probe
│   │       ├── zigbee_raw_probe.rs  # Raw Zigbee serial probe
│   │       └── decode_mitsubishi_ir.rs # Mitsubishi IR packet decoder
│   └── tests/
│       ├── tempo_regression.rs      # Integration tests for Tempo API
│       ├── meross_regression.rs     # Integration tests for Meross
│       ├── tuya_regression.rs       # Integration tests for Tuya
│       ├── broadlink_manager.rs     # Integration tests for Broadlink
│       ├── auth_regression.rs       # Integration tests for auth
│       └── fixtures/                # Test fixture data (tuya/, tempo/)
├── frontend/                        # React/TypeScript SPA
│   ├── package.json                 # Bun/Vite project
│   ├── vite.config.ts
│   ├── tsconfig.json
│   ├── index.html
│   ├── public/manifest.json         # PWA manifest
│   └── src/
│       ├── main.tsx                 # React entry
│       ├── App.tsx                  # React Router routes
│       ├── index.css                # TailwindCSS entry
│       ├── lib/api.ts              # Typed API client (all backend endpoints)
│       ├── lib/utils.ts
│       ├── lib/pwa.ts
│       ├── contexts/AuthContext.tsx  # JWT auth context
│       ├── hooks/use-toast.ts
│       ├── i18n/                    # i18next (English + French)
│       │   ├── index.ts
│       │   └── locales/{en,fr}.json
│       ├── pages/
│       │   ├── DashboardPage.tsx    # Main dashboard (all device sections + Tempo)
│       │   ├── DevicePage.tsx       # Individual Tuya device page
│       │   ├── HueLampPage.tsx
│       │   ├── ZigbeeLampPage.tsx
│       │   ├── MerossPlugPage.tsx
│       │   ├── TempoPredictionPage.tsx
│       │   └── LoginPage.tsx
│       └── components/
│           ├── Layout.tsx
│           ├── LanguageSwitcher.tsx
│           ├── dashboard/DashboardSectionHeader.tsx
│           ├── devices/             # Per-device-type control widgets
│           │   ├── FeederControl.tsx
│           │   ├── FountainControl.tsx
│           │   ├── LitterBoxControl.tsx
│           │   ├── HueLampControl.tsx
│           │   ├── ZigbeeLampControl.tsx
│           │   ├── MerossPlugControl.tsx
│           │   ├── BroadlinkClimateControl.tsx
│           │   ├── TempoCard.tsx
│           │   ├── TempoCalendar.tsx
│           │   └── MealPlanManager.tsx
│           └── ui/                  # shadcn/ui primitives (Radix + Tailwind)
├── cache/tempo/                     # Persisted Tempo data
│   ├── calibration_params.json      # Calibrated model parameters
│   ├── temperature_history.json     # Historical temperature cache
│   ├── temp_forecast.json           # Temperature forecast cache
│   └── tempo_history_*.json         # RTE Tempo season history (2018-2026)
├── deploy/                          # Deployment configs
│   ├── systemd/                     # systemd unit files for Pi
│   ├── openrc/                      # OpenRC init scripts for Alpine
│   └── mosquitto/                   # Mosquitto broker config for Pi
├── docs/                            # Documentation
│   ├── raspberry-pi-1.md
│   ├── alpine-headless-flash-macos.md
│   ├── zigbee2mqtt-host-setup.md
│   └── tempo-calibration.md
├── scripts/                         # Helper scripts
│   ├── build-rpi1-backend.sh        # Cross-compile for Raspberry Pi 1
│   ├── flash-alpine-headless-macos.sh
│   └── capture-mitsubishi-ir.sh
├── mosquitto/                       # Mosquitto runtime config/data
├── zigbee2mqtt/                     # Zigbee2MQTT config
├── vendor/                          # Vendored dependencies
├── docker-compose.yml               # Frontend + Mosquitto + optional Cloudflare tunnel
├── Makefile                         # Lifecycle: backend, frontend, start, stop, status
├── deploy.sh                        # One-shot Pi deployment helper
├── package.json                     # Root convenience scripts
├── .env.example                     # All environment variables documented
├── devices.json                     # Tuya device definitions
├── device-cache.json                # Tuya device state cache
├── meross-devices.json              # Meross device definitions
├── hue-lamps.json                   # Known Hue lamps
├── hue-lamps-blacklist.json         # Blacklisted Hue lamps
├── zigbee-lamps.json                # Known Zigbee lamps
├── broadlink-codes.json             # Saved Broadlink IR codes
├── users.json                       # User accounts (Argon2 hashed passwords)
└── users.json.template              # Template for users.json
---
### 3. Technology Stack
| Layer | Technology |
|-------|-----------|
| **Backend runtime** | Rust 2021 edition, Tokio async runtime |
| **Web framework** | Axum 0.8 with tower-http (CORS, static files, tracing) |
| **Authentication** | JWT (jsonwebtoken crate) + Argon2 password hashing, HttpOnly cookies, rate limiting |
| **Tuya IoT devices** | `rust-async-tuyapi` (custom fork) -- local LAN protocol |
| **Meross smart plugs** | MQTT via `rumqttc` -- talks to local Eclipse Mosquitto broker |
| **Hue lamps** | Bluetooth Low Energy via `btleplug` (optional, feature-gated) |
| **Zigbee lamps** | Dual backend: native EZSP over serial (`ashv2`/`ezsp`/`silizium` crates) OR Zigbee2MQTT via MQTT |
| **Broadlink IR** | `rbroadlink` crate -- LAN discovery, learning, IR sending |
| **Mitsubishi AC** | Custom IR protocol encoder (`mitsubishi_ir.rs`) |
| **Tempo (energy)** | HTTP client (`reqwest`) to RTE APIs, Open-Meteo APIs, data.gouv.fr; local calibration engine |
| **Frontend framework** | React 19, TypeScript 5.9, Vite 8 |
| **Frontend UI** | TailwindCSS 4, shadcn/ui (Radix primitives), Lucide icons |
| **State management** | TanStack React Query |
| **Routing** | React Router 7 |
| **i18n** | i18next (English + French) |
| **PWA** | manifest.json, service worker registration |
| **Containerization** | Docker Compose (frontend Nginx, Mosquitto, optional Cloudflare tunnel) |
| **MQTT broker** | Eclipse Mosquitto 2 |
| **Database** | **None** -- all state is JSON files on disk |
| **CI/Validation** | `cargo test`, `bun run build`, `oxlint`, `oxfmt` |
---
### 4. Module / API Organization
#### Backend Module Pattern
Each hardware/service integration follows a consistent two-file pattern:
1. **Service module** (`backend/src/<module>.rs`) -- Contains the manager struct, business logic, and hardware/network communication. The manager is typically `Clone` (wrapping `Arc<RwLock<...>>` internals) and constructed in `build_app_parts_from_config()`.
2. **Route module** (`backend/src/routes/<module>.rs`) -- Contains:
   - A `pub fn router() -> Router<AppState>` function that defines all HTTP endpoints
   - Request/response DTOs (private structs with `Serialize`/`Deserialize`)
   - Handler functions that extract `State<AppState>`, `AuthenticatedUser`, path params, and request body
   - All handlers require authentication via the `AuthenticatedUser` extractor
#### API Route Tree
/health                              # Health check (unauthenticated)
/api/                                # Root info
/api/health                          # API health
/api/auth/login                      # POST - JWT login
/api/auth/verify                     # POST - Token verification
/api/auth/logout                     # POST - Cookie-based logout
/api/devices/                        # GET - List Tuya devices
/api/devices/stats                   # GET - Connection stats
/api/devices/connect                 # POST - Connect all
/api/devices/disconnect              # POST - Disconnect all
/api/devices/{id}/connect            # GET - Connect one
/api/devices/{id}/disconnect         # GET - Disconnect one
/api/devices/{id}/status             # GET - Raw status
/api/devices/{id}/scan-dps           # GET - DPS scan
/api/devices/{id}/feeder/*           # Feeder-specific (feed, status, meal-plan)
/api/devices/{id}/litter-box/*       # Litter-specific (clean, settings, status)
/api/devices/{id}/fountain/*         # Fountain-specific (reset/, uv, eco-mode, power, status)
/api/hue-lamps/                      # Hue lamp CRUD, scan, connect/disconnect, power/brightness/temperature/state/rename/blacklist
/api/zigbee/lamps/                   # Zigbee lamp CRUD, pairing start/stop/status, power/brightness/temperature/rename
/api/meross/                         # Meross plug list, stats, status, electricity, consumption, toggle, on, off, dnd
/api/broadlink/                      # Broadlink discover, provision, learn/ir, send, codes, mitsubishi/
/api/tempo/                          # GET today+tomorrow colors + tarifs
/api/tempo/refresh                   # POST force-refresh
/api/tempo/predictions               # GET 7-day prediction
/api/tempo/state                     # GET season stock (red/white days remaining)
/api/tempo/calendar                  # GET full season calendar with predictions
/api/tempo/history                   # GET historical color data
/api/tempo/calibration               # GET calibration params
/api/tempo/calibration/rebuild       # POST trigger recalibration
#### AppState Composition
The `AppState` struct (in `lib.rs`) holds all managers as cloneable fields:
```rust
pub struct AppState {
    config: Arc<Config>,
    users: SharedUsers,
    auth_rate_limiter: AuthRateLimiter,
    broadlink: BroadlinkManager,
    hue: HueManager,
    meross: MerossManager,
    tempo: TempoService,
    tuya: TuyaManager,
    zigbee: ZigbeeManager,
}
---
5. Hardware / Integration Modules
Module	Hardware/Service	Protocol	Config Files
Tuya (tuya.rs)	Cat feeders, litter boxes, fountains	Local LAN Tuya protocol (encrypted TCP)	devices.json, device-cache.json
Meross (meross.rs)	MSS310 smart plugs (energy monitoring)	MQTT via local Mosquitto broker	meross-devices.json
Hue (hue.rs / hue_stub.rs)	Philips Hue lamps	Bluetooth Low Energy (btleplug)	hue-lamps.json, hue-lamps-blacklist.json
Zigbee (zigbee.rs, zigbee_native.rs)	Zigbee lamps (e.g., Paulmann)	MQTT (Zigbee2MQTT) or native EZSP/EmberZNet via serial USB (Sonoff MG21 dongle)	zigbee-lamps.json, zigbee2mqtt/configuration.yaml
Broadlink (broadlink.rs)	Broadlink RM4 IR blaster	LAN UDP (rbroadlink)	broadlink-codes.json
Mitsubishi IR (mitsubishi_ir.rs)	Mitsubishi AC units (MSZ-HJ5VA etc.)	IR via Broadlink	Encoded in Broadlink codes
Tempo (tempo.rs)	RTE French electricity Tempo tariffs	HTTPS to RTE APIs, Open-Meteo, data.gouv.fr	cache/tempo/*.json
The Bluetooth feature gate is notable: hue.rs is compiled only with features = ["bluetooth"] (the default). On Pi builds (--no-default-features), hue_stub.rs provides a no-op implementation instead.
---
6. Build System
Backend (Cargo):
- Single crate: cat-monitor-rust-backend
- Edition 2021, default-run is the main binary
- Feature flags: bluetooth (default, enables BLE Hue), live-runtime-tests
- Key dependencies: axum, tokio, reqwest, rumqttc, btleplug (optional), ashv2/ezsp/silizium (Zigbee), rbroadlink, rust-async-tuyapi (custom fork), argon2, jsonwebtoken, chrono, serde_json
- Six utility binaries in src/bin/
- Cross-compilation for Pi 1: scripts/build-rpi1-backend.sh
Frontend (Bun + Vite):
- bun --cwd frontend run dev for development (proxies /api to localhost:3033)
- bun --cwd frontend run build produces frontend/dist/ which the Rust backend can serve directly
- Linting: oxlint, formatting: oxfmt
Makefile orchestration:
- make backend / make frontend for development
- make start / make stop for full stack (backend + Docker frontend + Mosquitto + optional Zigbee2MQTT + optional Cloudflare tunnel)
- make backend-build-pi / make backend-build-pi-cross for Pi builds
---
7. Tempo / Energy Module (Detailed)
The Tempo module (backend/src/tempo.rs, ~1600 lines) is the most complex single module. It provides:
1. Real-time Tempo colors -- Fetches today/tomorrow Blue/White/Red day colors from the RTE public API
2. Tariff data -- Fetches current HC/HP prices per color from data.gouv.fr
3. Historical data -- Fetches and caches per-season Tempo history (back to 2018)
4. 7-day predictions -- A sophisticated probabilistic model that:
   - Fetches temperature forecasts from Open-Meteo
   - Estimates French national electricity consumption using calibrated parameters
   - Models Tempo constraints (stock of remaining red/white days, max consecutive red days, weekend/Sunday rules)
   - Outputs probabilities (Blue/White/Red) and confidence scores
5. Calibration engine -- Grid-search optimization over:
   - Base consumption, thermosensitivity, temperature reference, weekend factor
   - Monthly adjustments (12 parameters)
   - Red/white threshold offsets and probability scales
   - Metrics: accuracy, red recall, white recall, macro F1
   - Minimum 120 weather samples required
6. Caching strategy -- Multi-layer: in-memory RwLock<TempoCache> with TTLs + JSON files on disk in cache/tempo/
Frontend components: TempoCard.tsx (dashboard widget showing today/tomorrow colors + predictions + tariffs) and TempoCalendar.tsx / TempoPredictionPage.tsx (full calendar + detailed predictions).
---
8. How to Add a New Module
Based on the established patterns, adding a new hardware or service integration involves these steps:
1. Create the service module at backend/src/<newmodule>.rs:
   - Define a manager struct (e.g., NewModuleManager) that is Clone (using Arc<RwLock<...>>)
   - Implement a pub fn new(...) constructor that loads config/data
   - Implement async methods for the business logic
   - Keep hardware communication private, expose clean typed methods
2. Create the route module at backend/src/routes/<newmodule>.rs:
   - Define pub fn router() -> Router<AppState>
   - Add request/response structs with Serialize/Deserialize
   - Implement handler functions using State<AppState> + AuthenticatedUser
3. Register in backend/src/routes/mod.rs:
   - Add pub mod newmodule;
4. Register in backend/src/lib.rs:
   - Add pub mod newmodule; declaration
   - Add the manager field to AppState
   - Construct the manager in build_app_parts_from_config()
   - Nest the router: .nest("/newmodule", routes::newmodule::router())
   - If it needs graceful shutdown, add to AppState::shutdown()
5. Add config fields to backend/src/config.rs if new env vars or file paths are needed
6. Add frontend API client in frontend/src/lib/api.ts:
   - Define TypeScript interfaces
   - Add an API object (e.g., newModuleApi)
7. Add frontend components:
   - Control component at frontend/src/components/devices/NewModuleControl.tsx
   - Page at frontend/src/pages/NewModulePage.tsx (if standalone page needed)
   - Route in frontend/src/App.tsx
   - Dashboard section in frontend/src/pages/DashboardPage.tsx
   - i18n keys in frontend/src/i18n/locales/{en,fr}.json
8. Add integration tests at backend/tests/<newmodule>_regression.rs
9. Add any runtime JSON config files at the repo root (and document them in README.md)