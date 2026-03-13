use std::{collections::HashMap, path::PathBuf, sync::Arc};

use chrono::{Datelike, Duration, NaiveDate, Timelike, Utc, Weekday};
use chrono_tz::Europe::Paris;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use crate::error::AppError;

const RTE_PUBLIC_API: &str = "https://www.services-rte.com/cms/open_data/v1/tempo";
const RTE_LIGHT_API: &str = "https://www.services-rte.com/cms/open_data/v1/tempoLight";
const RTE_WEBPAGE_URL: &str = "https://www.services-rte.com/fr/visualisez-les-donnees-publiees-par-rte/calendrier-des-offres-de-fourniture-de-type-tempo.html";
const TARIFS_API_URL: &str = "https://tabular-api.data.gouv.fr/api/resources/0c3d1d36-c412-4620-8566-e5cbb4fa2b5a/data/?page_size=1&P_SOUSCRITE__exact=6&__id__sort=desc";
const OPEN_METEO_API: &str = "https://api.open-meteo.com/v1/forecast";
const FRANCE_LAT: f64 = 46.603354;
const FRANCE_LON: f64 = 1.888334;

const TARIFS_CACHE_SECONDS: i64 = 24 * 60 * 60;
const HISTORY_CACHE_SECONDS: i64 = 6 * 60 * 60;
const FORECAST_CACHE_SECONDS: i64 = 3 * 60 * 60;
const PREDICTIONS_CACHE_SECONDS: i64 = 30 * 60;
const STATE_CACHE_SECONDS: i64 = 60 * 60;

