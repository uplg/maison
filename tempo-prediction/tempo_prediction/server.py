"""
Standalone HTTP server for Tempo predictions.
Runs independently and can be called by the main Elysia backend.
Uses the Hybrid predictor (calibrated RTE algorithm).
"""

import json
import os
from datetime import date, timedelta
from http.server import HTTPServer, BaseHTTPRequestHandler
from urllib.parse import parse_qs, urlparse
import threading
from typing import Optional

from .data_collector import TempoDataCollector
from .constants import get_tempo_year, STOCK_RED_DAYS, STOCK_WHITE_DAYS
from .hybrid_predictor import HybridTempoPredictor


class TempoPredictionHandler(BaseHTTPRequestHandler):
    """HTTP request handler for Tempo predictions."""

    hybrid_predictor: Optional[HybridTempoPredictor] = None
    collector: TempoDataCollector = None

    def _send_json(self, data: dict, status: int = 200):
        """Send JSON response."""
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Access-Control-Allow-Origin", "*")
        self.end_headers()
        self.wfile.write(json.dumps(data, default=str).encode())

    def _send_error(self, message: str, status: int = 500):
        """Send error response."""
        self._send_json({"success": False, "error": message}, status)

    def do_OPTIONS(self):
        """Handle CORS preflight."""
        self.send_response(200)
        self.send_header("Access-Control-Allow-Origin", "*")
        self.send_header("Access-Control-Allow-Methods", "GET, POST, OPTIONS")
        self.send_header("Access-Control-Allow-Headers", "Content-Type")
        self.end_headers()

    def do_GET(self):
        """Handle GET requests."""
        parsed = urlparse(self.path)
        path = parsed.path
        query = parse_qs(parsed.query)

        try:
            if path == "/health":
                self._handle_health()
            elif path == "/predict/week":
                self._handle_predict_week()
            elif path == "/predict/hybrid":
                self._handle_predict_hybrid()
            elif path == "/state":
                self._handle_state()
            elif path == "/thresholds":
                self._handle_thresholds(query)
            elif path == "/history":
                self._handle_history(query)
            elif path == "/calendar":
                self._handle_calendar(query)
            elif path == "/calibration":
                self._handle_calibration()
            else:
                self._send_error("Not found", 404)
        except Exception as e:
            import traceback

            traceback.print_exc()
            self._send_error(str(e), 500)

    def do_POST(self):
        """Handle POST requests."""
        parsed = urlparse(self.path)
        path = parsed.path

        try:
            if path == "/calibrate":
                self._handle_recalibrate()
            else:
                self._send_error("Not found", 404)
        except Exception as e:
            import traceback

            traceback.print_exc()
            self._send_error(str(e), 500)

    def _handle_health(self):
        """Health check endpoint."""
        hybrid_loaded = self.hybrid_predictor is not None and self.hybrid_predictor._calibrated

        self._send_json(
            {
                "success": True,
                "status": "healthy",
                "active_model": "hybrid",
                "models": {
                    "hybrid": {
                        "available": True,
                        "loaded": hybrid_loaded,
                        "calibrated": hybrid_loaded,
                    },
                },
            }
        )

    def _handle_predict_week(self):
        """Predict for the next 7 days using the hybrid predictor."""
        if not self.hybrid_predictor or not self.hybrid_predictor._calibrated:
            self._send_error("Hybrid predictor not calibrated", 503)
            return

        state = self.hybrid_predictor.estimate_current_state()
        predictions = self.hybrid_predictor.predict_week(
            stock_red=state.stock_red,
            stock_white=state.stock_white,
        )
        model_version = "hybrid-calibrated-1.0.0"

        # Override with actual RTE colors for today and tomorrow if available
        today = date.today()
        start_year, end_year = get_tempo_year(today)
        season = f"{start_year}-{end_year}"
        history = self.collector.fetch_tempo_history(season)

        for pred in predictions:
            pred_date = pred["date"]
            if pred_date in history and history[pred_date] in ["BLUE", "WHITE", "RED"]:
                actual_color = history[pred_date]
                pred["predicted_color"] = actual_color
                pred["is_official"] = True
                # Set probabilities to 100% for actual color
                pred["probabilities"] = {"BLUE": 0, "WHITE": 0, "RED": 0}
                pred["probabilities"][actual_color] = 1.0
                pred["confidence"] = 1.0
            else:
                pred["is_official"] = False

        self._send_json(
            {
                "success": True,
                "predictions": predictions,
                "state": {
                    "season": season,
                    "stock_red_remaining": state.stock_red,
                    "stock_red_total": STOCK_RED_DAYS,
                    "stock_white_remaining": state.stock_white,
                    "stock_white_total": STOCK_WHITE_DAYS,
                },
                "model_version": model_version,
                "message": "Predictions generated successfully",
            }
        )

    def _handle_predict_hybrid(self):
        """Predict using hybrid predictor specifically."""
        if not self.hybrid_predictor or not self.hybrid_predictor._calibrated:
            self._send_error("Hybrid predictor not calibrated", 503)
            return

        state = self.hybrid_predictor.estimate_current_state()
        predictions = self.hybrid_predictor.predict_week(
            stock_red=state.stock_red,
            stock_white=state.stock_white,
        )

        today = date.today()
        start_year, end_year = get_tempo_year(today)

        self._send_json(
            {
                "success": True,
                "predictions": predictions,
                "state": {
                    "season": f"{start_year}-{end_year}",
                    "stock_red_remaining": state.stock_red,
                    "stock_red_total": STOCK_RED_DAYS,
                    "stock_white_remaining": state.stock_white,
                    "stock_white_total": STOCK_WHITE_DAYS,
                },
                "model_version": "hybrid-calibrated-1.0.0",
                "calibration": self.hybrid_predictor.get_calibration_info(),
            }
        )

    def _handle_state(self):
        """Get current Tempo state (stocks, etc.)."""
        state = self.hybrid_predictor.estimate_current_state()

        today = date.today()
        start_year, end_year = get_tempo_year(today)

        self._send_json(
            {
                "success": True,
                "season": f"{start_year}-{end_year}",
                "stock_red_remaining": state.stock_red,
                "stock_red_total": STOCK_RED_DAYS,
                "stock_white_remaining": state.stock_white,
                "stock_white_total": STOCK_WHITE_DAYS,
                "consecutive_red": state.consecutive_red,
            }
        )

    def _handle_thresholds(self, query: dict):
        """Get algorithm thresholds for a date."""
        from .algorithm import TempoAlgorithm

        date_str = query.get("date", [None])[0]
        if date_str:
            target_date = date.fromisoformat(date_str)
        else:
            target_date = date.today()

        algo = TempoAlgorithm()
        state = self.hybrid_predictor.estimate_current_state()
        info = algo.predict_with_thresholds(target_date, state)

        self._send_json(
            {
                "success": True,
                **info,
            }
        )

    def _handle_history(self, query: dict):
        """Get historical Tempo colors for a season."""
        season = query.get("season", [None])[0]

        history = self.hybrid_predictor.get_season_history(season)

        self._send_json(
            {
                "success": True,
                "season": season,
                "history": history,
                "count": len(history),
            }
        )

    def _handle_calendar(self, query: dict):
        """
        Get calendar data: historical colors + predictions for upcoming days.

        Returns a combined view suitable for a calendar display.
        """
        # Get season parameter
        season_param = query.get("season", [None])[0]

        # Determine season
        today = date.today()
        if season_param:
            season = season_param
            start_year = int(season.split("-")[0])
            end_year = int(season.split("-")[1])
        else:
            start_year, end_year = get_tempo_year(today)
            season = f"{start_year}-{end_year}"

        # Get historical data
        raw_history = self.collector.fetch_tempo_history(season)

        # Build calendar data
        calendar_data = []
        season_start = date(start_year, 9, 1)
        season_end = date(end_year, 8, 31)

        # Process each day in the season
        current = season_start
        while current <= min(season_end, today + timedelta(days=30)):
            date_str = current.isoformat()

            if date_str in raw_history and raw_history[date_str] in ["BLUE", "WHITE", "RED"]:
                # Historical data
                calendar_data.append(
                    {
                        "date": date_str,
                        "color": raw_history[date_str],
                        "is_actual": True,
                        "is_prediction": False,
                    }
                )
            elif current > today:
                # Future date - will be filled with predictions
                calendar_data.append(
                    {
                        "date": date_str,
                        "color": None,  # Will be filled
                        "is_actual": False,
                        "is_prediction": True,
                    }
                )

            current += timedelta(days=1)

        # Get predictions for upcoming days
        predictions = []
        if self.hybrid_predictor and self.hybrid_predictor._calibrated:
            state = self.hybrid_predictor.estimate_current_state()
            predictions = self.hybrid_predictor.predict_week(
                stock_red=state.stock_red,
                stock_white=state.stock_white,
            )

        # Merge predictions into calendar data
        prediction_dict = {p["date"]: p for p in predictions}
        for item in calendar_data:
            if item["is_prediction"] and item["date"] in prediction_dict:
                pred = prediction_dict[item["date"]]
                item["color"] = pred["predicted_color"]
                item["probabilities"] = pred["probabilities"]
                item["confidence"] = pred["confidence"]
                item["constraints"] = pred.get("constraints", {})

        # Calculate statistics
        color_counts = {"BLUE": 0, "WHITE": 0, "RED": 0}
        for item in calendar_data:
            if item["color"] in color_counts:
                color_counts[item["color"]] += 1

        self._send_json(
            {
                "success": True,
                "season": season,
                "calendar": calendar_data,
                "statistics": {
                    "total_days": len([d for d in calendar_data if d["color"]]),
                    "color_counts": color_counts,
                    "predictions_count": len(predictions),
                },
                "stock": {
                    "red_remaining": STOCK_RED_DAYS - color_counts["RED"],
                    "red_total": STOCK_RED_DAYS,
                    "white_remaining": STOCK_WHITE_DAYS - color_counts["WHITE"],
                    "white_total": STOCK_WHITE_DAYS,
                },
            }
        )

    def _handle_calibration(self):
        """Get calibration info."""
        if not self.hybrid_predictor:
            self._send_error("Hybrid predictor not initialized", 503)
            return

        self._send_json(
            {
                "success": True,
                **self.hybrid_predictor.get_calibration_info(),
            }
        )

    def _handle_recalibrate(self):
        """Trigger recalibration of the hybrid predictor."""
        if not self.hybrid_predictor:
            self._send_error("Hybrid predictor not initialized", 503)
            return

        result = self.hybrid_predictor.calibrate(start_year=2015, save=True)

        self._send_json(
            {
                "success": True,
                "message": "Recalibration complete",
                "calibration": result,
            }
        )

    def log_message(self, format, *args):
        """Suppress default logging."""
        pass


