const BROADLINK_TICK_US: f32 = 32.84;
const BROADLINK_IR_TOKEN: u8 = 0x26;
const MITSUBISHI_HDR_MARK_US: u16 = 3400;
const MITSUBISHI_HDR_SPACE_US: u16 = 1750;
const MITSUBISHI_BIT_MARK_US: u16 = 450;
const MITSUBISHI_ONE_SPACE_US: u16 = 1300;
const MITSUBISHI_ZERO_SPACE_US: u16 = 420;
const MITSUBISHI_REPEAT_MARK_US: u16 = 440;
const MITSUBISHI_REPEAT_GAP_US: u16 = 15500;
const MITSUBISHI_STATE_LEN: usize = 18;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Mode {
    Auto,
    Cool,
    Dry,
    Heat,
    Fan,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Fan {
    Auto,
    Level1,
    Level2,
    Level3,
    Level4,
    Silent,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Vane {
    Auto,
    Highest,
    High,
    Middle,
    Low,
    Lowest,
    Swing,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum WideVane {
    LeftMax,
    Left,
    Center,
    Right,
    RightMax,
    Wide,
    Auto,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TimerMode {
    None,
    Stop,
    Start,
    StartStop,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct MitsubishiState {
    power: bool,
    mode: Mode,
    temperature_c: u8,
    fan: Fan,
    vane: Vane,
    wide_vane: WideVane,
    i_see: bool,
    ecocool: bool,
    clock: u8,
    start_clock: u8,
    stop_clock: u8,
    timer_mode: TimerMode,
}

impl Default for MitsubishiState {
    fn default() -> Self {
        Self {
            power: true,
            mode: Mode::Cool,
            temperature_c: 20,
            fan: Fan::Auto,
            vane: Vane::Auto,
            wide_vane: WideVane::Center,
            i_see: false,
            ecocool: false,
            clock: 0,
            start_clock: 0,
            stop_clock: 0,
            timer_mode: TimerMode::None,
        }
    }
}

pub fn encode_mitsubishi_command(command: &str) -> Result<Option<Vec<u8>>, String> {
    if !command.starts_with("state-") {
        return Ok(None);
    }

    let state = parse_state_command(command)?;
    let raw = build_state_bytes(state);
    Ok(Some(encode_broadlink_packet(&raw)))
}

fn parse_state_command(command: &str) -> Result<MitsubishiState, String> {
    if command == "state-off" {
        return Ok(MitsubishiState {
            power: false,
            mode: Mode::Cool,
            temperature_c: 16,
            fan: Fan::Level4,
            vane: Vane::Swing,
            wide_vane: WideVane::Center,
            ..MitsubishiState::default()
        });
    }

    let tokens = command.split('-').collect::<Vec<_>>();
    if tokens.len() < 7 || tokens[0] != "state" {
        return Err(format!("unsupported Mitsubishi state command '{command}'"));
    }

    let mut index = 1;
    let mode = parse_mode(tokens[index])?;
    index += 1;

    let temperature_c = parse_temperature(tokens.get(index).copied())?;
    index += 1;

    expect_token(&tokens, index, "fan")?;
    index += 1;
    let fan = parse_fan(tokens.get(index).copied())?;
    index += 1;

    expect_token(&tokens, index, "vane")?;
    index += 1;
    let vane = parse_vane(tokens.get(index).copied())?;
    index += 1;

    let mut state = MitsubishiState {
        mode,
        temperature_c,
        fan,
        vane,
        ..MitsubishiState::default()
    };

    while index < tokens.len() {
        match tokens[index] {
            "wide" => {
                index += 1;
                state.wide_vane = parse_wide_vane(tokens.get(index).copied())?;
                index += 1;
            }
            "econo" => {
                index += 1;
                state.ecocool = parse_toggle(tokens.get(index).copied(), "econo")?;
                index += 1;
            }
            "isee" => {
                index += 1;
                state.i_see = parse_toggle(tokens.get(index).copied(), "isee")?;
                index += 1;
            }
            "timer" => {
                index += 1;
                let timer_off = parse_toggle(tokens.get(index).copied(), "timer")?;
                state.timer_mode = if timer_off {
                    TimerMode::Start
                } else {
                    TimerMode::None
                };
                index += 1;
                if !timer_off {
                    state.start_clock = 0;
                    state.stop_clock = 0;
                }
            }
            "start" => {
                index += 1;
                state.start_clock = parse_clock(&tokens, &mut index)?;
            }
            "stop" => {
                index += 1;
                state.stop_clock = parse_clock(&tokens, &mut index)?;
            }
            token => {
                return Err(format!(
                    "unsupported Mitsubishi state token '{token}' in '{command}'"
                ));
            }
        }
    }

    state.timer_mode = match (state.start_clock > 0, state.stop_clock > 0) {
        (false, false) => state.timer_mode,
        (true, false) => TimerMode::Start,
        (false, true) => TimerMode::Stop,
        (true, true) => TimerMode::StartStop,
    };

    if state.mode != Mode::Cool {
        state.ecocool = false;
    }

    Ok(state)
}

fn parse_mode(token: &str) -> Result<Mode, String> {
    match token {
        "auto" => Ok(Mode::Auto),
        "cool" => Ok(Mode::Cool),
        "dry" => Ok(Mode::Dry),
        "heat" => Ok(Mode::Heat),
        "fan" => Ok(Mode::Fan),
        _ => Err(format!("unsupported mode '{token}'")),
    }
}

fn parse_temperature(token: Option<&str>) -> Result<u8, String> {
    let value = token.ok_or_else(|| "missing temperature token".to_string())?;
    let temperature = value
        .parse::<u8>()
        .map_err(|_| format!("invalid temperature '{value}'"))?;
    if !(16..=31).contains(&temperature) {
        return Err(format!("temperature out of range '{value}'"));
    }
    Ok(temperature)
}

fn parse_fan(token: Option<&str>) -> Result<Fan, String> {
    match token.ok_or_else(|| "missing fan token".to_string())? {
        "auto" => Ok(Fan::Auto),
        "1" => Ok(Fan::Level1),
        "2" => Ok(Fan::Level2),
        "3" => Ok(Fan::Level3),
        "4" => Ok(Fan::Level4),
        "silent" => Ok(Fan::Silent),
        other => Err(format!("unsupported fan '{other}'")),
    }
}

fn parse_vane(token: Option<&str>) -> Result<Vane, String> {
    match token.ok_or_else(|| "missing vane token".to_string())? {
        "auto" => Ok(Vane::Auto),
        "highest" => Ok(Vane::Highest),
        "high" => Ok(Vane::High),
        "middle" => Ok(Vane::Middle),
        "low" => Ok(Vane::Low),
        "lowest" => Ok(Vane::Lowest),
        "swing" => Ok(Vane::Swing),
        other => Err(format!("unsupported vane '{other}'")),
    }
}

fn parse_wide_vane(token: Option<&str>) -> Result<WideVane, String> {
    match token.ok_or_else(|| "missing wide vane token".to_string())? {
        "left-max" => Ok(WideVane::LeftMax),
        "left" => Ok(WideVane::Left),
        "center" => Ok(WideVane::Center),
        "right" => Ok(WideVane::Right),
        "right-max" => Ok(WideVane::RightMax),
        "wide" => Ok(WideVane::Wide),
        "auto" => Ok(WideVane::Auto),
        other => Err(format!("unsupported wide vane '{other}'")),
    }
}

fn parse_toggle(token: Option<&str>, label: &str) -> Result<bool, String> {
    match token.ok_or_else(|| format!("missing {label} toggle"))? {
        "on" => Ok(true),
        "off" => Ok(false),
        other => Err(format!("unsupported {label} toggle '{other}'")),
    }
}

fn parse_clock(tokens: &[&str], index: &mut usize) -> Result<u8, String> {
    let hours = tokens
        .get(*index)
        .ok_or_else(|| "missing clock hour".to_string())?
        .parse::<u8>()
        .map_err(|_| "invalid clock hour".to_string())?;
    *index += 1;
    let minutes = tokens
        .get(*index)
        .ok_or_else(|| "missing clock minute".to_string())?
        .parse::<u8>()
        .map_err(|_| "invalid clock minute".to_string())?;
    *index += 1;

    if hours > 23 || minutes > 59 || minutes % 10 != 0 {
        return Err(format!(
            "unsupported clock value {:02}:{:02}",
            hours, minutes
        ));
    }

    Ok(hours * 6 + minutes / 10)
}

fn expect_token(tokens: &[&str], index: usize, expected: &str) -> Result<(), String> {
    match tokens.get(index).copied() {
        Some(token) if token == expected => Ok(()),
        Some(token) => Err(format!("expected '{expected}', got '{token}'")),
        None => Err(format!("missing token '{expected}'")),
    }
}

fn build_state_bytes(state: MitsubishiState) -> [u8; MITSUBISHI_STATE_LEN] {
    let mut bytes = [0_u8; MITSUBISHI_STATE_LEN];
    bytes[..5].copy_from_slice(&[0x23, 0xCB, 0x26, 0x01, 0x00]);
    bytes[5] = if state.power { 0x20 } else { 0x00 };
    bytes[6] = mode_byte(state.mode) | if state.i_see { 0x40 } else { 0x00 };
    bytes[7] = state.temperature_c.saturating_sub(16);
    bytes[8] = (wide_vane_byte(state.wide_vane) << 4) | mode_nibble(state.mode);
    bytes[9] = fan_byte(state.fan) | vane_byte(state.vane);
    bytes[10] = state.clock;
    bytes[11] = state.stop_clock;
    bytes[12] = state.start_clock;
    bytes[13] = timer_mode_byte(state.timer_mode);
    bytes[14] = if state.ecocool { 0x20 } else { 0x00 };
    bytes[17] = checksum(&bytes);
    bytes
}

fn mode_byte(mode: Mode) -> u8 {
    match mode {
        Mode::Heat => 0x08,
        Mode::Dry => 0x10,
        Mode::Cool => 0x18,
        Mode::Auto => 0x20,
        Mode::Fan => 0x38,
    }
}

fn mode_nibble(mode: Mode) -> u8 {
    match mode {
        Mode::Auto => 0x00,
        Mode::Cool => 0x06,
        Mode::Dry => 0x02,
        Mode::Heat => 0x00,
        Mode::Fan => 0x07,
    }
}

fn wide_vane_byte(wide_vane: WideVane) -> u8 {
    match wide_vane {
        WideVane::LeftMax => 0x1,
        WideVane::Left => 0x2,
        WideVane::Center => 0x3,
        WideVane::Right => 0x4,
        WideVane::RightMax => 0x5,
        WideVane::Wide => 0x6,
        WideVane::Auto => 0x8,
    }
}

fn fan_byte(fan: Fan) -> u8 {
    match fan {
        Fan::Auto => 0x80,
        Fan::Level1 => 0x01,
        Fan::Level2 => 0x02,
        Fan::Level3 => 0x03,
        Fan::Level4 => 0x04,
        Fan::Silent => 0x05,
    }
}

fn vane_byte(vane: Vane) -> u8 {
    let code = match vane {
        Vane::Auto => 0x00,
        Vane::Highest => 0x01,
        Vane::High => 0x02,
        Vane::Middle => 0x03,
        Vane::Low => 0x04,
        Vane::Lowest => 0x05,
        Vane::Swing => 0x07,
    };
    0x40 | (code << 3)
}

fn timer_mode_byte(timer_mode: TimerMode) -> u8 {
    match timer_mode {
        TimerMode::None => 0x00,
        TimerMode::Stop => 0x03,
        TimerMode::Start => 0x05,
        TimerMode::StartStop => 0x07,
    }
}

fn checksum(bytes: &[u8; MITSUBISHI_STATE_LEN]) -> u8 {
    bytes[..MITSUBISHI_STATE_LEN - 1]
        .iter()
        .fold(0_u8, |sum, byte| sum.wrapping_add(*byte))
}

fn encode_broadlink_packet(state: &[u8; MITSUBISHI_STATE_LEN]) -> Vec<u8> {
    let mut pulses = Vec::with_capacity((2 + state.len() * 16 + 1) * 2 + 1);
    append_frame(&mut pulses, state);
    pulses.push(MITSUBISHI_REPEAT_GAP_US);
    append_frame(&mut pulses, state);

    let mut packet = Vec::with_capacity(4 + pulses.len() * 2 + 2);
    packet.extend_from_slice(&[BROADLINK_IR_TOKEN, 0x00, 0x00, 0x00]);

    for pulse in pulses {
        let ticks = ((pulse as f32) / BROADLINK_TICK_US).round() as u16;
        if ticks >= 256 {
            packet.push(0x00);
            packet.push((ticks >> 8) as u8);
            packet.push((ticks & 0xFF) as u8);
        } else {
            packet.push(ticks as u8);
        }
    }

    packet.extend_from_slice(&[0x00, 0x0D]);
    let encoded_len = (packet.len() - 4 + 1) as u16;
    packet[2] = (encoded_len & 0xFF) as u8;
    packet[3] = (encoded_len >> 8) as u8;
    packet
}

fn append_frame(pulses: &mut Vec<u16>, state: &[u8; MITSUBISHI_STATE_LEN]) {
    pulses.push(MITSUBISHI_HDR_MARK_US);
    pulses.push(MITSUBISHI_HDR_SPACE_US);
    for byte in state {
        for bit in 0..8 {
            pulses.push(MITSUBISHI_BIT_MARK_US);
            let is_one = (byte >> bit) & 1 == 1;
            pulses.push(if is_one {
                MITSUBISHI_ONE_SPACE_US
            } else {
                MITSUBISHI_ZERO_SPACE_US
            });
        }
    }
    pulses.push(MITSUBISHI_REPEAT_MARK_US);
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::{engine::general_purpose::STANDARD, Engine as _};

    #[test]
    fn parses_state_command() {
        let packet = encode_mitsubishi_command(
            "state-cool-22-fan-2-vane-low-wide-center-econo-on-start-06-00-stop-11-00",
        )
        .expect("command should parse")
        .expect("state command should generate");

        let decoded = STANDARD.encode(packet);
        assert!(!decoded.is_empty());
    }

    #[test]
    fn builds_expected_bytes_for_manual_state() {
        let state = parse_state_command("state-cool-20-fan-3-vane-swing-wide-center")
            .expect("state command should parse");
        let bytes = build_state_bytes(state);

        assert_eq!(&bytes[..5], &[0x23, 0xCB, 0x26, 0x01, 0x00]);
        assert_eq!(bytes[6], 0x18);
        assert_eq!(bytes[7], 0x04);
        assert_eq!(bytes[8], 0x36);
        assert_eq!(bytes[9], 0x7B);
        assert_eq!(bytes[17], checksum(&bytes));
    }

    #[test]
    fn off_command_matches_known_state_shape() {
        let state = parse_state_command("state-off").expect("off command should parse");
        let bytes = build_state_bytes(state);

        assert_eq!(bytes[5], 0x00);
        assert_eq!(bytes[6], 0x18);
        assert_eq!(bytes[7], 0x00);
        assert_eq!(bytes[8], 0x36);
        assert_eq!(bytes[9], 0x7C);
    }
}
