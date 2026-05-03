// LogCrab - GPL-3.0-or-later
// Copyright (C) 2026 Daniel Freiermuth

// ============================================================================
// AVCTP/AVRCP parsing
// ============================================================================

const AVRCP_PID: u16 = 0x110E;
const AVRCP_TARGET_PID: u16 = 0x110C;

pub(super) fn try_parse_avctp(l2cap_payload: &[u8]) -> Option<String> {
    if l2cap_payload.len() < 3 {
        return None;
    }

    let header = l2cap_payload[0];
    let transaction_label = header >> 4;
    let packet_type = (header >> 2) & 0x03;
    let cr_bit = (header >> 1) & 0x01;
    let ipid = header & 0x01;

    if ipid != 0 {
        return None;
    }

    let pid = u16::from_be_bytes([l2cap_payload[1], l2cap_payload[2]]);

    if pid != AVRCP_PID && pid != AVRCP_TARGET_PID {
        return None;
    }

    let packet_type_str = match packet_type {
        0 => "Single",
        1 => "Start",
        2 => "Continue",
        3 => "End",
        _ => "Unknown",
    };

    let direction = if cr_bit == 0 { "Cmd" } else { "Rsp" };

    if l2cap_payload.len() >= 6 && (packet_type == 0 || packet_type == 1) {
        let avc_info = parse_avc_frame(&l2cap_payload[3..]);
        Some(format!(
            "AVCTP/{packet_type_str} TL={transaction_label} {direction} {avc_info}"
        ))
    } else {
        Some(format!(
            "AVCTP/{packet_type_str} TL={transaction_label} {direction}"
        ))
    }
}

fn parse_avc_frame(data: &[u8]) -> String {
    if data.len() < 3 {
        return "AV/C Truncated".to_string();
    }

    let ctype_response = data[0] & 0x0F;
    let subunit_type = (data[1] >> 3) & 0x1F;
    let subunit_id = data[1] & 0x07;
    let opcode = data[2];

    let ctype_str = get_avc_ctype(ctype_response);
    let subunit_str = get_avc_subunit(subunit_type);
    let opcode_str = get_avc_opcode(opcode);

    match opcode {
        0x7C => {
            // Pass Through
            if data.len() >= 5 {
                let operation_id = data[3] & 0x7F;
                let state = if data[3] & 0x80 != 0 {
                    "Released"
                } else {
                    "Pressed"
                };
                let op_name = get_passthrough_operation(operation_id);
                format!("AVRCP {ctype_str} {opcode_str} {op_name} {state}")
            } else {
                format!("AVRCP {ctype_str} {opcode_str}")
            }
        }
        0x00 => {
            // Vendor Dependent
            if data.len() >= 10 {
                let company_id = u32::from_be_bytes([0, data[3], data[4], data[5]]);
                if company_id != 0x00_1958 {
                    return format!(
                        "AVRCP {ctype_str} Vendor_Dependent CompanyID=0x{company_id:06X}"
                    );
                }

                let pdu_id = data[6];
                let param_type = data[7] & 0x03;
                let param_len = u16::from_be_bytes([data[8], data[9]]) as usize;
                let params = data.get(10..).unwrap_or(&[]);

                let param_type_str = match param_type {
                    0 => "",
                    1 => "/Start",
                    2 => "/Continue",
                    3 => "/End",
                    _ => "",
                };

                let pdu_name = get_avrcp_pdu_name(pdu_id);

                let detail = if param_type == 0 || param_type == 1 {
                    parse_avrcp_pdu_params(pdu_id, ctype_response, params, param_len)
                } else {
                    None
                };

                match detail {
                    Some(d) => format!("AVRCP {ctype_str} {pdu_name}{param_type_str} {d}"),
                    None => format!("AVRCP {ctype_str} {pdu_name}{param_type_str}"),
                }
            } else {
                format!("AVRCP {ctype_str} Vendor_Dependent")
            }
        }
        0x30 => format!("AVRCP {ctype_str} Unit_Info"),
        0x31 => format!("AVRCP {ctype_str} Subunit_Info {subunit_str}[{subunit_id}]"),
        _ => format!("AVRCP {ctype_str} {opcode_str}"),
    }
}

