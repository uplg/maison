"""
Data collector for Tempo prediction.
Fetches historical and forecast data from various APIs.
"""

import json
import os
from datetime import date, datetime, timedelta
from pathlib import Path
from typing import Optional

import pandas as pd
import requests

class DateTimeEncoder(json.JSONEncoder):
    """JSON encoder that handles datetime objects and pandas Timestamps."""
    def default(self, obj):
        if isinstance(obj, (datetime, date)):
            return obj.isoformat()
        if isinstance(obj, pd.Timestamp):
            return obj.isoformat()
        return super().default(obj)
from .constants import (
    CACHE_DIR,
    FRANCE_LAT,
    FRANCE_LON,
    OPEN_METEO_API,
    OPEN_METEO_HISTORICAL_API,
    RTE_ECO2MIX_API,
    RTE_TEMPO_API,
    get_tempo_year,
)


class TempoDataCollector:
    """
    Collects data required for Tempo prediction:
    - Historical Tempo colors
    - Electricity consumption and production
    - Temperature data (historical and forecast)
    """

    def __init__(self, cache_dir: Optional[str] = None):
        self.cache_dir = Path(cache_dir or CACHE_DIR)
        self.cache_dir.mkdir(parents=True, exist_ok=True)
        self.session = requests.Session()
        self.session.headers.update({
            "User-Agent": "TempoPredictor/1.0",
            "Accept": "application/json",
        })

    def _get_cache_path(self, name: str) -> Path:
        return self.cache_dir / f"{name}.json"

    def _load_cache(self, name: str, max_age_hours: int = 24) -> Optional[dict]:
        """Load cached data if it exists and is not too old."""
        cache_path = self._get_cache_path(name)
        if not cache_path.exists():
            return None
        
        try:
            with open(cache_path, "r") as f:
                data = json.load(f)
            
            cached_at = datetime.fromisoformat(data.get("_cached_at", "2000-01-01"))
            if datetime.now() - cached_at > timedelta(hours=max_age_hours):
                return None
            
            return data
        except (json.JSONDecodeError, KeyError):
            return None

    def _save_cache(self, name: str, data: dict):
        """Save data to cache with proper datetime serialization."""
        data["_cached_at"] = datetime.now().isoformat()
        cache_path = self._get_cache_path(name)
        with open(cache_path, "w") as f:
            json.dump(data, f, indent=2, cls=DateTimeEncoder)

    def fetch_tempo_history(self, season: Optional[str] = None) -> dict[str, str]:
        """
        Fetch Tempo color history from RTE API.
        
        Args:
            season: Season in format "YYYY-YYYY" (e.g., "2024-2025")
                   If None, uses current season.
        
        Returns:
            Dict mapping date strings to colors: {"2024-01-15": "BLUE", ...}
        """
        if season is None:
            start_year, end_year = get_tempo_year(date.today())
            season = f"{start_year}-{end_year}"

        cache_name = f"tempo_history_{season}"
        cached = self._load_cache(cache_name, max_age_hours=6)
        if cached and "values" in cached:
            return cached["values"]

        url = f"{RTE_TEMPO_API}?season={season}"
        
        try:
            response = self.session.get(url, timeout=30)
            response.raise_for_status()
            data = response.json()
            
            values = data.get("values", {})
            self._save_cache(cache_name, {"values": values})
            
            return values
        except requests.RequestException as e:
            print(f"Error fetching Tempo history: {e}")
            # Return cached data even if expired
            if cached and "values" in cached:
                return cached["values"]
            return {}

    def fetch_tempo_history_all_seasons(self, start_year: int = 2014) -> pd.DataFrame:
        """
        Fetch all Tempo history from start_year to current season.
        
        Returns:
            DataFrame with columns: date, color, day_of_week, month, tempo_day
        """
        all_data = []
        current_year = date.today().year
        
        for year in range(start_year, current_year + 1):
            season = f"{year}-{year + 1}"
            print(f"Fetching season {season}...")
            
            try:
                values = self.fetch_tempo_history(season)
                for date_str, color in values.items():
                    if color in ["BLUE", "WHITE", "RED"]:
                        all_data.append({
                            "date": date_str,
                            "color": color,
                        })
            except Exception as e:
                print(f"Error fetching season {season}: {e}")
                continue
        
        if not all_data:
            return pd.DataFrame()
        
        df = pd.DataFrame(all_data)
        df["date"] = pd.to_datetime(df["date"])
        df = df.sort_values("date").reset_index(drop=True)
        
        # Add derived features
        df["day_of_week"] = df["date"].dt.dayofweek
        df["month"] = df["date"].dt.month
        df["day_of_year"] = df["date"].dt.dayofyear
        df["is_weekend"] = df["day_of_week"] >= 5
        
        return df

    def fetch_eco2mix_data(
        self,
        start_date: date,
        end_date: date,
    ) -> pd.DataFrame:
        """
        Fetch electricity consumption and production data from éCO2mix.
        
        Returns:
            DataFrame with columns: date, consumption, wind, solar, net_consumption
        """
        cache_name = f"eco2mix_{start_date}_{end_date}"
        cached = self._load_cache(cache_name, max_age_hours=24)
        if cached and "data" in cached:
            return pd.DataFrame(cached["data"])

        # éCO2mix API with date filter
        params = {
            "select": "date,heure,consommation,eolien,solaire",
            "where": f"date >= '{start_date}' AND date <= '{end_date}'",
            "order_by": "date,heure",
            "limit": 100,
            "offset": 0,
        }

        all_records = []
        
        try:
            while True:
                response = self.session.get(RTE_ECO2MIX_API, params=params, timeout=30)
                response.raise_for_status()
                data = response.json()
                
                records = data.get("results", [])
                if not records:
                    break
                
                all_records.extend(records)
                
                if len(records) < params["limit"]:
                    break
                
                params["offset"] += params["limit"]
                
                # Limit to prevent infinite loops
                if params["offset"] > 50000:
                    break

        except requests.RequestException as e:
            print(f"Error fetching éCO2mix data: {e}")
            if cached and "data" in cached:
                return pd.DataFrame(cached["data"])
            return pd.DataFrame()

        if not all_records:
            return pd.DataFrame()

        df = pd.DataFrame(all_records)
        df["date"] = pd.to_datetime(df["date"])
        
        # Aggregate to daily values
        daily = df.groupby(df["date"].dt.date).agg({
            "consommation": "mean",
            "eolien": "mean",
            "solaire": "mean",
        }).reset_index()
        
        daily.columns = ["date", "consumption", "wind", "solar"]
        daily["date"] = pd.to_datetime(daily["date"])
        daily["net_consumption"] = daily["consumption"] - daily["wind"] - daily["solar"]
        
        self._save_cache(cache_name, {"data": daily.to_dict("records")})
        
        return daily

    def fetch_temperature_history(
        self,
        start_date: date,
        end_date: date,
    ) -> pd.DataFrame:
        """
        Fetch historical temperature data from Open-Meteo.
        
        Returns:
            DataFrame with columns: date, temperature_mean
        """
        cache_name = f"temp_history_{start_date}_{end_date}"
        cached = self._load_cache(cache_name, max_age_hours=168)  # 1 week cache
        if cached and "data" in cached:
            return pd.DataFrame(cached["data"])

        params = {
            "latitude": FRANCE_LAT,
            "longitude": FRANCE_LON,
            "start_date": start_date.isoformat(),
            "end_date": end_date.isoformat(),
            "daily": "temperature_2m_mean",
            "timezone": "Europe/Paris",
        }

        try:
            response = self.session.get(OPEN_METEO_HISTORICAL_API, params=params, timeout=30)
            response.raise_for_status()
            data = response.json()
            
            daily = data.get("daily", {})
            dates = daily.get("time", [])
            temps = daily.get("temperature_2m_mean", [])
            
            df = pd.DataFrame({
                "date": pd.to_datetime(dates),
                "temperature_mean": temps,
            })
            
            self._save_cache(cache_name, {"data": df.to_dict("records")})
            
            return df

        except requests.RequestException as e:
            print(f"Error fetching temperature history: {e}")
            if cached and "data" in cached:
                return pd.DataFrame(cached["data"])
            return pd.DataFrame()

    def fetch_temperature_forecast(self, days: int = 7) -> pd.DataFrame:
        """
        Fetch temperature forecast from Open-Meteo.
        
        Returns:
            DataFrame with columns: date, temperature_mean
        """
        cache_name = "temp_forecast"
        cached = self._load_cache(cache_name, max_age_hours=3)
        if cached and "data" in cached:
            return pd.DataFrame(cached["data"])

        params = {
            "latitude": FRANCE_LAT,
            "longitude": FRANCE_LON,
            "daily": "temperature_2m_mean",
            "timezone": "Europe/Paris",
            "forecast_days": days,
        }

        try:
            response = self.session.get(OPEN_METEO_API, params=params, timeout=30)
            response.raise_for_status()
            data = response.json()
            
            daily = data.get("daily", {})
            dates = daily.get("time", [])
            temps = daily.get("temperature_2m_mean", [])
            
            df = pd.DataFrame({
                "date": pd.to_datetime(dates),
                "temperature_mean": temps,
            })
            
            self._save_cache(cache_name, {"data": df.to_dict("records")})
            
            return df

        except requests.RequestException as e:
            print(f"Error fetching temperature forecast: {e}")
            if cached and "data" in cached:
                return pd.DataFrame(cached["data"])
            return pd.DataFrame()

    def build_training_dataset(
        self,
        start_year: int = 2015,
        end_date: Optional[date] = None,
    ) -> pd.DataFrame:
        """
        Build a complete training dataset combining all data sources.
        
        Returns:
            DataFrame ready for ML training with all features and labels.
        """
        if end_date is None:
            end_date = date.today() - timedelta(days=1)
        
        start_date = date(start_year, 9, 1)  # Start from September
        
        print("Fetching Tempo history...")
        tempo_df = self.fetch_tempo_history_all_seasons(start_year)
        if tempo_df.empty:
            raise ValueError("No Tempo history data available")
        
        print("Fetching temperature history...")
        temp_df = self.fetch_temperature_history(start_date, end_date)
        
        # Merge datasets
        if not temp_df.empty:
            tempo_df = tempo_df.merge(temp_df, on="date", how="left")
        else:
            tempo_df["temperature_mean"] = None
        
        # Add Tempo-specific features
        tempo_df["tempo_day"] = tempo_df["date"].apply(
            lambda d: self._get_tempo_day_number(d.date())
        )
        
        # Encode colors
        color_map = {"BLUE": 0, "WHITE": 1, "RED": 2}
        tempo_df["color_code"] = tempo_df["color"].map(color_map)
        
        return tempo_df

    def _get_tempo_day_number(self, d: date) -> int:
        """Get day number within Tempo year."""
        from .constants import get_tempo_day_number
        return get_tempo_day_number(d)


if __name__ == "__main__":
    # Test the data collector
    collector = TempoDataCollector()
    
    print("Testing Tempo history fetch...")
    history = collector.fetch_tempo_history("2024-2025")
    print(f"Got {len(history)} days of history")
    
    print("\nTesting temperature forecast...")
    forecast = collector.fetch_temperature_forecast()
    print(forecast)
