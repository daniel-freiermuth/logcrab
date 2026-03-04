// LogCrab - GPL-3.0-or-later
// Copyright (C) 2026 Daniel Freiermuth

use chrono::{DateTime, Local, TimeZone};
use egui::Ui;
use pcap_parser::traits::PcapReaderIterator;
use pcap_parser::{LegacyPcapReader, PcapBlockOwned, PcapError, PcapNGReader};
use std::collections::HashMap;
use std::fs::File;
use std::io::BufReader;
use std::path::Path;

use crate::filetype::{BinaryFileType, InputFileType, LineType};

// ============================================================================
// PcapLogLine
// ============================================================================

/// PCAP (Packet Capture) format log line representing a network packet
#[derive(Debug, Clone)]
pub struct PcapLogLine {
    /// Parsed packet information
    pub packet_info: PacketInfo,
    /// Original packet number in source file
    pub line_number: usize,
    /// Anomaly score (mutable)
    pub anomaly_score: f64,
}

impl PcapLogLine {
    pub const fn new(packet_info: PacketInfo, line_number: usize) -> Self {
        Self {
            packet_info,
            line_number,
            anomaly_score: 0.0,
        }
    }
}

// ============================================================================
// PcapFileState
// ============================================================================

/// Type alias kept for compatibility; the shared [`crate::filetype::SimpleFileState`]
/// provides all interior-mutable time-offset and calibration state.
pub type PcapFileState = crate::filetype::SimpleFileState;

// ============================================================================
// LineType implementation
// ============================================================================

impl LineType for PcapLogLine {
    type Config = ();
    type FileState = PcapFileState;

    fn file_state_from_v2(time_offset_ms: i64) -> PcapFileState {
        let s = PcapFileState::default();
        s.set_time_offset_ms(time_offset_ms);
        s
    }

    fn timestamp(&self, _config: &(), file_state: &PcapFileState) -> DateTime<Local> {
        self.packet_info.timestamp + chrono::Duration::milliseconds(file_state.time_offset_ms())
    }

    fn message(&self) -> String {
        self.packet_info.format_message()
    }

    fn display_message(&self, _config: &(), file_state: &PcapFileState) -> String {
        let offset_ms = file_state.time_offset_ms();
        if offset_ms != 0 {
            format!("[{}] {}", crate::parser::format_time_diff(chrono::Duration::milliseconds(offset_ms)), self.message())
        } else {
            self.packet_info.format_message()
        }
    }

    fn raw(&self) -> String {
        self.packet_info.format_raw()
    }

    fn line_number(&self) -> usize {
        self.line_number
    }

    fn anomaly_score(&self) -> f64 {
        self.anomaly_score
    }

    fn set_anomaly_score(&mut self, score: f64) {
        self.anomaly_score = score;
    }

    fn egui_render_context_menu(
        &self,
        ui: &mut Ui,
        _config: &(),
        file_state: &PcapFileState,
    ) {
        if ui.button("⏱ Calibrate Time Here").clicked() {
            let raw_time = self.packet_info.timestamp;
            let display_time =
                raw_time + chrono::Duration::milliseconds(file_state.time_offset_ms());
            *file_state.calibration.lock().expect("calibration lock poisoned") = Some((
                raw_time,
                crate::filetype::CalibrationWindow::new(display_time, false, Some(display_time), None),
            ));
            ui.close();
        }
    }

}

// ============================================================================
// PcapFileType (InputFileType + BinaryFileType)
// ============================================================================

/// Stateful reader for packet captures in both classic pcap and pcapng formats.
///
/// All packets are parsed eagerly at `open()` time via the streaming `pcap_parser`
/// crate, then drained in chunks via `read()`.
pub struct PcapFileType {
    lines: Vec<PcapLogLine>,
    cursor: usize,
    file_size: u64,
}

impl PcapFileType {
    pub const fn file_size(&self) -> u64 {
        self.file_size
    }
}

impl InputFileType for PcapFileType {
    type LineType = PcapLogLine;