const fn get_avc_ctype(ctype: u8) -> &'static str {
    match ctype {
        0x00 => "CONTROL",
        0x01 => "STATUS",
        0x02 => "SPECIFIC_INQUIRY",
        0x03 => "NOTIFY",
        0x04 => "GENERAL_INQUIRY",
        0x08 => "NOT_IMPLEMENTED",
        0x09 => "ACCEPTED",
        0x0A => "REJECTED",
        0x0B => "IN_TRANSITION",
        0x0C => "STABLE",
        0x0D => "CHANGED",
        0x0F => "INTERIM",
        _ => "UNKNOWN",
    }
}

const fn get_avc_subunit(subunit: u8) -> &'static str {
    match subunit {
        0x00 => "Monitor",
        0x01 => "Audio",
        0x02 => "Printer",
        0x03 => "Disc",
        0x04 => "Tape_Recorder",
        0x05 => "Tuner",
        0x06 => "CA",
        0x07 => "Camera",
        0x09 => "Panel",
        0x0A => "Bulletin_Board",
        0x0B => "Camera_Storage",
        0x1C => "Vendor_Unique",
        0x1D => "Extended",
        0x1E => "Extended",
        0x1F => "Unit",
        _ => "Reserved",
    }
}

const fn get_avc_opcode(opcode: u8) -> &'static str {
    match opcode {
        0x00 => "Vendor_Dependent",
        0x30 => "Unit_Info",
        0x31 => "Subunit_Info",
        0x7C => "Pass_Through",
        0xB0 => "Get_Capabilities",
        0xB1 => "List_Player_Settings",
        0xC0 => "Continue_Response",
        0xC1 => "Abort_Continue",
        _ => "Unknown_Opcode",
    }
}

const fn get_passthrough_operation(op: u8) -> &'static str {
    match op {
        0x00 => "Select",
        0x01 => "Up",
        0x02 => "Down",
        0x03 => "Left",
        0x04 => "Right",
        0x05 => "RightUp",
        0x06 => "RightDown",
        0x07 => "LeftUp",
        0x08 => "LeftDown",
        0x09 => "RootMenu",
        0x0A => "SetupMenu",
        0x0B => "ContentsMenu",
        0x0C => "FavoriteMenu",
        0x0D => "Exit",
        0x20 => "0",
        0x21 => "1",
        0x22 => "2",
        0x23 => "3",
        0x24 => "4",
        0x25 => "5",
        0x26 => "6",
        0x27 => "7",
        0x28 => "8",
        0x29 => "9",
        0x2A => "Dot",
        0x2B => "Enter",
        0x2C => "Clear",
        0x30 => "ChannelUp",
        0x31 => "ChannelDown",
        0x32 => "PreviousChannel",
        0x33 => "SoundSelect",
        0x34 => "InputSelect",
        0x35 => "DisplayInfo",
        0x36 => "Help",
        0x37 => "PageUp",
        0x38 => "PageDown",
        0x40 => "Power",
        0x41 => "VolumeUp",
        0x42 => "VolumeDown",
        0x43 => "Mute",
        0x44 => "Play",
        0x45 => "Stop",
        0x46 => "Pause",
        0x47 => "Record",
        0x48 => "Rewind",
        0x49 => "FastForward",
        0x4A => "Eject",
        0x4B => "Forward",
        0x4C => "Backward",
        0x50 => "Angle",
        0x51 => "Subpicture",
        0x60 => "F1",
        0x61 => "F2",
        0x62 => "F3",
        0x63 => "F4",
        0x64 => "F5",
        0x71 => "VendorUnique",
        0x7E => "GroupNavigation",
        _ => "Unknown",
    }
}

