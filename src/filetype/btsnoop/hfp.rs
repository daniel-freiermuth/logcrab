// LogCrab - GPL-3.0-or-later
// Copyright (C) 2026 Daniel Freiermuth

// ============================================================================
// HFP (Hands-Free Profile) AT command parsing
// ============================================================================

/// Try to parse RFCOMM payload as HFP AT commands.
pub(super) fn try_parse_hfp_at_command(payload: &[u8]) -> Option<String> {
    let text = std::str::from_utf8(payload).ok()?;
    let text = text.trim();

    if text.is_empty() {
        return None;
    }

    if text.starts_with("AT")
        || text.starts_with('+')
        || text == "OK"
        || text == "ERROR"
        || text.starts_with("RING")
    {
        let cmd_info = parse_hfp_command(text);
        Some(format!("HFP {cmd_info}"))
    } else {
        None
    }
}

fn parse_hfp_command(cmd: &str) -> String {
    if cmd == "OK" {
        return "OK".to_string();
    }
    if cmd == "ERROR" {
        return "ERROR".to_string();
    }
    if cmd.starts_with("RING") {
        return "RING (Incoming call)".to_string();
    }

    if let Some(rest) = cmd.strip_prefix('+') {
        return parse_hfp_unsolicited(rest);
    }

    if let Some(rest) = cmd.strip_prefix("AT") {
        return parse_hfp_at_command_str(rest);
    }

    cmd.to_string()
}

fn parse_hfp_unsolicited(code: &str) -> String {
    let (name, params) = code.find([':', '=']).map_or((code, None), |idx| {
        let (n, p) = code.split_at(idx);
        (n, Some(&p[1..]))
    });

    let desc = match name {
        "BRSF" => "Supported Features",
        "CIND" => "Indicator Status",
        "CIEV" => "Indicator Event",
        "CHLD" => "Call Hold Options",
        "COPS" => "Operator Name",
        "CLCC" => "Call List",
        "CLIP" => "Calling Line ID",
        "CCWA" => "Call Waiting",
        "BVRA" => "Voice Recognition",
        "VGS" => "Speaker Gain",
        "VGM" => "Microphone Gain",
        "BSIR" => "In-band Ring Tone",
        "BTRH" => "Response and Hold",
        "BINP" => "Phone Number",
        "BCS" => "Codec Selection",
        "CNUM" => "Subscriber Number",
        "CME ERROR" => "Extended Error",
        _ => name,
    };

    params.map_or_else(|| format!("+{desc}"), |p| format!("+{desc}: {p}"))
}

fn parse_hfp_at_command_str(cmd: &str) -> String {
    if let Some(name) = cmd.strip_suffix("=?") {
        let desc = get_at_command_name(name);
        return format!("AT{desc}=? (Test)");
    }

    if let Some(name) = cmd.strip_suffix('?') {
        let desc = get_at_command_name(name);
        return format!("AT{desc}? (Read)");
    }

    if let Some(idx) = cmd.find('=') {
        let (name, value) = cmd.split_at(idx);
        let desc = get_at_command_name(name);
        return format!("AT{desc}{value}");
    }

    let desc = get_at_command_name(cmd);
    format!("AT{desc}")
}

fn get_at_command_name(cmd: &str) -> &str {
    match cmd {
        "+BRSF" => "+BRSF (Supported Features)",
        "+CIND" => "+CIND (Indicators)",
        "+CMER" => "+CMER (Event Reporting)",
        "+CHLD" => "+CHLD (Call Hold)",
        "+CHUP" => "+CHUP (Hang Up)",
        "+CLCC" => "+CLCC (Call List)",
        "+COPS" => "+COPS (Operator)",
        "+CLIP" => "+CLIP (Caller ID)",
        "+CCWA" => "+CCWA (Call Waiting)",
        "+CMEE" => "+CMEE (Extended Errors)",
        "+BVRA" => "+BVRA (Voice Recognition)",
        "+NREC" => "+NREC (Noise Reduction)",
        "+VGS" => "+VGS (Speaker Gain)",
        "+VGM" => "+VGM (Mic Gain)",
        "+BINP" => "+BINP (Phone Number)",
        "+BLDN" => "+BLDN (Last Number Redial)",
        "+BTRH" => "+BTRH (Response Hold)",
        "+CNUM" => "+CNUM (Subscriber Number)",
        "+BIA" => "+BIA (Indicator Activation)",
        "+BAC" => "+BAC (Available Codecs)",
        "+BCC" => "+BCC (Codec Connection)",
        "+BCS" => "+BCS (Codec Selection)",
        "+BIND" => "+BIND (HF Indicators)",
        "+BIEV" => "+BIEV (HF Indicator Value)",
        "A" => "A (Answer)",
        "D" => "D (Dial)",
        "H" => "H (Hang Up)",
        _ => cmd,
    }
}
