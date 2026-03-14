#!/usr/bin/env bash

set -euo pipefail

BASE_URL="${BASE_URL:-http://127.0.0.1:3033}"
USERNAME="${USERNAME:-}"
PASSWORD="${PASSWORD:-}"
BROADLINK_HOST="${BROADLINK_HOST:-}"
LOCAL_IP="${LOCAL_IP:-}"
MODEL="${MODEL:-msz-hj5va}"
COOKIE_JAR="${COOKIE_JAR:-/tmp/maison-cookies.txt}"
TIMEOUT_SECS="${TIMEOUT_SECS:-30}"
TAG1="${TAG1:-clim}"
TAG2="${TAG2:-salon}"
TAG3="${TAG3:-reverse}"
PROGRESS_FILE="${PROGRESS_FILE:-cache/mitsubishi-capture-progress-${MODEL}.txt}"
ONLY_GROUPS="${ONLY_GROUPS:-}"
AUTO_ADVANCE="${AUTO_ADVANCE:-0}"
RESET_PROGRESS="${RESET_PROGRESS:-0}"

declare -a TASK_GROUPS=()
declare -a TASK_COMMANDS=()
declare -a TASK_NAMES=()
PROGRESS_DONE=""
REMOTE_DONE=""

require_var() {
  local name="$1"
  local value="$2"
  if [[ -z "$value" ]]; then
    printf 'Missing required variable: %s\n' "$name" >&2
    exit 1
  fi
}

escape_json() {
  python3 -c 'import json,sys; print(json.dumps(sys.argv[1]))' "$1"
}

json_string_or_null() {
  local value="$1"
  if [[ -z "$value" ]]; then
    printf 'null'
  else
    escape_json "$value"
  fi
}

normalize_group_filter() {
  if [[ -z "$ONLY_GROUPS" ]]; then
    return
  fi

  ONLY_GROUPS=",$(printf '%s' "$ONLY_GROUPS" | tr '[:upper:]' '[:lower:]' | tr -d ' '),"
}

group_enabled() {
  local group="$1"
  if [[ -z "$ONLY_GROUPS" ]]; then
    return 0
  fi

  [[ "$ONLY_GROUPS" == *",${group},"* ]]
}

add_task() {
  local group="$1"
  local command="$2"
  local name="$3"

  if ! group_enabled "$group"; then
    return
  fi

  TASK_GROUPS+=("$group")
  TASK_COMMANDS+=("$command")
  TASK_NAMES+=("$name")
}

init_progress_file() {
  mkdir -p "$(dirname "$PROGRESS_FILE")"
  if [[ "$RESET_PROGRESS" == "1" ]]; then
    rm -f "$PROGRESS_FILE"
  fi
  touch "$PROGRESS_FILE"
}

load_progress_file() {
  local status command _name
  while IFS='|' read -r status command _name; do
    if [[ "$status" == "done" && -n "$command" ]]; then
      append_done_entry PROGRESS_DONE "$command"
    fi
  done < "$PROGRESS_FILE"
}

mark_done() {
  local command="$1"
  local name="$2"
  append_done_entry PROGRESS_DONE "$command"
  printf 'done|%s|%s\n' "$command" "$name" >> "$PROGRESS_FILE"
}

append_done_entry() {
  local var_name="$1"
  local command="$2"
  local current_value

  current_value="${!var_name}"
  if ! printf '%s\n' "$current_value" | grep -Fqx "$command"; then
    if [[ -n "$current_value" ]]; then
      printf -v "$var_name" '%s\n%s' "$current_value" "$command"
    else
      printf -v "$var_name" '%s' "$command"
    fi
  fi
}

command_done() {
  local command="$1"
  printf '%s\n%s\n' "$PROGRESS_DONE" "$REMOTE_DONE" | grep -Fqx "$command"
}

curl_json() {
  local method="$1"
  local url="$2"
  local body="${3:-}"
  local tmp_body
  local http_code

  tmp_body="$(mktemp)"
  if [[ -n "$body" ]]; then
    http_code="$(curl -sS -b "$COOKIE_JAR" -c "$COOKIE_JAR" -o "$tmp_body" -w '%{http_code}' -X "$method" "$url" -H 'Content-Type: application/json' --data "$body")"
  else
    http_code="$(curl -sS -b "$COOKIE_JAR" -c "$COOKIE_JAR" -o "$tmp_body" -w '%{http_code}' -X "$method" "$url")"
  fi

  if [[ "$http_code" != 2* ]]; then
    printf 'HTTP %s for %s %s\n' "$http_code" "$method" "$url" >&2
    cat "$tmp_body" >&2
    rm -f "$tmp_body"
    return 1
  fi

  cat "$tmp_body"
  rm -f "$tmp_body"
}

login() {
  require_var USERNAME "$USERNAME"
  require_var PASSWORD "$PASSWORD"

  printf 'Logging in to %s\n' "$BASE_URL"
  curl_json "POST" "$BASE_URL/api/auth/login" "{\"username\":$(escape_json "$USERNAME"),\"password\":$(escape_json "$PASSWORD")}" >/dev/null
}