fn format_ms_as_duration(ms: u32) -> String {
    let total_secs = ms / 1000;
    let hours = total_secs / 3600;
    let mins = (total_secs % 3600) / 60;
    let secs = total_secs % 60;
    if hours > 0 {
        format!("{hours}:{mins:02}:{secs:02}")
    } else {
        format!("{mins}:{secs:02}")
    }
}

fn truncate_utf8(s: &str, max_chars: usize) -> String {
    let mut chars = s.chars();
    let truncated: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        format!("{truncated}\u{2026}")
    } else {
        truncated
    }
}

fn parse_avrcp_pdu_params(
    pdu_id: u8,
    ctype: u8,
    params: &[u8],
    _param_len: usize,
) -> Option<String> {
    let is_response = ctype >= 0x08;

    match pdu_id {
        // GetCapabilities
        0x10 => {
            let cap_id = *params.first()?;
            let cap_name = match cap_id {
                0x02 => "CompanyID",
                0x03 => "EventsSupported",
                _ => "Unknown",
            };
            if is_response && params.len() >= 2 {
                let count = params[1] as usize;
                if cap_id == 0x03 && params.len() >= 2 + count {
                    let events: Vec<&str> = params[2..2 + count]
                        .iter()
                        .map(|&e| get_avrcp_notification_event(e))
                        .collect();
                    Some(format!("Cap={cap_name} [{}]", events.join(", ")))
                } else {
                    Some(format!("Cap={cap_name} Count={count}"))
                }
            } else {
                Some(format!("Cap={cap_name}"))
            }
        }
        // ListPlayerApplicationSettingAttributes
        0x11 => {
            if is_response && !params.is_empty() {
                let count = params[0] as usize;
                if params.len() > count {
                    let attrs: Vec<&str> = params[1..=count]
                        .iter()
                        .map(|&id| get_player_app_attr_name(id))
                        .collect();
                    Some(format!("Attrs=[{}]", attrs.join(", ")))
                } else {
                    Some(format!("NumAttrs={count}"))
                }
            } else {
                None
            }
        }
        // ListPlayerApplicationSettingValues
        0x12 => {
            let attr_id = *params.first()?;
            let attr_name = get_player_app_attr_name(attr_id);
            if is_response && params.len() >= 2 {
                let count = params[1] as usize;
                Some(format!("Attr={attr_name} NumValues={count}"))
            } else {
                Some(format!("Attr={attr_name}"))
            }
        }
        // GetCurrentPlayerApplicationSettingValue
        0x13 => {
            if params.is_empty() {
                return None;
            }
            let count = params[0] as usize;
            if is_response {
                let pairs: Vec<String> = (0..count)
                    .filter_map(|i| {
                        let off = 1 + i * 2;
                        let attr_id = *params.get(off)?;
                        let value_id = *params.get(off + 1)?;
                        Some(format!(
                            "{}={}",
                            get_player_app_attr_name(attr_id),
                            get_player_app_attr_value(attr_id, value_id)
                        ))
                    })
                    .collect();
                if pairs.is_empty() {
                    None
                } else {
                    Some(pairs.join(" "))
                }
            } else {
                let attrs: Vec<&str> = (0..count)
                    .filter_map(|i| params.get(1 + i).map(|&id| get_player_app_attr_name(id)))
                    .collect();
                Some(format!("Query=[{}]", attrs.join(", ")))
            }
        }
        // SetPlayerApplicationSettingValue
        0x14 => {
            if is_response || params.is_empty() {
                return None;
            }
            let count = params[0] as usize;
            let pairs: Vec<String> = (0..count)
                .filter_map(|i| {
                    let off = 1 + i * 2;
                    let attr_id = *params.get(off)?;
                    let value_id = *params.get(off + 1)?;
                    Some(format!(
                        "{}={}",
                        get_player_app_attr_name(attr_id),
                        get_player_app_attr_value(attr_id, value_id)
                    ))
                })
                .collect();
            if pairs.is_empty() {
                None
            } else {
                Some(pairs.join(" "))
            }
        }
        // InformBatteryStatus
        0x18 => {
            let status = match *params.first()? {
                0x00 => "NORMAL",
                0x01 => "WARNING",
                0x02 => "CRITICAL",
                0x03 => "EXTERNAL",
                0x04 => "FULL_CHARGE",
                _ => "UNKNOWN",
            };
            Some(format!("Battery={status}"))
        }
        // GetElementAttributes
        0x20 => {
            if is_response {
                let count = *params.first()? as usize;
                if count == 0 {
                    return Some("NoAttributes".to_string());
                }
                let mut result: Vec<String> = Vec::new();
                let mut offset = 1usize;
                for _ in 0..count {
                    if params.len() < offset + 8 {
                        break;
                    }
                    let attr_id = u32::from_be_bytes([
                        params[offset],
                        params[offset + 1],
                        params[offset + 2],
                        params[offset + 3],
                    ]);
                    let length =
                        u16::from_be_bytes([params[offset + 6], params[offset + 7]]) as usize;
                    offset += 8;
                    if params.len() < offset + length {
                        break;
                    }
                    let value = String::from_utf8_lossy(&params[offset..offset + length]);
                    result.push(format!(
                        "{}=\"{}\"",
                        get_element_attr_name(attr_id),
                        truncate_utf8(&value, 40)
                    ));
                    offset += length;
                    if result.len() >= 3 {
                        let remaining = count - result.len();
                        if remaining > 0 {
                            result.push(format!("+{remaining} more"));
                        }
                        break;
                    }
                }
                if result.is_empty() {
                    None
                } else {
                    Some(result.join(" "))
                }
            } else {
                if params.len() < 9 {
                    return None;
                }
                let uid = u64::from_be_bytes([
                    params[0], params[1], params[2], params[3], params[4], params[5], params[6],
                    params[7],
                ]);
                let num_attrs = params[8] as usize;
                let track = if uid == 0 {
                    "CurrentTrack".to_string()
                } else {
                    format!("UID=0x{uid:016X}")
                };
                if num_attrs == 0 {
                    Some(format!("{track} AllAttrs"))
                } else {
                    let attr_names: Vec<&str> = (0..num_attrs)
                        .filter_map(|i| {
                            let off = 9 + i * 4;
                            if params.len() < off + 4 {
                                return None;
                            }
                            let id = u32::from_be_bytes([
                                params[off],
                                params[off + 1],
                                params[off + 2],
                                params[off + 3],
                            ]);
                            Some(get_element_attr_name(id))
                        })
                        .collect();
                    Some(format!("{track} Attrs=[{}]", attr_names.join(", ")))
                }
            }
        }
        // GetPlayStatus
        0x30 => {
            if params.len() >= 9 {
                let song_length_ms =
                    u32::from_be_bytes([params[0], params[1], params[2], params[3]]);
                let song_pos_ms = u32::from_be_bytes([params[4], params[5], params[6], params[7]]);
                let play_status = match params[8] {
                    0x00 => "STOPPED",
                    0x01 => "PLAYING",
                    0x02 => "PAUSED",
                    0x03 => "FWD_SEEK",
                    0x04 => "REV_SEEK",
                    0xFF => "ERROR",
                    _ => "UNKNOWN",
                };
                let pos_str = if song_pos_ms == 0xFFFF_FFFF {
                    "?".to_string()
                } else {
                    format_ms_as_duration(song_pos_ms)
                };
                let len_str = if song_length_ms == 0xFFFF_FFFF {
                    "?".to_string()
                } else {
                    format_ms_as_duration(song_length_ms)
                };
                Some(format!("Status={play_status} Pos={pos_str}/{len_str}"))
            } else {
                None
            }
        }
        // RegisterNotification
        0x31 => {
            let event_id = *params.first()?;
            let event_name = get_avrcp_notification_event(event_id);
            if is_response {
                let extra = decode_notification_response(event_id, params.get(1..).unwrap_or(&[]));
                match extra {
                    Some(e) => Some(format!("Event={event_name} {e}")),
                    None => Some(format!("Event={event_name}")),
                }
            } else if event_id == 0x05 && params.len() >= 5 {
                let interval_ms = u32::from_be_bytes([params[1], params[2], params[3], params[4]]);
                Some(format!("Event={event_name} Interval={interval_ms}ms"))
            } else {
                Some(format!("Event={event_name}"))
            }
        }
        // RequestContinuingResponse / AbortContinuingResponse
        0x40 | 0x41 => {
            let for_pdu = *params.first()?;
            Some(format!("For={}", get_avrcp_pdu_name(for_pdu)))
        }
        // SetAbsoluteVolume
        0x50 => {
            let raw = *params.first()? & 0x7F;
            let pct = (u32::from(raw) * 100) / 127;
            Some(format!("Volume={pct}% (0x{raw:02X})"))
        }
        // SetAddressedPlayer
        0x60 => {
            if is_response {
                let status = get_avrcp_error_status(*params.first()?);
                Some(format!("Status={status}"))
            } else if params.len() >= 2 {
                let player_id = u16::from_be_bytes([params[0], params[1]]);
                Some(format!("PlayerID={player_id}"))
            } else {
                None
            }
        }
        // SetBrowsedPlayer
        0x70 => {
            if is_response {
                if params.len() >= 11 {
                    let status = u16::from_be_bytes([params[0], params[1]]);
                    let num_items =
                        u32::from_be_bytes([params[4], params[5], params[6], params[7]]);
                    let folder_depth = params[10];
                    let status_name = if status == 0x0004 { "Success" } else { "Error" };
                    Some(format!(
                        "Status={status_name} Items={num_items} Depth={folder_depth}"
                    ))
                } else {
                    None
                }
            } else if params.len() >= 2 {
                let player_id = u16::from_be_bytes([params[0], params[1]]);
                Some(format!("PlayerID={player_id}"))
            } else {
                None
            }
        }
        // GetFolderItems
        0x71 => {
            if is_response {
                if params.len() >= 6 {
                    let status = u16::from_be_bytes([params[0], params[1]]);
                    let num_items = u16::from_be_bytes([params[4], params[5]]);
                    let status_name = if status == 0x0004 { "Success" } else { "Error" };
                    Some(format!("Status={status_name} NumItems={num_items}"))
                } else {
                    None
                }
            } else {
                let scope = get_folder_scope_name(*params.first()?);
                if params.len() >= 9 {
                    let start = u32::from_be_bytes([params[1], params[2], params[3], params[4]]);
                    let end = u32::from_be_bytes([params[5], params[6], params[7], params[8]]);
                    Some(format!("Scope={scope} Items={start}..={end}"))
                } else {
                    Some(format!("Scope={scope}"))
                }
            }
        }
        // ChangePath
        0x72 => {
            if is_response {
                if params.len() >= 6 {
                    let num_items =
                        u32::from_be_bytes([params[2], params[3], params[4], params[5]]);
                    Some(format!("NumItems={num_items}"))
                } else {
                    None
                }
            } else if params.len() >= 3 {
                let direction = match params[2] {
                    0x00 => "Up",
                    0x01 => "Down",
                    _ => "Unknown",
                };
                Some(format!("Direction={direction}"))
            } else {
                None
            }
        }
        // PlayItem
        0x74 => {
            if is_response {
                let status = get_avrcp_error_status(*params.first()?);
                Some(format!("Status={status}"))
            } else {
                let scope = get_folder_scope_name(*params.first()?);
                Some(format!("Scope={scope}"))
            }
        }
        // Search
        0x80 => {
            if is_response {
                if params.len() >= 8 {
                    let num_items =
                        u32::from_be_bytes([params[4], params[5], params[6], params[7]]);
                    Some(format!("NumItems={num_items}"))
                } else {
                    None
                }
            } else if params.len() >= 4 {
                let length = u16::from_be_bytes([params[2], params[3]]) as usize;
                if params.len() >= 4 + length && length > 0 {
                    let query = String::from_utf8_lossy(&params[4..4 + length]);
                    Some(format!("Query=\"{}\"", truncate_utf8(&query, 40)))
                } else {
                    None
                }
            } else {
                None
            }
        }
        _ => None,
    }
}

