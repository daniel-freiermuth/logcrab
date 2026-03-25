// LogCrab - GPL-3.0-or-later
// Copyright (C) 2026 Daniel Freiermuth

// ============================================================================
// RFCOMM frame parsing
// ============================================================================

const RFCOMM_SABM: u8 = 0x2F;
const RFCOMM_UA: u8 = 0x63;
const RFCOMM_DM: u8 = 0x0F;
const RFCOMM_DISC: u8 = 0x43;
const RFCOMM_UIH: u8 = 0xEF;

const fn get_rfcomm_frame_type(control: u8) -> &'static str {
    let frame_type = control & !0x10;
    match frame_type {
        RFCOMM_SABM => "SABM",
        RFCOMM_UA => "UA",
        RFCOMM_DM => "DM",
        RFCOMM_DISC => "DISC",
        RFCOMM_UIH => "UIH",
        _ => "Unknown",
    }
}

const fn get_rfcomm_mux_cmd(cmd_type: u8) -> &'static str {
    let cmd = cmd_type >> 2;
    match cmd {
        0x08 => "PN",
        0x14 => "PSC",
        0x04 => "CLD",
        0x18 => "Test",
        0x02 => "FCoff",
        0x0A => "FCon",
        0x0E => "MSC",
        0x12 => "NSC",
        0x11 => "RPN",
        0x09 => "RLS",
        0x20 => "SNC",
        _ => "Unknown_MuxCmd",
    }
}

pub(super) fn try_parse_rfcomm(l2cap_payload: &[u8]) -> Option<String> {
    if l2cap_payload.len() < 3 {
        return None;
    }

    let address = l2cap_payload[0];
    let control = l2cap_payload[1];

    if address & 0x01 != 1 {
        return None;
    }

    let dlci = address >> 2;
    let cr_bit = (address >> 1) & 0x01;
    let frame_type = get_rfcomm_frame_type(control);

    let (length, data_offset) = if l2cap_payload[2] & 0x01 == 1 {
        (u16::from(l2cap_payload[2] >> 1), 3)
    } else if l2cap_payload.len() >= 4 {
        let len = u16::from(l2cap_payload[2] >> 1) | (u16::from(l2cap_payload[3]) << 7);
        (len, 4)
    } else {
        return None;
    };

    let info = if dlci == 0 {
        if frame_type == "UIH" && l2cap_payload.len() > data_offset {
            let mux_type = l2cap_payload[data_offset];
            let mux_cmd = get_rfcomm_mux_cmd(mux_type);
            let cr = if mux_type & 0x02 != 0 { "Cmd" } else { "Rsp" };
            format!("MuxCtrl {mux_cmd} {cr}")
        } else {
            "DLCI=0".to_string()
        }
    } else {
        let direction = if cr_bit == 1 { "Initiator" } else { "Responder" };

        if frame_type == "UIH" && length > 0 {
            let pf_bit = (control >> 4) & 0x01;
            let actual_data_offset = if pf_bit == 1 {
                data_offset + 1
            } else {
                data_offset
            };

            let payload_end = (data_offset + length as usize)
                .min(l2cap_payload.len().saturating_sub(1));
            if actual_data_offset < payload_end {
                let payload = &l2cap_payload[actual_data_offset..payload_end];
                if let Some(hfp_info) = super::hfp::try_parse_hfp_at_command(payload) {
                    return Some(format!("RFCOMM UIH DLCI={dlci} {direction} {hfp_info}"));
                }
            }

            format!("DLCI={dlci} {direction} Len={length}")
        } else {
            match frame_type {
                "SABM" => format!("DLCI={dlci} Connect"),
                "UA" => format!("DLCI={dlci} Ack"),
                "DISC" => format!("DLCI={dlci} Disconnect"),
                "DM" => format!("DLCI={dlci} Rejected"),
                _ => format!("DLCI={dlci}"),
            }
        }
    };

    Some(format!("RFCOMM {frame_type} {info}"))
}