load_remote_saved_commands() {
  local response
  response="$(curl_json "GET" "$BASE_URL/api/broadlink/mitsubishi/codes?model=$MODEL")"

  while IFS= read -r command; do
    if [[ -n "$command" ]]; then
      append_done_entry REMOTE_DONE "$command"
    fi
  done < <(printf '%s' "$response" | python3 -c 'import json,sys; data=json.load(sys.stdin); [print(code.get("command", "")) for code in data.get("codes", [])]')
}

learn() {
  local command="$1"
  local name="$2"
  local response
  local success

  response="$(curl_json "POST" "$BASE_URL/api/broadlink/learn/ir" "$(cat <<JSON
{
  "host": $(escape_json "$BROADLINK_HOST"),
  "localIp": $(json_string_or_null "$LOCAL_IP"),
  "timeoutSecs": $TIMEOUT_SECS,
  "saveCode": {
    "name": $(escape_json "$name"),
    "brand": "mitsubishi",
    "model": $(escape_json "$MODEL"),
    "command": $(escape_json "$command"),
    "tags": [
      $(escape_json "$TAG1"),
      $(escape_json "$TAG2"),
      $(escape_json "$TAG3")
    ]
  }
}
JSON
)")"

  printf '%s\n' "$response"
  success="$(printf '%s' "$response" | python3 -c 'import json,sys; data=json.load(sys.stdin); print("true" if data.get("success") else "false")')"
  [[ "$success" == "true" ]]
}

wait_before_learn() {
  local group="$1"
  local command="$2"
  local name="$3"
  local answer

  if [[ "$AUTO_ADVANCE" == "1" ]]; then
    return 0
  fi

  while true; do
    printf '\n[%s] %s\n' "$group" "$name"
    printf 'Command: %s\n' "$command"
    printf 'Remote ready? [y=learn, n=wait, q=quit]\n'
    read -r answer
    case "$answer" in
      ""|y|Y|yes|YES|Yes) return 0 ;;
      n|N|no|NO|No)
        printf 'Waiting. Prepare the remote, then answer Y when ready.\n'
        ;;
      q|Q|quit|QUIT|Quit)
        printf 'Stopping now. Resume later with the same script; completed captures stay recorded in %s\n' "$PROGRESS_FILE"
        exit 0
        ;;
      *) printf 'Please answer Y, N, or Q.\n' ;;
    esac
  done
}

run_tasks() {
  local total="${#TASK_COMMANDS[@]}"
  local index
  local completed=0
  local skipped_existing=0

  for command in "${TASK_COMMANDS[@]}"; do
    if command_done "$command"; then
      skipped_existing=$((skipped_existing + 1))
    fi
  done

  printf 'Loaded %s tasks for model %s\n' "$total" "$MODEL"
  printf 'Already saved or completed: %s\n' "$skipped_existing"
  printf 'Progress file: %s\n' "$PROGRESS_FILE"

  for ((index = 0; index < total; index++)); do
    local group="${TASK_GROUPS[$index]}"
    local command="${TASK_COMMANDS[$index]}"
    local name="${TASK_NAMES[$index]}"

    if command_done "$command"; then
      printf '[skip] %s\n' "$name"
      continue
    fi

    wait_before_learn "$group" "$command" "$name"

    printf '\n============================================================\n'
    printf 'Capture %d/%d\n' "$((index + 1))" "$total"
    printf 'Group  : %s\n' "$group"
    printf 'Name   : %s\n' "$name"
    printf 'Command: %s\n' "$command"
    printf 'Action : Press the matching button on the physical remote now.\n'
    printf 'Timeout: %ss\n' "$TIMEOUT_SECS"
    printf '============================================================\n'

    if learn "$command" "$name"; then
      mark_done "$command" "$name"
      completed=$((completed + 1))
      printf '[done] %s\n' "$name"
    else
      printf '[failed] %s\n' "$name" >&2
      printf 'You can rerun the script; this capture was not marked complete.\n' >&2
    fi
  done

  printf '\nNew captures this run: %s\n' "$completed"
}

add_mode_tasks() {
  add_task modes "state-off" "State Off"
  add_task modes "state-cool-20-fan-auto-vane-auto-wide-center" "State Cool 20C Fan Auto Vane Auto Wide Center"
  add_task modes "state-heat-20-fan-auto-vane-auto-wide-center" "State Heat 20C Fan Auto Vane Auto Wide Center"
  add_task modes "state-dry-20-fan-auto-vane-auto-wide-center" "State Dry 20C Fan Auto Vane Auto Wide Center"
  add_task modes "state-fan-20-fan-auto-vane-auto-wide-center" "State Fan 20C Fan Auto Vane Auto Wide Center"
  add_task modes "state-auto-20-fan-auto-vane-auto-wide-center" "State Auto 20C Fan Auto Vane Auto Wide Center"
}