fn decode_notification_response(event_id: u8, data: &[u8]) -> Option<String> {
    match event_id {
        0x01 => {
            let status = match *data.first()? {
                0x00 => "STOPPED",
                0x01 => "PLAYING",
                0x02 => "PAUSED",
                0x03 => "FWD_SEEK",
                0x04 => "REV_SEEK",
                0xFF => "ERROR",
                _ => "UNKNOWN",
            };
            Some(format!("Status={status}"))
        }
        0x02 => {
            if data.len() >= 8 {
                let uid = u64::from_be_bytes([
                    data[0], data[1], data[2], data[3], data[4], data[5], data[6], data[7],
                ]);
                if uid == u64::MAX {
                    Some("Track=None".to_string())
                } else if uid == 0 {
                    Some("Track=Selected".to_string())
                } else {
                    Some(format!("UID=0x{uid:016X}"))
                }
            } else {
                None
            }
        }
        0x05 => {
            if data.len() >= 4 {
                let pos_ms = u32::from_be_bytes([data[0], data[1], data[2], data[3]]);
                if pos_ms == u32::MAX {
                    Some("Pos=?".to_string())
                } else {
                    Some(format!("Pos={}", format_ms_as_duration(pos_ms)))
                }
            } else {
                None
            }
        }
        0x06 => {
            let status = match *data.first()? {
                0x00 => "NORMAL",
                0x01 => "WARNING",
                0x02 => "CRITICAL",
                0x03 => "EXTERNAL",
                0x04 => "FULL_CHARGE",
                _ => "UNKNOWN",
            };
            Some(format!("Battery={status}"))
        }
        0x07 => {
            let status = match *data.first()? {
                0x00 => "POWER_ON",
                0x01 => "POWER_OFF",
                0x02 => "UNPLUGGED",
                _ => "UNKNOWN",
            };
            Some(format!("System={status}"))
        }
        0x0B => {
            if data.len() >= 4 {
                let player_id = u16::from_be_bytes([data[0], data[1]]);
                let uid_counter = u16::from_be_bytes([data[2], data[3]]);
                Some(format!("PlayerID={player_id} UIDCounter={uid_counter}"))
            } else {
                None
            }
        }
        0x0C => {
            if data.len() >= 2 {
                let uid_counter = u16::from_be_bytes([data[0], data[1]]);
                Some(format!("UIDCounter={uid_counter}"))
            } else {
                None
            }
        }
        0x0D => {
            let raw = *data.first()? & 0x7F;
            let pct = (u32::from(raw) * 100) / 127;
            Some(format!("Volume={pct}% (0x{raw:02X})"))
        }
        _ => None,
    }
}