    const FILE_EXTENSIONS: &'static [&'static str] = &["pcap", "pcapng", "cap"];

    /// Open a pcap/pcapng file for pull-based reading.
    fn open(path: &Path, _config: (), _file_state: std::sync::Arc<PcapFileState>) -> Result<Self, String> {
        let file_size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
        let lines = parse_pcap_to_lines(path)?;
        Ok(Self {
            lines,
            cursor: 0,
            file_size,
        })
    }

    fn read(&mut self, lines_to_read: usize) -> Result<Vec<Self::LineType>, String> {
        let end = (self.cursor + lines_to_read).min(self.lines.len());
        let batch = self.lines[self.cursor..end].to_vec();
        self.cursor = end;
        Ok(batch)
    }

    fn bytes_consumed(&self) -> u64 {
        let total = self.lines.len();
        if total == 0 {
            return self.file_size;
        }
        (self.cursor as f64 / total as f64 * self.file_size as f64) as u64
    }
}

impl BinaryFileType for PcapFileType {
    /// All magic byte patterns for classic pcap (LE/BE, normal/nanosec) and pcapng.
    const MAGIC_BYTES: &'static [&'static [u8]] = &[
        &[0xd4, 0xc3, 0xb2, 0xa1], // classic pcap, little-endian
        &[0xa1, 0xb2, 0xc3, 0xd4], // classic pcap, big-endian
        &[0x4d, 0x3c, 0xb2, 0xa1], // nanosec pcap, little-endian
        &[0xa1, 0xb2, 0x3c, 0x4d], // nanosec pcap, big-endian
        &[0x0a, 0x0d, 0x0d, 0x0a], // pcapng Section Header Block
    ];
}

// ============================================================================
// Pcap parsing utilities (moved from parser/pcap.rs)
// ============================================================================

/// Represents a parsed network packet for display
#[derive(Debug, Clone)]
pub struct PacketInfo {
    pub timestamp: DateTime<Local>,
    pub src_addr: String,
    pub src_port: Option<u16>,
    pub dst_addr: String,
    pub dst_port: Option<u16>,
    pub protocol: String,
    pub vlan_id: Option<u16>,
    pub length: u32,
    pub info: String,
    pub tcp_details: Option<TcpDetails>,
    pub is_abnormal: bool,
}

/// TCP-specific packet details
#[derive(Debug, Clone)]
pub struct TcpDetails {
    pub seq: u32,
    pub ack: u32,
    pub flags: u8,
    pub window: u16,
    pub payload_len: u32,
}

impl PacketInfo {
    /// Format as a display message
    pub fn format_message(&self) -> String {
        let src = self.src_port.map_or_else(
            || self.src_addr.clone(),
            |port| format!("{}:{}", self.src_addr, port),
        );
        let dst = self.dst_port.map_or_else(
            || self.dst_addr.clone(),
            |port| format!("{}:{}", self.dst_addr, port),
        );
        let vlan = self
            .vlan_id
            .map_or(String::new(), |id| format!(" [VLAN {id}]"));
        let abnormal = if self.is_abnormal { " \u{26a0}" } else { "" };
        self.tcp_details.as_ref().map_or_else(
            || {
                if self.info.is_empty() {
                    format!("{} {} \u{2192} {}{} Len={}{}", self.protocol, src, dst, vlan, self.length, abnormal)
                } else {
                    format!("{} {} \u{2192} {}{} {} Len={}{}", self.protocol, src, dst, vlan, self.info, self.length, abnormal)
                }
            },
            |tcp| {
                let flags_str = format_tcp_flags(tcp.flags);
                let seq_str = format!("Seq={}", tcp.seq);
                let ack_str = if tcp.flags & 0x10 != 0 { format!(" Ack={}", tcp.ack) } else { String::new() };
                let win_str = format!(" Win={}", tcp.window);
                let len_str = if tcp.payload_len > 0 { format!(" Len={}", tcp.payload_len) } else { String::new() };
                format!("{} {} \u{2192} {}{} {} {}{}{}{}{}", self.protocol, src, dst, vlan, flags_str, seq_str, ack_str, win_str, len_str, abnormal)
            },
        )
    }