def create_server(host: str = "127.0.0.1", port: int = 3034) -> HTTPServer:
    """Create and configure the prediction server."""
    # Initialize shared resources
    TempoPredictionHandler.collector = TempoDataCollector()

    # Initialize hybrid predictor
    print("Initializing hybrid predictor...")
    TempoPredictionHandler.hybrid_predictor = HybridTempoPredictor(
        collector=TempoPredictionHandler.collector,
        auto_load=True,
    )

    if TempoPredictionHandler.hybrid_predictor._calibrated:
        print("Hybrid predictor loaded (calibrated)")
    else:
        print("Hybrid predictor not calibrated, running calibration...")
        TempoPredictionHandler.hybrid_predictor.calibrate(start_year=2015, save=True)

    server = HTTPServer((host, port), TempoPredictionHandler)
    return server


def main():
    """Run the prediction server."""
    host = "127.0.0.1"
    port = 3034

    print("=" * 50)
    print("TEMPO PREDICTION SERVER")
    print("=" * 50)

    server = create_server(host, port)

    print(f"\nServer running at http://{host}:{port}")
    print("\nEndpoints:")
    print("  GET /health           - Health check")
    print("  GET /predict/week     - Predict next 7 days")
    print("  GET /predict/hybrid   - Predict using hybrid predictor")
    print("  GET /state            - Get current Tempo state")
    print("  GET /thresholds       - Get algorithm thresholds")
    print("  GET /history          - Get historical colors (?season=2024-2025)")
    print("  GET /calendar         - Get calendar data with predictions")
    print("  GET /calibration      - Get calibration info")
    print("  POST /calibrate       - Trigger recalibration")
    print("\nPress Ctrl+C to stop\n")

    try:
        server.serve_forever()
    except KeyboardInterrupt:
        print("\n\nServer stopped")
        server.shutdown()


if __name__ == "__main__":
    main()