const fn get_player_app_attr_name(attr_id: u8) -> &'static str {
    match attr_id {
        0x01 => "Equalizer",
        0x02 => "Repeat",
        0x03 => "Shuffle",
        0x04 => "Scan",
        _ => "Attr?",
    }
}

const fn get_player_app_attr_value(attr_id: u8, value_id: u8) -> &'static str {
    match (attr_id, value_id) {
        (0x01..=0x04, 0x01) => "OFF",
        (0x01, 0x02) => "ON",
        (0x02..=0x04, 0x02) => "All",
        (0x02, 0x03) => "Single",
        (0x02, 0x04) | (0x03 | 0x04, 0x03) => "Group",
        _ => "?",
    }
}

const fn get_element_attr_name(attr_id: u32) -> &'static str {
    match attr_id {
        0x01 => "Title",
        0x02 => "Artist",
        0x03 => "Album",
        0x04 => "TrackNum",
        0x05 => "NumTracks",
        0x06 => "Genre",
        0x07 => "Duration",
        _ => "Attr?",
    }
}

const fn get_folder_scope_name(scope: u8) -> &'static str {
    match scope {
        0x00 => "MediaPlayerList",
        0x01 => "VirtualFilesystem",
        0x02 => "SearchResultList",
        0x03 => "NowPlayingList",
        _ => "Unknown",
    }
}