const STOCK_RED_DAYS: i32 = 22;
const STOCK_WHITE_DAYS: i32 = 43;
const NORMALIZATION_MEAN: f64 = 46_050.0;
const NORMALIZATION_STD: f64 = 2_160.0;
const THRESHOLD_RED_A: f64 = 3.15;
const THRESHOLD_RED_B: f64 = 0.010;
const THRESHOLD_RED_C: f64 = 0.031;
const THRESHOLD_WHITE_RED_A: f64 = 4.00;
const THRESHOLD_WHITE_RED_B: f64 = 0.015;
const THRESHOLD_WHITE_RED_C: f64 = 0.026;
const MAX_CONSECUTIVE_RED_DAYS: i32 = 5;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TempoDay {
    pub date: String,
    pub color: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TempoTarifColor {
    pub hc: f64,
    pub hp: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TempoTarifs {
    pub blue: TempoTarifColor,
    pub white: TempoTarifColor,
    pub red: TempoTarifColor,
    #[serde(rename = "dateDebut")]
    pub date_debut: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TempoData {
    pub today: TempoDay,
    pub tomorrow: TempoDay,
    pub tarifs: Option<TempoTarifs>,
    #[serde(rename = "lastUpdated")]
    pub last_updated: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TempoPrediction {
    pub date: String,
    pub predicted_color: String,
    pub probabilities: TempoProbabilities,
    pub confidence: f64,
    pub constraints: TempoConstraints,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TempoProbabilities {
    #[serde(rename = "BLUE")]
    pub blue: f64,
    #[serde(rename = "WHITE")]
    pub white: f64,
    #[serde(rename = "RED")]
    pub red: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TempoConstraints {
    pub can_be_red: bool,
    pub can_be_white: bool,
    pub is_in_red_period: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TempoState {
    pub success: bool,
    pub season: String,
    pub stock_red_remaining: i32,
    pub stock_red_total: i32,
    pub stock_white_remaining: i32,
    pub stock_white_total: i32,
    pub consecutive_red: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TempoPredictionServiceResponse {
    pub success: bool,
    pub predictions: Vec<TempoPrediction>,
    pub state: Option<TempoPredictionState>,
    pub model_version: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TempoPredictionState {
    pub season: String,
    pub stock_red_remaining: i32,
    pub stock_red_total: i32,
    pub stock_white_remaining: i32,
    pub stock_white_total: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TempoHistoryDay {
    pub date: String,
    pub color: String,
    pub is_actual: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TempoCalendarDay {
    pub date: String,
    pub color: Option<String>,
    pub is_actual: bool,
    pub is_prediction: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub probabilities: Option<TempoProbabilities>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub constraints: Option<TempoConstraints>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TempoCalendarStatistics {
    pub total_days: usize,
    pub color_counts: HashMap<String, usize>,
    pub predictions_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TempoCalendarStock {
    pub red_remaining: i32,
    pub red_total: i32,
    pub white_remaining: i32,
    pub white_total: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TempoCalendarResponse {
    pub success: bool,
    pub season: String,
    pub calendar: Vec<TempoCalendarDay>,
    pub statistics: TempoCalendarStatistics,
    pub stock: TempoCalendarStock,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TempoHistoryResponse {
    pub success: bool,
    pub season: String,
    pub history: Vec<TempoHistoryDay>,
    pub count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TempoCalibrationResponse {
    pub success: bool,
    pub calibrated: Option<bool>,
    pub params: Option<TempoCalibrationParams>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TempoCalibrationParams {
    pub base_consumption: f64,
    pub thermosensitivity: f64,
    pub temp_reference: f64,
    pub weekend_factor: f64,
    pub renewable_factor: f64,
    pub calibration_date: Option<String>,
    pub calibration_accuracy: f64,
    pub calibration_red_recall: f64,
    pub monthly_adjustments: HashMap<u32, f64>,
}

impl Default for TempoCalibrationParams {
    fn default() -> Self {
        Self {
            base_consumption: 46_050.0,
            thermosensitivity: 1_900.0,
            temp_reference: 12.0,
            weekend_factor: 0.92,
            renewable_factor: 0.12,
            calibration_date: None,
            calibration_accuracy: 0.0,
            calibration_red_recall: 0.0,
            monthly_adjustments: HashMap::from([
                (1, 0.98),
                (2, 0.97),
                (3, 0.93),
                (4, 0.88),
                (5, 0.83),
                (6, 0.80),
                (7, 0.78),
                (8, 0.80),
                (9, 0.85),
                (10, 0.90),
                (11, 0.95),
                (12, 0.97),
            ]),
        }
    }
}

#[derive(Debug, Deserialize)]
struct RawCalibrationParams {
    base_consumption: Option<f64>,
    thermosensitivity: Option<f64>,
    temp_reference: Option<f64>,
    weekend_factor: Option<f64>,
    renewable_factor: Option<f64>,
    calibration_date: Option<String>,
    calibration_accuracy: Option<f64>,
    calibration_red_recall: Option<f64>,
    monthly_adjustments: Option<HashMap<String, f64>>,
}

#[derive(Debug, Deserialize)]
struct RteTempoResponse {
    values: HashMap<String, String>,
}

#[derive(Debug, Deserialize)]
struct TarifGouvResponse {
    data: Vec<TarifGouvRow>,
}

#[derive(Debug, Deserialize)]
struct TarifGouvRow {
    #[serde(rename = "DATE_DEBUT")]
    date_debut: String,
    #[serde(rename = "DATE_FIN")]
    date_fin: Option<String>,
    #[serde(rename = "PART_VARIABLE_HCBleu_TTC")]
    blue_hc: String,
    #[serde(rename = "PART_VARIABLE_HPBleu_TTC")]
    blue_hp: String,
    #[serde(rename = "PART_VARIABLE_HCBlanc_TTC")]
    white_hc: String,
    #[serde(rename = "PART_VARIABLE_HPBlanc_TTC")]
    white_hp: String,
    #[serde(rename = "PART_VARIABLE_HCRouge_TTC")]
    red_hc: String,
    #[serde(rename = "PART_VARIABLE_HPRouge_TTC")]
    red_hp: String,
}

#[derive(Debug, Deserialize)]
struct OpenMeteoForecastResponse {
    daily: OpenMeteoDaily,
}

#[derive(Debug, Deserialize)]
struct OpenMeteoDaily {
    time: Vec<String>,
    temperature_2m_mean: Vec<f64>,
}

#[derive(Debug, Deserialize)]
struct ForecastCacheFile {
    data: Vec<ForecastCacheEntry>,
}

#[derive(Debug, Deserialize)]
struct ForecastCacheEntry {
    date: String,
    temperature_mean: f64,
}

#[derive(Debug, Clone)]
struct Cached<T> {
    value: T,
    fetched_at: chrono::DateTime<Utc>,
}

#[derive(Debug, Default)]
struct TempoCache {
    tempo: Option<Cached<TempoData>>,
    tarifs: Option<Cached<Option<TempoTarifs>>>,
    history: HashMap<String, Cached<HashMap<String, String>>>,
    forecast: Option<Cached<Vec<ForecastDay>>>,
    predictions: Option<Cached<TempoPredictionServiceResponse>>,
    state: Option<Cached<TempoState>>,
}

#[derive(Debug, Clone)]
struct PredictorState {
    stock_red: i32,
    stock_white: i32,
    consecutive_red: i32,
}

impl Default for PredictorState {
    fn default() -> Self {
        Self {
            stock_red: STOCK_RED_DAYS,
            stock_white: STOCK_WHITE_DAYS,
            consecutive_red: 0,
        }
    }
}

#[derive(Debug, Clone)]
struct ForecastDay {
    date: NaiveDate,
    temperature_mean: f64,
}

#[derive(Clone)]
pub struct TempoService {
    client: reqwest::Client,
    cache: Arc<RwLock<TempoCache>>,
    cache_dir: PathBuf,
    calibration_path: PathBuf,
    forecast_cache_path: PathBuf,
    calibration: Arc<TempoCalibration>,
}

#[derive(Debug, Clone)]
struct TempoCalibration {
    calibrated: bool,
    params: TempoCalibrationParams,
}

impl TempoService {
    pub fn new(source_root: PathBuf) -> Result<Self, AppError> {
        let client = reqwest::Client::builder()
            .user_agent("CatMonitor-Rust/0.1")
            .build()?;
        let cache_dir = source_root.join("cache").join("tempo");
        let calibration_path = cache_dir.join("calibration_params.json");
        let forecast_cache_path = cache_dir.join("temp_forecast.json");
        let calibration = Arc::new(load_calibration(&calibration_path)?);

        Ok(Self {
            client,
            cache: Arc::new(RwLock::new(TempoCache::default())),
            cache_dir,
            calibration_path,
            forecast_cache_path,
            calibration,
        })
    }

    pub async fn get_tempo_data(&self, force_refresh: bool) -> Result<(TempoData, bool), AppError> {
        if !force_refresh {
            if let Some(cached) = self.cached_tempo(false).await {
                return Ok((cached, false));
            }
        }

        match self.fetch_tempo_data().await {
            Ok(data) => {
                self.cache.write().await.tempo = Some(Cached {
                    value: data.clone(),
                    fetched_at: Utc::now(),
                });
                Ok((data, false))
            }
            Err(err) => {
                if let Some(cached) = self.cached_tempo(true).await {
                    Ok((cached, true))
                } else {
                    Err(err)
                }
            }
        }
    }

    pub async fn get_predictions(&self) -> Result<TempoPredictionServiceResponse, AppError> {
        if let Some(cached) = self.cached_predictions(false).await {
            return Ok(cached);
        }

        let state = self.get_state().await?;
        let season = state.season.clone();
        let history = self.fetch_history_map(&season).await?;
        let forecast = self.fetch_temperature_forecast(7).await?;

        let mut predictor_state = predictor_state_from_history(&history);
        let mut predictions = Vec::new();

        for day in forecast {
            let date_key = day.date.format("%Y-%m-%d").to_string();
            if let Some(actual_color) = history.get(&date_key).filter(|color| is_tempo_color(color)) {
                predictions.push(official_prediction(&date_key, actual_color));
                predictor_state = advance_state_with_actual_color(predictor_state, actual_color);
                continue;
            }

            let prediction = predict_day(&self.calibration.params, day.date, day.temperature_mean, &predictor_state);
            predictor_state = advance_state_with_actual_color(predictor_state, &prediction.predicted_color);
            predictions.push(prediction);
        }

        let response = TempoPredictionServiceResponse {
            success: true,
            predictions,
            state: Some(TempoPredictionState {
                season: state.season,
                stock_red_remaining: state.stock_red_remaining,
                stock_red_total: state.stock_red_total,
                stock_white_remaining: state.stock_white_remaining,
                stock_white_total: state.stock_white_total,
            }),
            model_version: Some(if self.calibration.calibrated {
                "hybrid-calibrated-1.0.0".to_string()
            } else {
                "hybrid-default-1.0.0".to_string()
            }),
        };

        self.cache.write().await.predictions = Some(Cached {
            value: response.clone(),
            fetched_at: Utc::now(),
        });

        Ok(response)
    }

    pub async fn get_state(&self) -> Result<TempoState, AppError> {
        if let Some(cached) = self.cached_state(false).await {
            return Ok(cached);
        }

        let season = current_season();
        let history = self.fetch_history_map(&season).await?;
        let predictor_state = predictor_state_from_history(&history);

        let response = TempoState {
            success: true,
            season,
            stock_red_remaining: predictor_state.stock_red,
            stock_red_total: STOCK_RED_DAYS,
            stock_white_remaining: predictor_state.stock_white,
            stock_white_total: STOCK_WHITE_DAYS,
            consecutive_red: predictor_state.consecutive_red,
        };

        self.cache.write().await.state = Some(Cached {
            value: response.clone(),
            fetched_at: Utc::now(),
        });

        Ok(response)
    }

    pub async fn get_history(&self, season: Option<&str>) -> Result<TempoHistoryResponse, AppError> {
        let season = season.map(str::to_owned).unwrap_or_else(current_season);
        let history = self.fetch_history_map(&season).await?;
        let history = sorted_history_days(&history);
        let count = history.len();

        Ok(TempoHistoryResponse {
            success: true,
            season,
            history,
            count,
        })
    }

    pub async fn get_calendar(&self, season: Option<&str>) -> Result<TempoCalendarResponse, AppError> {
        let season = season.map(str::to_owned).unwrap_or_else(current_season);
        let (start_year, end_year) = parse_season(&season)?;
        let history = self.fetch_history_map(&season).await?;

        let today = paris_today();
        let season_start = NaiveDate::from_ymd_opt(start_year, 9, 1)
            .ok_or_else(|| AppError::service_unavailable("Invalid season start"))?;
        let season_end = NaiveDate::from_ymd_opt(end_year, 8, 31)
            .ok_or_else(|| AppError::service_unavailable("Invalid season end"))?;
        let max_date = std::cmp::min(season_end, today + Duration::days(30));

        let mut calendar = Vec::new();
        let mut current = season_start;
        while current <= max_date {
            let date_key = current.format("%Y-%m-%d").to_string();
            if let Some(color) = history.get(&date_key).filter(|color| is_tempo_color(color)) {
                calendar.push(TempoCalendarDay {
                    date: date_key,
                    color: Some(color.clone()),
                    is_actual: true,
                    is_prediction: false,
                    probabilities: None,
                    confidence: None,
                    constraints: None,
                });
            } else if current > today {
                calendar.push(TempoCalendarDay {
                    date: date_key,
                    color: None,
                    is_actual: false,
                    is_prediction: true,
                    probabilities: None,
                    confidence: None,
                    constraints: None,
                });
            }
            current += Duration::days(1);
        }

        let prediction_response = self.get_predictions().await.ok();
        let predictions_count = prediction_response
            .as_ref()
            .map(|response| response.predictions.len())
            .unwrap_or_default();
        let prediction_map = prediction_response
            .map(|response| {
                response
                    .predictions
                    .into_iter()
                    .map(|prediction| (prediction.date.clone(), prediction))
                    .collect::<HashMap<_, _>>()
            })
            .unwrap_or_default();

        for day in &mut calendar {
            if !day.is_prediction {
                continue;
            }
            if let Some(prediction) = prediction_map.get(&day.date) {
                day.color = Some(prediction.predicted_color.clone());
                day.probabilities = Some(prediction.probabilities.clone());
                day.confidence = Some(prediction.confidence);
                day.constraints = Some(prediction.constraints.clone());
            }
        }

        let mut color_counts = HashMap::from([
            ("BLUE".to_string(), 0_usize),
            ("WHITE".to_string(), 0_usize),
            ("RED".to_string(), 0_usize),
        ]);
        for day in &calendar {
            if let Some(color) = &day.color {
                if let Some(count) = color_counts.get_mut(color) {
                    *count += 1;
                }
            }
        }

        let red_used = color_counts.get("RED").copied().unwrap_or_default() as i32;
        let white_used = color_counts.get("WHITE").copied().unwrap_or_default() as i32;

        Ok(TempoCalendarResponse {
            success: true,
            season,
            calendar: calendar.clone(),
            statistics: TempoCalendarStatistics {
                total_days: calendar.iter().filter(|day| day.color.is_some()).count(),
                color_counts,
                predictions_count,
            },
            stock: TempoCalendarStock {
                red_remaining: STOCK_RED_DAYS - red_used,
                red_total: STOCK_RED_DAYS,
                white_remaining: STOCK_WHITE_DAYS - white_used,
                white_total: STOCK_WHITE_DAYS,
            },
        })
    }

    pub async fn get_calibration(&self) -> Result<TempoCalibrationResponse, AppError> {
        let calibration = load_calibration(&self.calibration_path)?;
        Ok(TempoCalibrationResponse {
            success: true,
            calibrated: Some(calibration.calibrated),
            params: Some(calibration.params),
        })
    }

    async fn fetch_tempo_data(&self) -> Result<TempoData, AppError> {
        let response = self
            .client
            .get(RTE_LIGHT_API)
            .header("Accept", "application/json, text/plain, */*")
            .header("Accept-Language", "fr,fr-FR;q=0.8,en-US;q=0.5,en;q=0.3")
            .header("Cache-Control", "no-cache")
            .header("Connection", "keep-alive")
            .header("DNT", "1")
            .header("Referer", RTE_WEBPAGE_URL)
            .header("Pragma", "no-cache")
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(AppError::service_unavailable(format!(
                "Failed to fetch Tempo data: {}",
                response.status()
            )));
        }

        let payload: RteTempoResponse = response.json().await?;
        let today = paris_today();
        let tomorrow = today + Duration::days(1);
        let today_key = today.format("%Y-%m-%d").to_string();
        let tomorrow_key = tomorrow.format("%Y-%m-%d").to_string();

        Ok(TempoData {
            today: TempoDay {
                date: today_key.clone(),
                color: payload.values.get(&today_key).cloned(),
            },
            tomorrow: TempoDay {
                date: tomorrow_key.clone(),
                color: payload.values.get(&tomorrow_key).cloned(),
            },
            tarifs: self.fetch_tarifs().await,
            last_updated: Utc::now().to_rfc3339(),
        })
    }

    async fn fetch_tarifs(&self) -> Option<TempoTarifs> {
        if let Some(cached) = self.cached_tarifs(false).await {
            return cached;
        }

        let response = match self
            .client
            .get(TARIFS_API_URL)
            .header("Accept", "application/json")
            .send()
            .await
        {
            Ok(response) => response,
            Err(_) => return self.cached_tarifs(true).await.flatten(),
        };

        if !response.status().is_success() {
            return self.cached_tarifs(true).await.flatten();
        }

        let payload: TarifGouvResponse = match response.json().await {
            Ok(payload) => payload,
            Err(_) => return self.cached_tarifs(true).await.flatten(),
        };

        let row = match payload.data.first() {
            Some(row) => row,
            None => return self.cached_tarifs(true).await.flatten(),
        };

        if let Some(date_fin) = row.date_fin.as_deref() {
            let today = paris_today().format("%Y-%m-%d").to_string();
            if date_fin < today.as_str() {
                return self.cached_tarifs(true).await.flatten();
            }
        }

        let tarifs = Some(TempoTarifs {
            blue: TempoTarifColor {
                hc: row.blue_hc.parse().ok()?,
                hp: row.blue_hp.parse().ok()?,
            },
            white: TempoTarifColor {
                hc: row.white_hc.parse().ok()?,
                hp: row.white_hp.parse().ok()?,
            },
            red: TempoTarifColor {
                hc: row.red_hc.parse().ok()?,
                hp: row.red_hp.parse().ok()?,
            },
            date_debut: normalize_date_debut(&row.date_debut),
        });

        self.cache.write().await.tarifs = Some(Cached {
            value: tarifs.clone(),
            fetched_at: Utc::now(),
        });

        tarifs
    }

    async fn fetch_history_map(&self, season: &str) -> Result<HashMap<String, String>, AppError> {
        if let Some(cached) = self.cached_history(season, false).await {
            return Ok(cached);
        }

        let url = format!("{RTE_PUBLIC_API}?season={season}");
        match self.client.get(url).header("Accept", "application/json").send().await {
            Ok(response) if response.status().is_success() => {
                let payload: RteTempoResponse = response.json().await?;
                let values = payload.values;
                self.cache.write().await.history.insert(
                    season.to_string(),
                    Cached {
                        value: values.clone(),
                        fetched_at: Utc::now(),
                    },
                );
                Ok(values)
            }
            Ok(response) => {
                if let Some(values) = self.load_history_from_cache(season).await {
                    Ok(values)
                } else {
                    self.cached_history(season, true).await.ok_or_else(|| {
                        AppError::service_unavailable(format!(
                            "Failed to fetch Tempo history: {}",
                            response.status()
                        ))
                    })
                }
            }
            Err(err) => {
                if let Some(values) = self.load_history_from_cache(season).await {
                    Ok(values)
                } else {
                    self.cached_history(season, true)
                        .await
                        .ok_or(AppError::Reqwest(err))
                }
            }
        }
    }

    async fn fetch_temperature_forecast(&self, days: usize) -> Result<Vec<ForecastDay>, AppError> {
        if let Some(cached) = self.cached_forecast(false).await {
            return Ok(cached.into_iter().take(days).collect());
        }

        if let Ok(content) = std::fs::read_to_string(&self.forecast_cache_path) {
            if let Ok(cache_file) = serde_json::from_str::<ForecastCacheFile>(&content) {
                let forecast = cache_file
                    .data
                    .into_iter()
                    .filter_map(|entry| {
                        NaiveDate::parse_from_str(&entry.date, "%Y-%m-%d")
                            .ok()
                            .map(|date| ForecastDay {
                                date,
                                temperature_mean: entry.temperature_mean,
                            })
                    })
                    .collect::<Vec<_>>();

                if !forecast.is_empty() {
                    self.cache.write().await.forecast = Some(Cached {
                        value: forecast.clone(),
                        fetched_at: Utc::now(),
                    });
                    return Ok(forecast.into_iter().take(days).collect());
                }
            }
        }

        let response = self
            .client
            .get(OPEN_METEO_API)
            .query(&[
                ("latitude", FRANCE_LAT.to_string()),
                ("longitude", FRANCE_LON.to_string()),
                ("daily", "temperature_2m_mean".to_string()),
                ("timezone", "Europe/Paris".to_string()),
                ("forecast_days", days.to_string()),
            ])
            .send()
            .await?;

        if !response.status().is_success() {
            return self.cached_forecast(true).await.ok_or_else(|| {
                AppError::service_unavailable(format!(
                    "Failed to fetch temperature forecast: {}",
                    response.status()
                ))
            });
        }

        let payload: OpenMeteoForecastResponse = response.json().await?;
        let forecast = payload
            .daily
            .time
            .into_iter()
            .zip(payload.daily.temperature_2m_mean)
            .filter_map(|(date, temperature_mean)| {
                NaiveDate::parse_from_str(&date, "%Y-%m-%d")
                    .ok()
                    .map(|date| ForecastDay {
                        date,
                        temperature_mean,
                    })
            })
            .collect::<Vec<_>>();

        self.cache.write().await.forecast = Some(Cached {
            value: forecast.clone(),
            fetched_at: Utc::now(),
        });

        Ok(forecast)
    }

    async fn cached_tempo(&self, allow_expired: bool) -> Option<TempoData> {
        let cache = self.cache.read().await;
        let cached = cache.tempo.as_ref()?;
        if allow_expired || tempo_cache_is_fresh(cached.fetched_at) {
            Some(cached.value.clone())
        } else {
            None
        }
    }

    async fn cached_tarifs(&self, allow_expired: bool) -> Option<Option<TempoTarifs>> {
        let cache = self.cache.read().await;
        let cached = cache.tarifs.as_ref()?;
        let age = Utc::now() - cached.fetched_at;
        if allow_expired || age < Duration::seconds(TARIFS_CACHE_SECONDS) {
            Some(cached.value.clone())
        } else {
            None
        }
    }

    async fn cached_history(&self, season: &str, allow_expired: bool) -> Option<HashMap<String, String>> {
        let cache = self.cache.read().await;
        let cached = cache.history.get(season)?;
        let age = Utc::now() - cached.fetched_at;
        if allow_expired || age < Duration::seconds(HISTORY_CACHE_SECONDS) {
            Some(cached.value.clone())
        } else {
            None
        }
    }

    async fn cached_forecast(&self, allow_expired: bool) -> Option<Vec<ForecastDay>> {
        let cache = self.cache.read().await;
        let cached = cache.forecast.as_ref()?;
        let age = Utc::now() - cached.fetched_at;
        if allow_expired || age < Duration::seconds(FORECAST_CACHE_SECONDS) {
            Some(cached.value.clone())
        } else {
            None
        }
    }

    async fn cached_predictions(&self, allow_expired: bool) -> Option<TempoPredictionServiceResponse> {
        let cache = self.cache.read().await;
        let cached = cache.predictions.as_ref()?;
        let age = Utc::now() - cached.fetched_at;
        if allow_expired || age < Duration::seconds(PREDICTIONS_CACHE_SECONDS) {
            Some(cached.value.clone())
        } else {
            None
        }
    }

    async fn cached_state(&self, allow_expired: bool) -> Option<TempoState> {
        let cache = self.cache.read().await;
        let cached = cache.state.as_ref()?;
        let age = Utc::now() - cached.fetched_at;
        if allow_expired || age < Duration::seconds(STATE_CACHE_SECONDS) {
            Some(cached.value.clone())
        } else {
            None
        }
    }

    async fn load_history_from_cache(&self, season: &str) -> Option<HashMap<String, String>> {
        let path = self.cache_dir.join(format!("tempo_history_{season}.json"));
        let content = std::fs::read_to_string(path).ok()?;
        let payload = serde_json::from_str::<RteTempoResponse>(&content).ok()?;
        let values = payload.values;
        self.cache.write().await.history.insert(
            season.to_string(),
            Cached {
                value: values.clone(),
                fetched_at: Utc::now(),
            },
        );
        Some(values)
    }
}

fn load_calibration(path: &PathBuf) -> Result<TempoCalibration, AppError> {
    let default = TempoCalibration {
        calibrated: false,
        params: TempoCalibrationParams::default(),
    };

    if !path.exists() {
        return Ok(default);
    }

    let content = std::fs::read_to_string(path)?;
    let raw: RawCalibrationParams = serde_json::from_str(&content)?;
    let mut params = TempoCalibrationParams::default();
    params.base_consumption = raw.base_consumption.unwrap_or(params.base_consumption);
    params.thermosensitivity = raw.thermosensitivity.unwrap_or(params.thermosensitivity);
    params.temp_reference = raw.temp_reference.unwrap_or(params.temp_reference);
    params.weekend_factor = raw.weekend_factor.unwrap_or(params.weekend_factor);
    params.renewable_factor = raw.renewable_factor.unwrap_or(params.renewable_factor);
    params.calibration_date = raw.calibration_date;
    params.calibration_accuracy = raw.calibration_accuracy.unwrap_or(params.calibration_accuracy);
    params.calibration_red_recall = raw
        .calibration_red_recall
        .unwrap_or(params.calibration_red_recall);

    if let Some(monthly_adjustments) = raw.monthly_adjustments {
        params.monthly_adjustments = monthly_adjustments
            .into_iter()
            .filter_map(|(month, value)| month.parse::<u32>().ok().map(|month| (month, value)))
            .collect();
    }

    Ok(TempoCalibration {
        calibrated: true,
        params,
    })
}

fn predict_day(
    params: &TempoCalibrationParams,
    date: NaiveDate,
    temperature_mean: f64,
    state: &PredictorState,
) -> TempoPrediction {
    let estimated_consumption = estimate_consumption(params, date, temperature_mean);
    let normalized = normalize_consumption(estimated_consumption);
    let tempo_day = get_tempo_day_number(date);
    let threshold_red = calculate_threshold_red(tempo_day, state.stock_red);
    let threshold_white = calculate_threshold_white_red(tempo_day, state.stock_red, state.stock_white);
    let dist_to_red = normalized - threshold_red;
    let dist_to_white = normalized - threshold_white;

    let can_red = can_be_red(date, state.consecutive_red) && state.stock_red > 0;
    let can_white = can_be_white(date) && state.stock_white > 0;

    let prob_red = if can_red { sigmoid(dist_to_red, 1.5) } else { 0.0 };
    let prob_white = if can_white {
        sigmoid(dist_to_white, 1.5) * (1.0 - prob_red)
    } else {
        0.0
    };
    let prob_blue = (1.0 - prob_red - prob_white).max(0.0);

    let probabilities = TempoProbabilities {
        blue: prob_blue,
        white: prob_white.max(0.0),
        red: prob_red.max(0.0),
    };
    let predicted_color = max_probability_color(&probabilities).to_string();
    let confidence = probabilities
        .blue
        .max(probabilities.white)
        .max(probabilities.red);

    TempoPrediction {
        date: date.format("%Y-%m-%d").to_string(),
        predicted_color,
        probabilities,
        confidence,
        constraints: TempoConstraints {
            can_be_red: can_red,
            can_be_white: can_white,
            is_in_red_period: is_in_red_period(date),
        },
    }
}

fn official_prediction(date: &str, color: &str) -> TempoPrediction {
    let mut probabilities = TempoProbabilities {
        blue: 0.0,
        white: 0.0,
        red: 0.0,
    };
    match color {
        "BLUE" => probabilities.blue = 1.0,
        "WHITE" => probabilities.white = 1.0,
        "RED" => probabilities.red = 1.0,
        _ => {}
    }

    let parsed_date = NaiveDate::parse_from_str(date, "%Y-%m-%d").unwrap_or_else(|_| paris_today());
    TempoPrediction {
        date: date.to_string(),
        predicted_color: color.to_string(),
        probabilities,
        confidence: 1.0,
        constraints: TempoConstraints {
            can_be_red: can_be_red(parsed_date, 0),
            can_be_white: can_be_white(parsed_date),
            is_in_red_period: is_in_red_period(parsed_date),
        },
    }
}

fn predictor_state_from_history(history: &HashMap<String, String>) -> PredictorState {
    let mut sorted_dates = history
        .iter()
        .filter_map(|(date, color)| {
            if is_tempo_color(color) {
                NaiveDate::parse_from_str(date, "%Y-%m-%d")
                    .ok()
                    .map(|parsed| (parsed, color.as_str()))
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    sorted_dates.sort_by_key(|(date, _)| *date);

    let mut state = PredictorState::default();
    for (_, color) in &sorted_dates {
        state = advance_state_with_actual_color(state, color);
    }

    let consecutive_red = sorted_dates
        .iter()
        .rev()
        .take_while(|(_, color)| *color == "RED")
        .count() as i32;
    state.consecutive_red = consecutive_red;
    state
}

fn advance_state_with_actual_color(mut state: PredictorState, color: &str) -> PredictorState {
    match color {
        "RED" => {
            state.stock_red = (state.stock_red - 1).max(0);
            state.consecutive_red += 1;
        }
        "WHITE" => {
            state.stock_white = (state.stock_white - 1).max(0);
            state.consecutive_red = 0;
        }
        _ => {
            state.consecutive_red = 0;
        }
    }
    state
}

fn max_probability_color(probabilities: &TempoProbabilities) -> &'static str {
    if probabilities.red >= probabilities.white && probabilities.red >= probabilities.blue {
        "RED"
    } else if probabilities.white >= probabilities.blue {
        "WHITE"
    } else {
        "BLUE"
    }
}

fn estimate_consumption(params: &TempoCalibrationParams, date: NaiveDate, temperature: f64) -> f64 {
    let base = params.base_consumption;
    let temp_effect = (params.temp_reference - temperature) * params.thermosensitivity;
    let weekend_factor = if is_weekend(date) {
        params.weekend_factor
    } else {
        1.0
    };
    let monthly_factor = params.monthly_adjustments.get(&date.month()).copied().unwrap_or(1.0);
    let gross_consumption = (base + temp_effect) * weekend_factor * monthly_factor;
    gross_consumption.clamp(35_000.0, 75_000.0)
}

fn normalize_consumption(consumption: f64) -> f64 {
    (consumption - NORMALIZATION_MEAN) / NORMALIZATION_STD
}

fn calculate_threshold_red(tempo_day: i32, stock_red: i32) -> f64 {
    THRESHOLD_RED_A - THRESHOLD_RED_B * f64::from(tempo_day) - THRESHOLD_RED_C * f64::from(stock_red)
}

fn calculate_threshold_white_red(tempo_day: i32, stock_red: i32, stock_white: i32) -> f64 {
    THRESHOLD_WHITE_RED_A
        - THRESHOLD_WHITE_RED_B * f64::from(tempo_day)
        - THRESHOLD_WHITE_RED_C * f64::from(stock_red + stock_white)
}

fn sigmoid(value: f64, scale: f64) -> f64 {
    1.0 / (1.0 + (-value * scale).exp())
}

fn current_season() -> String {
    let today = paris_today();
    if today.month() >= 9 {
        format!("{}-{}", today.year(), today.year() + 1)
    } else {
        format!("{}-{}", today.year() - 1, today.year())
    }
}

fn paris_today() -> NaiveDate {
    Utc::now().with_timezone(&Paris).date_naive()
}

fn tempo_cache_is_fresh(fetched_at: chrono::DateTime<Utc>) -> bool {
    let now = Utc::now().with_timezone(&Paris);
    let fetched = fetched_at.with_timezone(&Paris);

    if fetched.date_naive() == now.date_naive() && fetched.hour() >= 11 {
        return true;
    }

    if fetched.date_naive() == now.date_naive() && now.hour() < 11 {
        return true;
    }

    let yesterday = now.date_naive() - Duration::days(1);
    fetched.date_naive() == yesterday && fetched.hour() >= 11 && now.hour() < 11
}

fn get_tempo_day_number(date: NaiveDate) -> i32 {
    let start_year = if date.month() >= 9 { date.year() } else { date.year() - 1 };
    let tempo_start = NaiveDate::from_ymd_opt(start_year, 9, 1).unwrap_or(date);
    (date - tempo_start).num_days() as i32
}

fn is_in_red_period(date: NaiveDate) -> bool {
    date.month() >= 11 || date.month() <= 3
}

fn is_weekend(date: NaiveDate) -> bool {
    matches!(date.weekday(), Weekday::Sat | Weekday::Sun)
}

fn is_sunday(date: NaiveDate) -> bool {
    matches!(date.weekday(), Weekday::Sun)
}

fn can_be_red(date: NaiveDate, consecutive_red: i32) -> bool {
    is_in_red_period(date) && !is_weekend(date) && consecutive_red < MAX_CONSECUTIVE_RED_DAYS
}

fn can_be_white(date: NaiveDate) -> bool {
    !is_sunday(date)
}

fn parse_season(season: &str) -> Result<(i32, i32), AppError> {
    let mut parts = season.split('-');
    let Some(start_year) = parts.next() else {
        return Err(AppError::http(axum::http::StatusCode::BAD_REQUEST, "Invalid season"));
    };
    let Some(end_year) = parts.next() else {
        return Err(AppError::http(axum::http::StatusCode::BAD_REQUEST, "Invalid season"));
    };
    if parts.next().is_some() {
        return Err(AppError::http(axum::http::StatusCode::BAD_REQUEST, "Invalid season"));
    }

    let start_year = start_year
        .parse::<i32>()
        .map_err(|_| AppError::http(axum::http::StatusCode::BAD_REQUEST, "Invalid season"))?;
    let end_year = end_year
        .parse::<i32>()
        .map_err(|_| AppError::http(axum::http::StatusCode::BAD_REQUEST, "Invalid season"))?;
    Ok((start_year, end_year))
}

fn sorted_history_days(history: &HashMap<String, String>) -> Vec<TempoHistoryDay> {
    let mut days = history
        .iter()
        .filter(|(_, color)| is_tempo_color(color))
        .map(|(date, color)| TempoHistoryDay {
            date: date.clone(),
            color: color.clone(),
            is_actual: true,
        })
        .collect::<Vec<_>>();
    days.sort_by(|left, right| left.date.cmp(&right.date));
    days
}

fn normalize_date_debut(value: &str) -> String {
    let parts = value.split('-').collect::<Vec<_>>();
    if parts.len() == 3 && parts[1].parse::<u32>().is_ok_and(|month| month > 12) {
        return format!("{}-{}-{}", parts[0], parts[2], parts[1]);
    }
    value.to_string()
}

fn is_tempo_color(color: &str) -> bool {
    matches!(color, "BLUE" | "WHITE" | "RED")
}