    /// Format as raw line (more detailed)
    pub fn format_raw(&self) -> String {
        let src = self.src_port.map_or_else(
            || self.src_addr.clone(),
            |port| format!("{}:{}", self.src_addr, port),
        );
        let dst = self.dst_port.map_or_else(
            || self.dst_addr.clone(),
            |port| format!("{}:{}", self.dst_addr, port),
        );
        let vlan = self.vlan_id.map_or(String::new(), |id| format!(" VLAN={id}"));
        let abnormal = if self.is_abnormal { " [ABNORMAL]" } else { "" };
        self.tcp_details.as_ref().map_or_else(
            || format!("[{}] {} {} \u{2192} {}{} {} Length={}{}", self.timestamp.format("%H:%M:%S%.6f"), self.protocol, src, dst, vlan, self.info, self.length, abnormal),
            |tcp| {
                let flags_str = format_tcp_flags(tcp.flags);
                let seq_str = format!("Seq={}", tcp.seq);
                let ack_str = if tcp.flags & 0x10 != 0 { format!(" Ack={}", tcp.ack) } else { String::new() };
                let win_str = format!(" Win={}", tcp.window);
                let len_str = if tcp.payload_len > 0 { format!(" Len={}", tcp.payload_len) } else { String::new() };
                format!("[{}] {} {} \u{2192} {}{} {} {}{}{}{}{}", self.timestamp.format("%H:%M:%S%.6f"), self.protocol, src, dst, vlan, flags_str, seq_str, ack_str, win_str, len_str, abnormal)
            },
        )
    }
}

fn format_tcp_flags(flags: u8) -> String {
    let mut flag_strs = Vec::new();
    if flags & 0x02 != 0 { flag_strs.push("SYN"); }
    if flags & 0x10 != 0 { flag_strs.push("ACK"); }
    if flags & 0x01 != 0 { flag_strs.push("FIN"); }
    if flags & 0x04 != 0 { flag_strs.push("RST"); }
    if flags & 0x08 != 0 { flag_strs.push("PSH"); }
    if flags & 0x20 != 0 { flag_strs.push("URG"); }
    if flag_strs.is_empty() { "[]".to_string() } else { format!("[{}]", flag_strs.join(",")) }
}

// ============================================================================
// TCP Flow Tracking
// ============================================================================

#[derive(Debug, Clone, Hash, Eq, PartialEq)]
struct FlowKey {
    src_addr: String,
    src_port: u16,
    dst_addr: String,
    dst_port: u16,
}

impl FlowKey {
    const fn new(src_addr: String, src_port: u16, dst_addr: String, dst_port: u16) -> Self {
        Self { src_addr, src_port, dst_addr, dst_port }
    }
    fn reverse(&self) -> Self {
        Self { src_addr: self.dst_addr.clone(), src_port: self.dst_port, dst_addr: self.src_addr.clone(), dst_port: self.src_port }
    }
}

#[derive(Debug, Clone)]
struct TcpFlowState {
    next_seq: u32,
    last_ack: u32,
    dup_ack_count: u8,
    recent_seqs: Vec<(u32, u32)>,
}

impl TcpFlowState {
    fn new() -> Self {
        Self { next_seq: 0, last_ack: 0, dup_ack_count: 0, recent_seqs: Vec::with_capacity(10) }
    }
    fn is_retransmission(&self, seq: u32, payload_len: u32) -> bool {
        if payload_len == 0 { return false; }
        for (old_seq, old_len) in &self.recent_seqs {
            if seq == *old_seq && payload_len == *old_len { return true; }
            if seq < self.next_seq && seq + payload_len > *old_seq { return true; }
        }
        false
    }
    const fn is_out_of_order(&self, seq: u32, payload_len: u32) -> bool {
        if payload_len == 0 || self.next_seq == 0 { return false; }
        seq > self.next_seq
    }
    fn update(&mut self, seq: u32, ack: u32, payload_len: u32, has_ack_flag: bool) {
        if payload_len > 0 {
            self.recent_seqs.push((seq, payload_len));
            if self.recent_seqs.len() > 10 { self.recent_seqs.remove(0); }
            let seq_end = seq.wrapping_add(payload_len);
            if self.next_seq == 0 || seq == self.next_seq { self.next_seq = seq_end; }
        }
        if has_ack_flag {
            if ack == self.last_ack && payload_len == 0 { self.dup_ack_count += 1; }
            else { self.dup_ack_count = 0; self.last_ack = ack; }
        }
    }
}