const fn get_avrcp_error_status(status: u8) -> &'static str {
    match status {
        0x00 => "INVALID_COMMAND",
        0x01 => "INVALID_PARAMETER",
        0x02 => "PARAM_NOT_FOUND",
        0x03 => "INTERNAL_ERROR",
        0x04 => "SUCCESS",
        0x05 => "UID_CHANGED",
        0x06 => "RESERVED",
        0x07 => "INVALID_DIRECTION",
        0x08 => "NOT_A_DIRECTORY",
        0x09 => "DOES_NOT_EXIST",
        0x0A => "INVALID_SCOPE",
        0x0B => "RANGE_OUT_OF_BOUNDS",
        0x0C => "UID_IS_A_DIRECTORY",
        0x0D => "MEDIA_IN_USE",
        0x0E => "NOW_PLAYING_LIST_FULL",
        0x0F => "SEARCH_NOT_SUPPORTED",
        0x10 => "SEARCH_IN_PROGRESS",
        0x11 => "INVALID_PLAYER_ID",
        0x12 => "PLAYER_NOT_BROWSABLE",
        0x13 => "PLAYER_NOT_ADDRESSED",
        0x14 => "NO_VALID_SEARCH_RESULTS",
        0x15 => "NO_AVAILABLE_PLAYERS",
        0x16 => "ADDRESSED_PLAYER_CHANGED",
        _ => "UNKNOWN",
    }
}

