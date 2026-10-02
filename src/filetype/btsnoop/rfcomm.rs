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
        let direction = if cr_bit == 1 {
            "Initiator"
        } else {
            "Responder"
        };

        if frame_type == "UIH" && length > 0 {
            let pf_bit = (control >> 4) & 0x01;
            let actual_data_offset = if pf_bit == 1 {
                data_offset + 1
            } else {
                data_offset
            };

            let payload_end =
                (data_offset + length as usize).min(l2cap_payload.len().saturating_sub(1));
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

#[cfg(test)]
mod tests {
    use super::*;

    /// RFCOMM address byte: EA=1, C/R=`cr`, DLCI=`dlci`.
    const fn addr(dlci: u8, cr: u8) -> u8 {
        (dlci << 2) | (cr << 1) | 0x01
    }

    /// FCS byte that is not valid UTF-8 on its own, so it would break HFP
    /// detection if it leaked into the payload slice.
    const FCS: u8 = 0x9A;

    #[test]
    fn rejects_short_or_non_rfcomm_payloads() {
        assert_eq!(try_parse_rfcomm(&[]), None);
        assert_eq!(try_parse_rfcomm(&[addr(2, 0), RFCOMM_UIH]), None);
        // EA bit clear on the address byte.
        assert_eq!(try_parse_rfcomm(&[0x08, RFCOMM_UIH, 0x01, FCS]), None);
        // Length EA=0 announces a second length byte that is missing.
        assert_eq!(try_parse_rfcomm(&[addr(2, 0), RFCOMM_UIH, 0x0C]), None);
    }

    #[test]
    fn control_frames_on_data_channel() {
        let cases = [
            (RFCOMM_SABM | 0x10, "RFCOMM SABM DLCI=3 Connect"),
            (RFCOMM_UA | 0x10, "RFCOMM UA DLCI=3 Ack"),
            (RFCOMM_DISC | 0x10, "RFCOMM DISC DLCI=3 Disconnect"),
            (RFCOMM_DM, "RFCOMM DM DLCI=3 Rejected"),
            (0x00, "RFCOMM Unknown DLCI=3"),
        ];
        for (control, expected) in cases {
            assert_eq!(
                try_parse_rfcomm(&[addr(3, 1), control, 0x01, FCS]).as_deref(),
                Some(expected),
                "control 0x{control:02x}"
            );
        }
    }

    #[test]
    fn dlci_zero_control_channel() {
        assert_eq!(
            try_parse_rfcomm(&[addr(0, 1), RFCOMM_SABM | 0x10, 0x01, FCS]).as_deref(),
            Some("RFCOMM SABM DLCI=0")
        );
        // UIH on DLCI 0 carries a multiplexer command; C/R bit 0x02 selects Cmd/Rsp.
        let cmd = try_parse_rfcomm(&[addr(0, 1), RFCOMM_UIH, 0x05, 0x83, 0x01, FCS])
            .expect("mux command decodes");
        assert!(
            cmd.starts_with("RFCOMM UIH MuxCtrl ") && cmd.ends_with(" Cmd"),
            "{cmd}"
        );
        let rsp = try_parse_rfcomm(&[addr(0, 1), RFCOMM_UIH, 0x05, 0x81, 0x01, FCS])
            .expect("mux response decodes");
        assert!(rsp.ends_with(" Rsp"), "{rsp}");
        // UIH on DLCI 0 without any mux byte.
        assert_eq!(
            try_parse_rfcomm(&[addr(0, 1), RFCOMM_UIH, 0x01]).as_deref(),
            Some("RFCOMM UIH DLCI=0")
        );
    }

    #[test]
    fn uih_with_one_byte_length_decodes_hfp() {
        let mut frame = vec![addr(2, 0), RFCOMM_UIH, (6 << 1) | 0x01];
        frame.extend_from_slice(b"\r\nOK\r\n");
        frame.push(FCS);
        assert_eq!(
            try_parse_rfcomm(&frame).as_deref(),
            Some("RFCOMM UIH DLCI=2 Responder HFP OK")
        );
    }

    #[test]
    fn uih_with_pf_bit_skips_credit_byte() {
        // P/F=1 on UIH means a credit byte precedes the information field.
        let mut frame = vec![addr(2, 1), RFCOMM_UIH | 0x10, (6 << 1) | 0x01, 0x05];
        frame.extend_from_slice(b"\r\nOK\r\n");
        frame.push(FCS);
        assert_eq!(
            try_parse_rfcomm(&frame).as_deref(),
            Some("RFCOMM UIH DLCI=2 Initiator HFP OK")
        );
    }

    #[test]
    fn uih_with_two_byte_length_beyond_captured_data_is_clamped() {
        // Length 200 = 0x48 | (1 << 7): first byte carries 7 bits with EA=0.
        let frame = [addr(2, 0), RFCOMM_UIH, 0x48 << 1, 0x01, b'O', b'K', FCS];
        assert_eq!(
            try_parse_rfcomm(&frame).as_deref(),
            Some("RFCOMM UIH DLCI=2 Responder HFP OK")
        );
    }

    #[test]
    fn uih_payload_end_excludes_fcs_when_length_overstates() {
        // Declared length 127 while only 6 info bytes were captured: the slice
        // must stop before the trailing FCS byte.
        let mut frame = vec![addr(2, 0), RFCOMM_UIH, 0xFF];
        frame.extend_from_slice(b"\r\nOK\r\n");
        frame.push(FCS);
        assert_eq!(
            try_parse_rfcomm(&frame).as_deref(),
            Some("RFCOMM UIH DLCI=2 Responder HFP OK")
        );
    }

    #[test]
    fn uih_non_hfp_payload_reports_length() {
        let frame = [
            addr(5, 1),
            RFCOMM_UIH,
            (3 << 1) | 0x01,
            0x00,
            0xFF,
            0x10,
            FCS,
        ];
        assert_eq!(
            try_parse_rfcomm(&frame).as_deref(),
            Some("RFCOMM UIH DLCI=5 Initiator Len=3")
        );
        // Zero-length UIH (empty credit-only frame) falls to the control-frame arm.
        assert_eq!(
            try_parse_rfcomm(&[addr(5, 1), RFCOMM_UIH | 0x10, 0x01, 0x03, FCS]).as_deref(),
            Some("RFCOMM UIH DLCI=5")
        );
    }
}