pub struct TcpFlowTracker {
    flows: HashMap<FlowKey, TcpFlowState>,
}

impl TcpFlowTracker {
    pub fn new() -> Self {
        Self { flows: HashMap::new() }
    }
    pub fn analyze_packet(&mut self, packet: &mut PacketInfo) {
        let Some(tcp) = &packet.tcp_details else { return; };
        let (Some(src_port), Some(dst_port)) = (packet.src_port, packet.dst_port) else { return; };
        let flow_key = FlowKey::new(packet.src_addr.clone(), src_port, packet.dst_addr.clone(), dst_port);
        let flow_state = self.flows.entry(flow_key.clone()).or_insert_with(TcpFlowState::new);
        let mut anomaly_reasons: Vec<String> = Vec::new();
        if tcp.flags & 0x04 != 0 { anomaly_reasons.push("RST".to_string()); packet.is_abnormal = true; }
        if flow_state.is_retransmission(tcp.seq, tcp.payload_len) { anomaly_reasons.push("Retransmission".to_string()); packet.is_abnormal = true; }
        if flow_state.is_out_of_order(tcp.seq, tcp.payload_len) { anomaly_reasons.push("Out-of-Order".to_string()); packet.is_abnormal = true; }
        if flow_state.dup_ack_count >= 2 && tcp.flags & 0x10 != 0 { anomaly_reasons.push(format!("Dup ACK #{}", flow_state.dup_ack_count + 1)); packet.is_abnormal = true; }
        if tcp.window == 0 && tcp.flags & 0x10 != 0 { anomaly_reasons.push("ZeroWindow".to_string()); packet.is_abnormal = true; }
        if !anomaly_reasons.is_empty() { packet.info = format!("{}{}", packet.info, format!(" [{}]", anomaly_reasons.join(", "))); }
        flow_state.update(tcp.seq, tcp.ack, tcp.payload_len, tcp.flags & 0x10 != 0);
        if tcp.flags & 0x05 != 0 { self.flows.remove(&flow_key); self.flows.remove(&flow_key.reverse()); }
    }
    pub fn cleanup(&mut self, max_flows: usize) {
        if self.flows.len() > max_flows {
            let to_remove = self.flows.len() - max_flows;
            let keys: Vec<_> = self.flows.keys().take(to_remove).cloned().collect();
            for key in keys { self.flows.remove(&key); }
        }
    }
}

// ============================================================================
// Packet parsing helpers
// ============================================================================

fn parse_packet_data(data: &[u8], timestamp: DateTime<Local>) -> Option<PacketInfo> {
    profiling::scope!("parse_packet_data");
    if data.len() < 14 { return None; }
    let mut ethertype = u16::from_be_bytes([data[12], data[13]]);
    let mut payload_offset = 14;
    let mut vlan_id = None;
    if ethertype == 0x8100 && data.len() >= 18 {
        let tci = u16::from_be_bytes([data[14], data[15]]);
        vlan_id = Some(tci & 0x0FFF);
        ethertype = u16::from_be_bytes([data[16], data[17]]);
        payload_offset = 18;
    }
    let payload = &data[payload_offset..];
    match ethertype {
        0x0800 => parse_ipv4_packet(payload, timestamp, vlan_id),
        0x86DD => parse_ipv6_packet(payload, timestamp, vlan_id),
        0x0806 => Some(PacketInfo { timestamp, src_addr: format_mac(&data[6..12]), src_port: None, dst_addr: format_mac(&data[0..6]), dst_port: None, protocol: "ARP".to_string(), vlan_id, length: data.len() as u32, info: "ARP Request/Reply".to_string(), tcp_details: None, is_abnormal: false }),
        _ => Some(PacketInfo { timestamp, src_addr: format_mac(&data[6..12]), src_port: None, dst_addr: format_mac(&data[0..6]), dst_port: None, protocol: format!("0x{ethertype:04x}"), vlan_id, length: data.len() as u32, info: String::new(), tcp_details: None, is_abnormal: false }),
    }
}