const fn get_avrcp_notification_event(event_id: u8) -> &'static str {
    match event_id {
        0x01 => "PLAYBACK_STATUS_CHANGED",
        0x02 => "TRACK_CHANGED",
        0x03 => "TRACK_REACHED_END",
        0x04 => "TRACK_REACHED_START",
        0x05 => "PLAYBACK_POS_CHANGED",
        0x06 => "BATT_STATUS_CHANGED",
        0x07 => "SYSTEM_STATUS_CHANGED",
        0x08 => "PLAYER_APP_SETTING_CHANGED",
        0x09 => "NOW_PLAYING_CONTENT_CHANGED",
        0x0A => "AVAILABLE_PLAYERS_CHANGED",
        0x0B => "ADDRESSED_PLAYER_CHANGED",
        0x0C => "UIDS_CHANGED",
        0x0D => "VOLUME_CHANGED",
        _ => "UNKNOWN_EVENT",
    }
}

const fn get_avrcp_pdu_name(pdu_id: u8) -> &'static str {
    match pdu_id {
        0x10 => "GetCapabilities",
        0x11 => "ListPlayerAppSettingAttr",
        0x12 => "ListPlayerAppSettingValues",
        0x13 => "GetCurrentPlayerAppSettingValue",
        0x14 => "SetPlayerAppSettingValue",
        0x15 => "GetPlayerAppSettingAttrText",
        0x16 => "GetPlayerAppSettingValueText",
        0x17 => "InformDisplayableCharSet",
        0x18 => "InformBatteryStatus",
        0x20 => "GetElementAttributes",
        0x30 => "GetPlayStatus",
        0x31 => "RegisterNotification",
        0x40 => "RequestContinuingResponse",
        0x41 => "AbortContinuingResponse",
        0x50 => "SetAbsoluteVolume",
        0x60 => "SetAddressedPlayer",
        0x70 => "SetBrowsedPlayer",
        0x71 => "GetFolderItems",
        0x72 => "ChangePath",
        0x73 => "GetItemAttributes",
        0x74 => "PlayItem",
        0x75 => "GetTotalNumberOfItems",
        0x80 => "Search",
        0x90 => "AddToNowPlaying",
        _ => "Unknown_PDU",
    }
}