add_cool_temperature_tasks() {
  local temp
  for temp in 16 17 18 19 20 21 22 23 24; do
    add_task cool-temp \
      "state-cool-${temp}-fan-auto-vane-auto-wide-center" \
      "State Cool ${temp}C Fan Auto Vane Auto Wide Center"
  done
}

add_heat_temperature_tasks() {
  local temp
  for temp in 16 17 18 19 20 21 22 23 24; do
    add_task heat-temp \
      "state-heat-${temp}-fan-auto-vane-auto-wide-center" \
      "State Heat ${temp}C Fan Auto Vane Auto Wide Center"
  done
}

add_fan_tasks() {
  add_task fan "state-cool-20-fan-auto-vane-auto-wide-center" "State Cool 20C Fan Auto Vane Auto Wide Center"
  add_task fan "state-cool-20-fan-1-vane-auto-wide-center" "State Cool 20C Fan 1 Vane Auto Wide Center"
  add_task fan "state-cool-20-fan-2-vane-auto-wide-center" "State Cool 20C Fan 2 Vane Auto Wide Center"
  add_task fan "state-cool-20-fan-3-vane-auto-wide-center" "State Cool 20C Fan 3 Vane Auto Wide Center"
  add_task fan "state-cool-20-fan-4-vane-auto-wide-center" "State Cool 20C Fan 4 Vane Auto Wide Center"
  add_task fan "state-cool-20-fan-silent-vane-auto-wide-center" "State Cool 20C Fan Silent Vane Auto Wide Center"
}

add_vertical_vane_tasks() {
  add_task vertical-vane "state-cool-20-fan-auto-vane-auto-wide-center" "State Cool 20C Fan Auto Vane Auto Wide Center"
  add_task vertical-vane "state-cool-20-fan-auto-vane-highest-wide-center" "State Cool 20C Fan Auto Vane Highest Wide Center"
  add_task vertical-vane "state-cool-20-fan-auto-vane-high-wide-center" "State Cool 20C Fan Auto Vane High Wide Center"
  add_task vertical-vane "state-cool-20-fan-auto-vane-middle-wide-center" "State Cool 20C Fan Auto Vane Middle Wide Center"
  add_task vertical-vane "state-cool-20-fan-auto-vane-low-wide-center" "State Cool 20C Fan Auto Vane Low Wide Center"
  add_task vertical-vane "state-cool-20-fan-auto-vane-lowest-wide-center" "State Cool 20C Fan Auto Vane Lowest Wide Center"
  add_task vertical-vane "state-cool-20-fan-auto-vane-swing-wide-center" "State Cool 20C Fan Auto Vane Swing Wide Center"
}

add_extra_tasks() {
  add_task extras "state-cool-20-fan-auto-vane-auto-wide-center-econo-off" "State Cool 20C Fan Auto Vane Auto Wide Center Econo Off"
  add_task extras "state-cool-20-fan-auto-vane-auto-wide-center-econo-on" "State Cool 20C Fan Auto Vane Auto Wide Center Econo On"
}

add_timer_tasks() {
  add_task timers "state-cool-20-fan-auto-vane-auto-wide-center-timer-off" "State Cool 20C Fan Auto Vane Auto Wide Center Timer Off"
  add_task timers "state-cool-20-fan-auto-vane-auto-wide-center-start-06-00" "State Cool 20C Fan Auto Vane Auto Wide Center Start 06:00"
  add_task timers "state-cool-20-fan-auto-vane-auto-wide-center-stop-11-00" "State Cool 20C Fan Auto Vane Auto Wide Center Stop 11:00"
  add_task timers "state-cool-20-fan-auto-vane-auto-wide-center-stop-12-00" "State Cool 20C Fan Auto Vane Auto Wide Center Stop 12:00"
}

build_task_list() {
  add_mode_tasks
  add_cool_temperature_tasks
  add_heat_temperature_tasks
  add_fan_tasks
  add_vertical_vane_tasks
  add_extra_tasks
  add_timer_tasks
}

show_saved_codes() {
  printf '\nSaved Mitsubishi codes for model %s:\n' "$MODEL"
  curl_json "GET" "$BASE_URL/api/broadlink/mitsubishi/codes?model=$MODEL"
  printf '\n'
}

main() {
  require_var BROADLINK_HOST "$BROADLINK_HOST"

  normalize_group_filter
  init_progress_file
  load_progress_file
  login
  load_remote_saved_commands
  build_task_list

  printf 'Starting Mitsubishi capture session for model %s\n' "$MODEL"
  if [[ -n "$ONLY_GROUPS" ]]; then
    printf 'Group filter enabled: %s\n' "$ONLY_GROUPS"
  fi
  printf 'Auto advance: %s\n\n' "$AUTO_ADVANCE"

  run_tasks
  show_saved_codes

  printf '\nDone. Decode them with:\n'
  printf 'cargo run --manifest-path backend/Cargo.toml --bin decode_mitsubishi_ir -- broadlink-codes.json\n'
}

main "$@"