fn format_mac(bytes: &[u8]) -> String {
    if bytes.len() >= 6 {
        format!("{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}", bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5])
    } else {
        "??:??:??:??:??:??".to_string()
    }
}

fn parse_ipv4_packet(data: &[u8], timestamp: DateTime<Local>, vlan_id: Option<u16>) -> Option<PacketInfo> {
    profiling::scope!("parse_ipv4_packet");
    if data.len() < 20 { return None; }
    let ihl = (data[0] & 0x0F) as usize * 4;
    if data.len() < ihl { return None; }
    let protocol = data[9];
    let src_ip = format!("{}.{}.{}.{}", data[12], data[13], data[14], data[15]);
    let dst_ip = format!("{}.{}.{}.{}", data[16], data[17], data[18], data[19]);
    let total_len = u16::from_be_bytes([data[2], data[3]]);
    let transport_data = &data[ihl..];
    let (proto_name, src_port, dst_port, info, tcp_details) = match protocol {
        6 => parse_tcp_info(transport_data),
        17 => { let (p, sp, dp, i) = parse_udp_info(transport_data); (p, sp, dp, i, None) },
        1 => ("ICMP".to_string(), None, None, parse_icmp_info(transport_data), None),
        _ => (format!("IP/{protocol}"), None, None, String::new(), None),
    };
    let is_abnormal = tcp_details.as_ref().is_some_and(|tcp| tcp.flags & 0x04 != 0);
    Some(PacketInfo { timestamp, src_addr: src_ip, src_port, dst_addr: dst_ip, dst_port, protocol: proto_name, vlan_id, length: u32::from(total_len), info, tcp_details, is_abnormal })
}

fn parse_ipv6_packet(data: &[u8], timestamp: DateTime<Local>, vlan_id: Option<u16>) -> Option<PacketInfo> {
    profiling::scope!("parse_ipv6_packet");
    if data.len() < 40 { return None; }
    let next_header = data[6];
    let payload_len = u16::from_be_bytes([data[4], data[5]]);
    let src_ip = format_ipv6(&data[8..24]);
    let dst_ip = format_ipv6(&data[24..40]);
    let transport_data = &data[40..];
    let (proto_name, src_port, dst_port, info, tcp_details) = match next_header {
        6 => parse_tcp_info(transport_data),
        17 => { let (p, sp, dp, i) = parse_udp_info(transport_data); (p, sp, dp, i, None) },
        58 => ("ICMPv6".to_string(), None, None, String::new(), None),
        _ => (format!("IPv6/{next_header}"), None, None, String::new(), None),
    };
    let is_abnormal = tcp_details.as_ref().is_some_and(|tcp| tcp.flags & 0x04 != 0);
    Some(PacketInfo { timestamp, src_addr: src_ip, src_port, dst_addr: dst_ip, dst_port, protocol: proto_name, vlan_id, length: u32::from(payload_len) + 40, info, tcp_details, is_abnormal })
}

fn format_ipv6(bytes: &[u8]) -> String {
    if bytes.len() >= 16 {
        let groups: Vec<String> = (0..8).map(|i| { let val = u16::from_be_bytes([bytes[i * 2], bytes[i * 2 + 1]]); format!("{val:x}") }).collect();
        groups.join(":")
    } else {
        "::".to_string()
    }
}

fn parse_tcp_info(data: &[u8]) -> (String, Option<u16>, Option<u16>, String, Option<TcpDetails>) {
    if data.len() < 20 { return ("TCP".to_string(), None, None, String::new(), None); }
    let src_port = u16::from_be_bytes([data[0], data[1]]);
    let dst_port = u16::from_be_bytes([data[2], data[3]]);
    let seq = u32::from_be_bytes([data[4], data[5], data[6], data[7]]);
    let ack = u32::from_be_bytes([data[8], data[9], data[10], data[11]]);
    let data_offset = ((data[12] >> 4) & 0x0F) as usize * 4;
    let flags = data[13];
    let window = u16::from_be_bytes([data[14], data[15]]);
    let payload_len = if data.len() > data_offset { (data.len() - data_offset) as u32 } else { 0 };
    let tcp_details = TcpDetails { seq, ack, flags, window, payload_len };
    ("TCP".to_string(), Some(src_port), Some(dst_port), String::new(), Some(tcp_details))
}

fn parse_udp_info(data: &[u8]) -> (String, Option<u16>, Option<u16>, String) {
    if data.len() < 8 { return ("UDP".to_string(), None, None, String::new()); }
    let src_port = u16::from_be_bytes([data[0], data[1]]);
    let dst_port = u16::from_be_bytes([data[2], data[3]]);
    ("UDP".to_string(), Some(src_port), Some(dst_port), String::new())
}

fn parse_icmp_info(data: &[u8]) -> String {
    if data.len() < 2 { return String::new(); }
    match (data[0], data[1]) {
        (0, _) => "Echo Reply".to_string(),
        (8, _) => "Echo Request".to_string(),
        (3, 0) => "Dest Unreachable (Net)".to_string(),
        (3, 1) => "Dest Unreachable (Host)".to_string(),
        (3, 3) => "Dest Unreachable (Port)".to_string(),
        (11, _) => "Time Exceeded".to_string(),
        (t, c) => format!("Type={t} Code={c}"),
    }
}

fn pcap_ts_to_datetime(sec: u32, usec: u32) -> Option<DateTime<Local>> {
    Local.timestamp_opt(i64::from(sec), usec * 1000).single()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PcapFormat {
    Legacy,
    PcapNG,
}

fn detect_pcap_format(path: &Path) -> Result<PcapFormat, String> {
    use std::io::Read;
    let mut file = File::open(path).map_err(|e| format!("Failed to open file: {e}"))?;
    let mut magic = [0u8; 4];
    file.read_exact(&mut magic).map_err(|e| format!("Failed to read magic: {e}"))?;
    match &magic {
        [0xd4, 0xc3, 0xb2, 0xa1] => Ok(PcapFormat::Legacy),
        [0xa1, 0xb2, 0xc3, 0xd4] => Ok(PcapFormat::Legacy),
        [0x4d, 0x3c, 0xb2, 0xa1] => Ok(PcapFormat::Legacy),
        [0xa1, 0xb2, 0x3c, 0x4d] => Ok(PcapFormat::Legacy),
        [0x0a, 0x0d, 0x0d, 0x0a] => Ok(PcapFormat::PcapNG),
        _ => Err("Unknown pcap format".to_string()),
    }
}

/// Parse all packets from a pcap/pcapng file and return them as typed log lines.
pub fn parse_pcap_to_lines<P: AsRef<Path>>(path: P) -> Result<Vec<PcapLogLine>, String> {
    let path = path.as_ref();
    let format = detect_pcap_format(path)?;
    let lines = match format {
        PcapFormat::Legacy => parse_legacy_pcap_to_lines(path),
        PcapFormat::PcapNG => parse_pcapng_to_lines(path),
    }?;
    if lines.is_empty() {
        return Err("No valid packets found in pcap file".to_string());
    }
    Ok(lines)
}

fn parse_legacy_pcap_to_lines(path: &Path) -> Result<Vec<PcapLogLine>, String> {
    profiling::scope!("parse_legacy_pcap_to_lines");
    log::info!("Starting legacy pcap parsing: {}", path.display());
    let file = File::open(path).map_err(|e| format!("Failed to open pcap file: {e}"))?;
    let reader = BufReader::new(file);
    let mut pcap_reader = LegacyPcapReader::new(65536, reader)
        .map_err(|e| format!("Failed to create pcap reader: {e:?}"))?;
    let mut lines = Vec::new();
    let mut line_number = 1usize;
    let mut flow_tracker = TcpFlowTracker::new();
    loop {
        match pcap_reader.next() {
            Ok((offset, block)) => {
                if let PcapBlockOwned::Legacy(packet) = block {
                    let timestamp = pcap_ts_to_datetime(packet.ts_sec, packet.ts_usec).unwrap_or_else(Local::now);
                    if let Some(mut packet_info) = parse_packet_data(packet.data, timestamp) {
                        flow_tracker.analyze_packet(&mut packet_info);
                        lines.push(PcapLogLine::new(packet_info, line_number));
                        line_number += 1;
                    }
                }
                if !lines.is_empty() && lines.len() % 10_000 == 0 { flow_tracker.cleanup(10_000); }
                pcap_reader.consume(offset);
            }
            Err(PcapError::Eof) => break,
            Err(PcapError::Incomplete(_)) => { pcap_reader.refill().map_err(|e| format!("Read error: {e}"))?; }
            Err(e) => { log::warn!("Pcap parse error: {e:?}"); break; }
        }
    }
    log::info!("Parsed {} legacy pcap packets", lines.len());
    Ok(lines)
}

fn parse_pcapng_to_lines(path: &Path) -> Result<Vec<PcapLogLine>, String> {
    profiling::scope!("parse_pcapng_to_lines");
    log::info!("Starting pcapng parsing: {}", path.display());
    let file = File::open(path).map_err(|e| format!("Failed to open pcapng file: {e}"))?;
    let reader = BufReader::new(file);
    let mut pcap_reader = PcapNGReader::new(65536, reader)
        .map_err(|e| format!("Failed to create pcapng reader: {e:?}"))?;
    let mut lines = Vec::new();
    let mut line_number = 1usize;
    let mut if_tsresol: u64 = 1_000_000;
    let mut flow_tracker = TcpFlowTracker::new();
    loop {
        match pcap_reader.next() {
            Ok((offset, block)) => {
                match block {
                    PcapBlockOwned::NG(pcap_parser::Block::InterfaceDescription(idb)) => {
                        for opt in &idb.options {
                            if opt.code.0 == 9 && !opt.value.is_empty() {
                                let resol = opt.value[0];
                                if_tsresol = if resol & 0x80 != 0 { 1u64 << (resol & 0x7F) } else { 10u64.pow(u32::from(resol)) };
                            }
                        }
                    }
                    PcapBlockOwned::NG(pcap_parser::Block::EnhancedPacket(epb)) => {
                        let ts_raw = (u64::from(epb.ts_high) << 32) | u64::from(epb.ts_low);
                        let sec = ts_raw / if_tsresol;
                        let nsec = ((ts_raw % if_tsresol) * 1_000_000_000) / if_tsresol;
                        let timestamp = Local.timestamp_opt(sec.cast_signed(), nsec as u32).single().unwrap_or_else(Local::now);
                        if let Some(mut packet_info) = parse_packet_data(epb.data, timestamp) {
                            flow_tracker.analyze_packet(&mut packet_info);
                            lines.push(PcapLogLine::new(packet_info, line_number));
                            line_number += 1;
                        }
                    }
                    PcapBlockOwned::NG(pcap_parser::Block::SimplePacket(spb)) => {
                        let timestamp = Local::now();
                        if let Some(mut packet_info) = parse_packet_data(spb.data, timestamp) {
                            flow_tracker.analyze_packet(&mut packet_info);
                            lines.push(PcapLogLine::new(packet_info, line_number));
                            line_number += 1;
                        }
                    }
                    PcapBlockOwned::NG(_) | PcapBlockOwned::Legacy(_) | PcapBlockOwned::LegacyHeader(_) => {}
                }
                if !lines.is_empty() && lines.len() % 10_000 == 0 { flow_tracker.cleanup(10_000); }
                pcap_reader.consume(offset);
            }
            Err(PcapError::Eof) => break,
            Err(PcapError::Incomplete(_)) => { pcap_reader.refill().map_err(|e| format!("Read error: {e}"))?; }
            Err(e) => { log::warn!("Pcapng parse error: {e:?}"); break; }
        }
    }
    log::info!("Parsed {} pcapng packets", lines.len());
    Ok(lines)
}
