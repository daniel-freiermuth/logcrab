// LogCrab - GPL-3.0-or-later
// This file is part of LogCrab.
//
// Copyright (C) 2025 Daniel Freiermuth
//
// LogCrab is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// LogCrab is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with LogCrab.  If not, see <https://www.gnu.org/licenses/>.

//! PCAP (Packet Capture) file parser for network log analysis.
//!
//! Supports both classic pcap and pcapng formats.

use crate::core::log_file::ProgressCallback;
use crate::core::log_store::SourceData;

use super::line::{LogLine, PcapLogLine};
use chrono::{DateTime, Local, TimeZone};
use pcap_parser::traits::PcapReaderIterator;
use pcap_parser::{PcapBlockOwned, PcapError, PcapNGReader, LegacyPcapReader};
use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use std::sync::Arc;

/// Initial chunk size for incremental loading
/// Start small for fast initial feedback, then grow to handle merge overhead
const PCAP_INITIAL_CHUNK_SIZE: usize = 1 << 14; // 16,384 packets
const PCAP_MAX_CHUNK_SIZE: usize = 1 << 20; // 1,048,576 packets
const PCAP_CHUNKS_BEFORE_GROWTH: usize = 3; // Double chunk size every 3 chunks

/// Represents a parsed network packet for display
#[derive(Debug, Clone)]
pub struct PacketInfo {
    /// Packet timestamp
    pub timestamp: DateTime<Local>,
    /// Source address (IP or MAC)
    pub src_addr: String,
    /// Source port (if applicable)
    pub src_port: Option<u16>,
    /// Destination address (IP or MAC)  
    pub dst_addr: String,
    /// Destination port (if applicable)
    pub dst_port: Option<u16>,
    /// Protocol name (TCP, UDP, ICMP, etc.)
    pub protocol: String,
    /// VLAN ID (if 802.1Q tagged)
    pub vlan_id: Option<u16>,
    /// Packet length in bytes
    pub length: u32,
    /// Brief payload info or flags
    pub info: String,
    /// TCP details (if TCP packet)
    pub tcp_details: Option<TcpDetails>,
    /// Marks abnormal flows (RST, retransmissions, etc.)
    pub is_abnormal: bool,
}

/// TCP-specific packet details
#[derive(Debug, Clone)]
pub struct TcpDetails {
    /// Sequence number
    pub seq: u32,
    /// Acknowledgment number
    pub ack: u32,
    /// TCP flags byte
    pub flags: u8,
    /// Window size
    pub window: u16,
    /// Payload length (TCP data bytes)
    pub payload_len: u32,
}

impl PacketInfo {
    /// Format as a display message
    pub fn format_message(&self) -> String {
        let src = match self.src_port {
            Some(port) => format!("{}:{}", self.src_addr, port),
            None => self.src_addr.clone(),
        };
        let dst = match self.dst_port {
            Some(port) => format!("{}:{}", self.dst_addr, port),
            None => self.dst_addr.clone(),
        };
        
        let vlan = self.vlan_id.map_or(String::new(), |id| format!(" [VLAN {}]", id));
        let abnormal = if self.is_abnormal { " ⚠" } else { "" };
        
        // Enhanced TCP formatting with seq/ack
        if let Some(ref tcp) = self.tcp_details {
            let flags_str = format_tcp_flags(tcp.flags);
            let seq_str = format!("Seq={}", tcp.seq);
            let ack_str = if tcp.flags & 0x10 != 0 {
                format!(" Ack={}", tcp.ack)
            } else {
                String::new()
            };
            let win_str = format!(" Win={}", tcp.window);
            let len_str = if tcp.payload_len > 0 {
                format!(" Len={}", tcp.payload_len)
            } else {
                String::new()
            };
            
            format!(
                "{} {} → {}{} {} {}{}{}{}{}" ,
                self.protocol, src, dst, vlan, flags_str, seq_str, ack_str, win_str, len_str, abnormal
            )
        } else if self.info.is_empty() {
            format!("{} {} → {}{} Len={}{}", self.protocol, src, dst, vlan, self.length, abnormal)
        } else {
            format!(
                "{} {} → {}{} {} Len={}{}",
                self.protocol, src, dst, vlan, self.info, self.length, abnormal
            )
        }
    }

    /// Format as raw line (more detailed)
    pub fn format_raw(&self) -> String {
        let src = match self.src_port {
            Some(port) => format!("{}:{}", self.src_addr, port),
            None => self.src_addr.clone(),
        };
        let dst = match self.dst_port {
            Some(port) => format!("{}:{}", self.dst_addr, port),
            None => self.dst_addr.clone(),
        };
        
        let vlan = self.vlan_id.map_or(String::new(), |id| format!(" VLAN={}", id));
        let abnormal = if self.is_abnormal { " [ABNORMAL]" } else { "" };
        
        // Enhanced TCP formatting for raw view
        if let Some(ref tcp) = self.tcp_details {
            let flags_str = format_tcp_flags(tcp.flags);
            let seq_str = format!("Seq={}", tcp.seq);
            let ack_str = if tcp.flags & 0x10 != 0 {
                format!(" Ack={}", tcp.ack)
            } else {
                String::new()
            };
            let win_str = format!(" Win={}", tcp.window);
            let len_str = if tcp.payload_len > 0 {
                format!(" Len={}", tcp.payload_len)
            } else {
                String::new()
            };
            
            format!(
                "[{}] {} {} → {}{} {} {}{}{}{}{}" ,
                self.timestamp.format("%H:%M:%S%.6f"),
                self.protocol,
                src,
                dst,
                vlan,
                flags_str,
                seq_str,
                ack_str,
                win_str,
                len_str,
                abnormal
            )
        } else {
            format!(
                "[{}] {} {} → {}{} {} Length={}{}",
                self.timestamp.format("%H:%M:%S%.6f"),
                self.protocol,
                src,
                dst,
                vlan,
                self.info,
                self.length,
                abnormal
            )
        }
    }
}

/// Format TCP flags as human-readable string
fn format_tcp_flags(flags: u8) -> String {
    let mut flag_strs = Vec::new();
    if flags & 0x02 != 0 { flag_strs.push("SYN"); }
    if flags & 0x10 != 0 { flag_strs.push("ACK"); }
    if flags & 0x01 != 0 { flag_strs.push("FIN"); }
    if flags & 0x04 != 0 { flag_strs.push("RST"); }
    if flags & 0x08 != 0 { flag_strs.push("PSH"); }
    if flags & 0x20 != 0 { flag_strs.push("URG"); }
    
    if flag_strs.is_empty() {
        "[]".to_string()
    } else {
        format!("[{}]", flag_strs.join(","))
    }
}

// ============================================================================
// TCP Flow Tracking for Anomaly Detection
// ============================================================================

use std::collections::HashMap;

/// Unique identifier for a TCP flow (5-tuple)
#[derive(Debug, Clone, Hash, Eq, PartialEq)]
struct FlowKey {
    src_addr: String,
    src_port: u16,
    dst_addr: String,
    dst_port: u16,
}

impl FlowKey {
    fn new(src_addr: String, src_port: u16, dst_addr: String, dst_port: u16) -> Self {
        Self { src_addr, src_port, dst_addr, dst_port }
    }
    
    /// Get the reverse flow key (for bidirectional tracking)
    fn reverse(&self) -> Self {
        Self {
            src_addr: self.dst_addr.clone(),
            src_port: self.dst_port,
            dst_addr: self.src_addr.clone(),
            dst_port: self.src_port,
        }
    }
}

/// TCP flow state for tracking anomalies
#[derive(Debug, Clone)]
struct TcpFlowState {
    /// Next expected sequence number
    next_seq: u32,
    /// Last seen acknowledgment number
    last_ack: u32,
    /// Count of duplicate ACKs
    dup_ack_count: u8,
    /// Track last few sequence numbers seen (for retransmission detection)
    recent_seqs: Vec<(u32, u32)>, // (seq, payload_len)
}

impl TcpFlowState {
    fn new() -> Self {
        Self {
            next_seq: 0,
            last_ack: 0,
            dup_ack_count: 0,
            recent_seqs: Vec::with_capacity(10),
        }
    }
    
    /// Check if this packet is a retransmission
    fn is_retransmission(&self, seq: u32, payload_len: u32) -> bool {
        if payload_len == 0 {
            return false; // Pure ACKs are not retransmissions
        }
        
        // Check if we've seen this sequence range before
        for (old_seq, old_len) in &self.recent_seqs {
            if seq == *old_seq && payload_len == *old_len {
                return true;
            }
            // Also check for overlapping sequence ranges
            if seq < self.next_seq && seq + payload_len > *old_seq {
                return true;
            }
        }
        false
    }
    
    /// Check if this is an out-of-order packet
    fn is_out_of_order(&self, seq: u32, payload_len: u32) -> bool {
        if payload_len == 0 || self.next_seq == 0 {
            return false;
        }
        // Packet is out of order if it arrives after the expected sequence
        seq > self.next_seq
    }
    
    /// Update flow state with new packet
    fn update(&mut self, seq: u32, ack: u32, payload_len: u32, has_ack_flag: bool) {
        // Update sequence tracking
        if payload_len > 0 {
            self.recent_seqs.push((seq, payload_len));
            if self.recent_seqs.len() > 10 {
                self.recent_seqs.remove(0);
            }
            
            // Update next expected sequence
            let seq_end = seq.wrapping_add(payload_len);
            if self.next_seq == 0 || seq == self.next_seq {
                self.next_seq = seq_end;
            }
        }
        
        // Update ACK tracking for duplicate ACK detection
        if has_ack_flag {
            if ack == self.last_ack && payload_len == 0 {
                self.dup_ack_count += 1;
            } else {
                self.dup_ack_count = 0;
                self.last_ack = ack;
            }
        }
    }
}

/// TCP flow tracker for detecting anomalies
pub struct TcpFlowTracker {
    flows: HashMap<FlowKey, TcpFlowState>,
}

impl TcpFlowTracker {
    pub fn new() -> Self {
        Self {
            flows: HashMap::new(),
        }
    }
    
    /// Analyze a TCP packet and detect anomalies
    pub fn analyze_packet(&mut self, packet: &mut PacketInfo) {
        let tcp = match &packet.tcp_details {
            Some(t) => t,
            None => return,
        };
        
        let (src_port, dst_port) = match (packet.src_port, packet.dst_port) {
            (Some(sp), Some(dp)) => (sp, dp),
            _ => return,
        };
        
        let flow_key = FlowKey::new(
            packet.src_addr.clone(),
            src_port,
            packet.dst_addr.clone(),
            dst_port,
        );
        
        // Get or create flow state
        let flow_state = self.flows.entry(flow_key.clone()).or_insert_with(TcpFlowState::new);
        
        // Detect anomalies
        let mut anomaly_reasons: Vec<String> = Vec::new();
        
        // Check for RST
        if tcp.flags & 0x04 != 0 {
            anomaly_reasons.push("RST".to_string());
            packet.is_abnormal = true;
        }
        
        // Check for retransmission
        if flow_state.is_retransmission(tcp.seq, tcp.payload_len) {
            anomaly_reasons.push("Retransmission".to_string());
            packet.is_abnormal = true;
        }
        
        // Check for out-of-order
        if flow_state.is_out_of_order(tcp.seq, tcp.payload_len) {
            anomaly_reasons.push("Out-of-Order".to_string());
            packet.is_abnormal = true;
        }
        
        // Check for duplicate ACK (3+ duplicate ACKs indicate fast retransmit)
        if flow_state.dup_ack_count >= 2 && tcp.flags & 0x10 != 0 {
            anomaly_reasons.push(format!("Dup ACK #{}", flow_state.dup_ack_count + 1));
            packet.is_abnormal = true;
        }
        
        // Check for zero window
        if tcp.window == 0 && tcp.flags & 0x10 != 0 {
            anomaly_reasons.push("ZeroWindow".to_string());
            packet.is_abnormal = true;
        }
        
        // Append anomaly info to the info string
        if !anomaly_reasons.is_empty() {
            let anomaly_str = format!(" [{}]", anomaly_reasons.join(", "));
            packet.info = format!("{}{}", packet.info, anomaly_str);
        }
        
        // Update flow state
        flow_state.update(tcp.seq, tcp.ack, tcp.payload_len, tcp.flags & 0x10 != 0);
        
        // Cleanup: remove flow on FIN or RST
        if tcp.flags & 0x05 != 0 { // FIN or RST
            self.flows.remove(&flow_key);
            self.flows.remove(&flow_key.reverse());
        }
    }
    
    /// Clear old flows to prevent unbounded memory growth
    pub fn cleanup(&mut self, max_flows: usize) {
        if self.flows.len() > max_flows {
            // Keep only the most recent flows (simple strategy)
            let to_remove = self.flows.len() - max_flows;
            let keys: Vec<_> = self.flows.keys().take(to_remove).cloned().collect();
            for key in keys {
                self.flows.remove(&key);
            }
        }
    }
}

/// Parse Ethernet frame and extract packet info
fn parse_packet_data(data: &[u8], timestamp: DateTime<Local>) -> Option<PacketInfo> {
    profiling::scope!("parse_packet_data");
    // Minimum Ethernet header size
    if data.len() < 14 {
        return None;
    }

    // Parse Ethernet header
    let mut ethertype = u16::from_be_bytes([data[12], data[13]]);
    let mut payload_offset = 14;
    let mut vlan_id = None;

    // Handle 802.1Q VLAN tagging (0x8100)
    // VLAN tag is 4 bytes: 2 bytes TCI + 2 bytes ethertype
    if ethertype == 0x8100 && data.len() >= 18 {
        // Extract VLAN ID from TCI (bits 0-11 of the 16-bit TCI field)
        let tci = u16::from_be_bytes([data[14], data[15]]);
        vlan_id = Some(tci & 0x0FFF);
        // Read the actual ethertype after the VLAN tag
        ethertype = u16::from_be_bytes([data[16], data[17]]);
        payload_offset = 18;
    }

    let payload = &data[payload_offset..];

    match ethertype {
        0x0800 => parse_ipv4_packet(payload, timestamp, vlan_id),
        0x86DD => parse_ipv6_packet(payload, timestamp, vlan_id),
        0x0806 => Some(PacketInfo {
            timestamp,
            src_addr: format_mac(&data[6..12]),
            src_port: None,
            dst_addr: format_mac(&data[0..6]),
            dst_port: None,
            protocol: "ARP".to_string(),
            vlan_id,
            length: data.len() as u32,
            info: "ARP Request/Reply".to_string(),
            tcp_details: None,
            is_abnormal: false,
        }),
        _ => Some(PacketInfo {
            timestamp,
            src_addr: format_mac(&data[6..12]),
            src_port: None,
            dst_addr: format_mac(&data[0..6]),
            dst_port: None,
            protocol: format!("0x{ethertype:04x}"),
            vlan_id,
            length: data.len() as u32,
            info: String::new(),
            tcp_details: None,
            is_abnormal: false,
        }),
    }
}

/// Format MAC address
fn format_mac(bytes: &[u8]) -> String {
    if bytes.len() >= 6 {
        format!(
            "{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
            bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5]
        )
    } else {
        "??:??:??:??:??:??".to_string()
    }
}

/// Parse IPv4 packet
fn parse_ipv4_packet(data: &[u8], timestamp: DateTime<Local>, vlan_id: Option<u16>) -> Option<PacketInfo> {
    profiling::scope!("parse_ipv4_packet");
    // Minimum IPv4 header size
    if data.len() < 20 {
        return None;
    }

    let ihl = (data[0] & 0x0F) as usize * 4;
    if data.len() < ihl {
        return None;
    }

    let protocol = data[9];
    let src_ip = format!("{}.{}.{}.{}", data[12], data[13], data[14], data[15]);
    let dst_ip = format!("{}.{}.{}.{}", data[16], data[17], data[18], data[19]);
    let total_len = u16::from_be_bytes([data[2], data[3]]);

    let transport_data = &data[ihl..];

    let (proto_name, src_port, dst_port, info, tcp_details) = match protocol {
        6 => parse_tcp_info(transport_data),
        17 => {
            let (p, sp, dp, i) = parse_udp_info(transport_data);
            (p, sp, dp, i, None)
        }
        1 => ("ICMP".to_string(), None, None, parse_icmp_info(transport_data), None),
        _ => (format!("IP/{protocol}"), None, None, String::new(), None),
    };
    
    // Mark abnormal packets (RST, retransmissions handled by flow tracker)
    let is_abnormal = if let Some(ref tcp) = tcp_details {
        tcp.flags & 0x04 != 0 // RST flag
    } else {
        false
    };

    Some(PacketInfo {
        timestamp,
        src_addr: src_ip,
        src_port,
        dst_addr: dst_ip,
        dst_port,
        protocol: proto_name,
        vlan_id,
        length: u32::from(total_len),
        info,
        tcp_details,
        is_abnormal,
    })
}

/// Parse IPv6 packet
fn parse_ipv6_packet(data: &[u8], timestamp: DateTime<Local>, vlan_id: Option<u16>) -> Option<PacketInfo> {
    profiling::scope!("parse_ipv6_packet");
    // Minimum IPv6 header size
    if data.len() < 40 {
        return None;
    }

    let next_header = data[6];
    let payload_len = u16::from_be_bytes([data[4], data[5]]);

    let src_ip = format_ipv6(&data[8..24]);
    let dst_ip = format_ipv6(&data[24..40]);

    let transport_data = &data[40..];

    let (proto_name, src_port, dst_port, info, tcp_details) = match next_header {
        6 => parse_tcp_info(transport_data),
        17 => {
            let (p, sp, dp, i) = parse_udp_info(transport_data);
            (p, sp, dp, i, None)
        }
        58 => ("ICMPv6".to_string(), None, None, String::new(), None),
        _ => (format!("IPv6/{next_header}"), None, None, String::new(), None),
    };
    
    // Mark abnormal packets (RST, retransmissions handled by flow tracker)
    let is_abnormal = if let Some(ref tcp) = tcp_details {
        tcp.flags & 0x04 != 0 // RST flag
    } else {
        false
    };

    Some(PacketInfo {
        timestamp,
        src_addr: src_ip,
        src_port,
        dst_addr: dst_ip,
        dst_port,
        protocol: proto_name,
        vlan_id,
        length: u32::from(payload_len) + 40,
        info,
        tcp_details,
        is_abnormal,
    })
}

/// Format IPv6 address
fn format_ipv6(bytes: &[u8]) -> String {
    if bytes.len() >= 16 {
        let groups: Vec<String> = (0..8)
            .map(|i| {
                let val = u16::from_be_bytes([bytes[i * 2], bytes[i * 2 + 1]]);
                format!("{val:x}")
            })
            .collect();
        groups.join(":")
    } else {
        "::".to_string()
    }
}

/// Parse TCP header and extract port/flag info with sequence numbers
fn parse_tcp_info(data: &[u8]) -> (String, Option<u16>, Option<u16>, String, Option<TcpDetails>) {
    if data.len() < 20 {
        return ("TCP".to_string(), None, None, String::new(), None);
    }

    let src_port = u16::from_be_bytes([data[0], data[1]]);
    let dst_port = u16::from_be_bytes([data[2], data[3]]);
    let seq = u32::from_be_bytes([data[4], data[5], data[6], data[7]]);
    let ack = u32::from_be_bytes([data[8], data[9], data[10], data[11]]);
    let data_offset = ((data[12] >> 4) & 0x0F) as usize * 4;
    let flags = data[13];
    let window = u16::from_be_bytes([data[14], data[15]]);
    
    // Calculate TCP payload length
    let payload_len = if data.len() > data_offset {
        (data.len() - data_offset) as u32
    } else {
        0
    };
    
    let tcp_details = TcpDetails {
        seq,
        ack,
        flags,
        window,
        payload_len,
    };

    ("TCP".to_string(), Some(src_port), Some(dst_port), String::new(), Some(tcp_details))
}

/// Parse UDP header and extract port info
fn parse_udp_info(data: &[u8]) -> (String, Option<u16>, Option<u16>, String) {
    if data.len() < 8 {
        return ("UDP".to_string(), None, None, String::new());
    }

    let src_port = u16::from_be_bytes([data[0], data[1]]);
    let dst_port = u16::from_be_bytes([data[2], data[3]]);

    ("UDP".to_string(), Some(src_port), Some(dst_port), String::new())
}

/// Parse ICMP type/code
fn parse_icmp_info(data: &[u8]) -> String {
    if data.len() < 2 {
        return String::new();
    }

    let icmp_type = data[0];
    let icmp_code = data[1];

    match (icmp_type, icmp_code) {
        (0, _) => "Echo Reply".to_string(),
        (8, _) => "Echo Request".to_string(),
        (3, 0) => "Dest Unreachable (Net)".to_string(),
        (3, 1) => "Dest Unreachable (Host)".to_string(),
        (3, 3) => "Dest Unreachable (Port)".to_string(),
        (11, _) => "Time Exceeded".to_string(),
        _ => format!("Type={icmp_type} Code={icmp_code}"),
    }
}

/// Convert pcap timestamp to `DateTime<Local>`
fn pcap_ts_to_datetime(ts_sec: u32, ts_usec: u32) -> Option<DateTime<Local>> {
    Local.timestamp_opt(i64::from(ts_sec), ts_usec * 1000).single()
}

/// Parse a legacy pcap file with incremental loading
fn parse_legacy_pcap<P: AsRef<Path>>(
    path: P,
    source: &Arc<SourceData>,
    progress_callback: &ProgressCallback,
) -> Result<usize, String> {
    profiling::scope!("parse_legacy_pcap");
    let start_time = std::time::Instant::now();
    let path = path.as_ref();
    log::info!("Starting legacy pcap parsing: {}", path.display());
    let file = File::open(path).map_err(|e| format!("Failed to open pcap file: {e}"))?;
    let file_size = file.metadata().map(|m| m.len()).unwrap_or(0);
    let reader = BufReader::new(file);
    
    let mut pcap_reader = LegacyPcapReader::new(65536, reader)
        .map_err(|e| format!("Failed to create pcap reader: {e:?}"))?;

    let mut chunk_lines = Vec::new();
    let mut line_number = 1;
    let mut bytes_processed = 0u64;
    let mut last_log_time = std::time::Instant::now();
    let mut packets_since_log = 0;
    let mut chunk_count = 0;
    let mut current_chunk_size = PCAP_INITIAL_CHUNK_SIZE;
    
    // Create TCP flow tracker for anomaly detection
    let mut flow_tracker = TcpFlowTracker::new();

    loop {
        match pcap_reader.next() {
            Ok((offset, block)) => {
                bytes_processed += offset as u64;

                if let PcapBlockOwned::Legacy(packet) = block {
                    let timestamp = pcap_ts_to_datetime(packet.ts_sec, packet.ts_usec)
                        .unwrap_or_else(Local::now);

                    if let Some(mut packet_info) = parse_packet_data(&packet.data, timestamp) {
                        // Analyze TCP packets for anomalies
                        flow_tracker.analyze_packet(&mut packet_info);
                        
                        let log_line = LogLine::Pcap(PcapLogLine::new(packet_info, line_number));
                        chunk_lines.push(log_line);
                        line_number += 1;
                        packets_since_log += 1;

                        if chunk_lines.len() >= current_chunk_size {
                            source.append_lines(std::mem::take(&mut chunk_lines));
                            chunk_count += 1;
                            
                            // Grow chunk size exponentially (double every N chunks)
                            if chunk_count % PCAP_CHUNKS_BEFORE_GROWTH == 0 && current_chunk_size < PCAP_MAX_CHUNK_SIZE {
                                current_chunk_size = (current_chunk_size * 2).min(PCAP_MAX_CHUNK_SIZE);
                                log::debug!("Increased chunk size to {} packets", current_chunk_size);
                            }
                            
                            let progress = if file_size > 0 {
                                bytes_processed as f32 / file_size as f32
                            } else {
                                0.0
                            };
                            progress_callback(
                                progress,
                                &format!("Parsing pcap... ({} packets)", source.len()),
                            );
                            
                            // Log performance every 5 seconds
                            let now = std::time::Instant::now();
                            if now.duration_since(last_log_time).as_secs() >= 5 {
                                let elapsed = now.duration_since(start_time).as_secs_f64();
                                let rate = packets_since_log as f64 / now.duration_since(last_log_time).as_secs_f64();
                                log::info!(
                                    "Parsed {} packets in {:.2}s ({:.0} pkt/s, {:.1} MB/s)",
                                    source.len(),
                                    elapsed,
                                    rate,
                                    (bytes_processed as f64 / elapsed) / 1_048_576.0
                                );
                                last_log_time = now;
                                packets_since_log = 0;
                            }
                            
                            // Cleanup flow tracker periodically to prevent memory growth
                            if chunk_count % 10 == 0 {
                                flow_tracker.cleanup(10000);
                            }
                        }
                    }
                }
                pcap_reader.consume(offset);
            }
            Err(PcapError::Eof) => break,
            Err(PcapError::Incomplete(_)) => {
                pcap_reader.refill().map_err(|e| format!("Read error: {e}"))?;
            }
            Err(e) => {
                log::warn!("Pcap parse error: {e:?}");
                break;
            }
        }
    }

    // Send any remaining lines
    if !chunk_lines.is_empty() {
        source.append_lines(chunk_lines);
    }

    let elapsed = start_time.elapsed();
    let total_packets = source.len();
    log::info!(
        "Completed legacy pcap parsing: {} packets in {:.2}s ({:.0} pkt/s, {} MB total)",
        total_packets,
        elapsed.as_secs_f64(),
        total_packets as f64 / elapsed.as_secs_f64(),
        file_size / 1_048_576
    );

    Ok(source.len())
}

/// Parse a pcapng file with incremental loading
fn parse_pcapng<P: AsRef<Path>>(
    path: P,
    source: &Arc<SourceData>,
    progress_callback: &ProgressCallback,
) -> Result<usize, String> {
    profiling::scope!("parse_pcapng");
    let start_time = std::time::Instant::now();
    let path = path.as_ref();
    log::info!("Starting pcapng parsing: {}", path.display());
    let file = File::open(path).map_err(|e| format!("Failed to open pcapng file: {e}"))?;
    let file_size = file.metadata().map(|m| m.len()).unwrap_or(0);
    let reader = BufReader::new(file);
    
    let mut pcap_reader = PcapNGReader::new(65536, reader)
        .map_err(|e| format!("Failed to create pcapng reader: {e:?}"))?;

    let mut chunk_lines = Vec::new();
    let mut line_number = 1;
    let mut bytes_processed = 0u64;
    // pcapng stores timestamp resolution per interface, default to microseconds
    let mut if_tsresol: u64 = 1_000_000;
    let mut last_log_time = std::time::Instant::now();
    let mut packets_since_log = 0;
    let mut chunk_count = 0;
    let mut current_chunk_size = PCAP_INITIAL_CHUNK_SIZE;
    
    // Create TCP flow tracker for anomaly detection
    let mut flow_tracker = TcpFlowTracker::new();

    loop {
        match pcap_reader.next() {
            Ok((offset, block)) => {
                bytes_processed += offset as u64;

                match block {
                    PcapBlockOwned::NG(pcap_parser::Block::InterfaceDescription(idb)) => {
                        // Extract timestamp resolution from interface options
                        for opt in &idb.options {
                            if opt.code.0 == 9 && !opt.value.is_empty() {
                                // if_tsresol option
                                let resol = opt.value[0];
                                if resol & 0x80 != 0 {
                                    // Power of 2
                                    if_tsresol = 1u64 << (resol & 0x7F);
                                } else {
                                    // Power of 10
                                    if_tsresol = 10u64.pow(u32::from(resol));
                                }
                            }
                        }
                    }
                    PcapBlockOwned::NG(pcap_parser::Block::EnhancedPacket(epb)) => {
                        // Calculate timestamp from high/low parts
                        let ts_raw = (u64::from(epb.ts_high) << 32) | u64::from(epb.ts_low);
                        let ts_sec = ts_raw / if_tsresol;
                        let ts_frac = ts_raw % if_tsresol;
                        let ts_nsec = (ts_frac * 1_000_000_000) / if_tsresol;

                        let timestamp = Local.timestamp_opt(ts_sec as i64, ts_nsec as u32)
                            .single()
                            .unwrap_or_else(Local::now);

                        if let Some(mut packet_info) = parse_packet_data(&epb.data, timestamp) {
                            // Analyze TCP packets for anomalies
                            flow_tracker.analyze_packet(&mut packet_info);
                            
                            let log_line = LogLine::Pcap(PcapLogLine::new(packet_info, line_number));
                            chunk_lines.push(log_line);
                            line_number += 1;
                            packets_since_log += 1;

                            if chunk_lines.len() >= current_chunk_size {
                                source.append_lines(std::mem::take(&mut chunk_lines));
                                chunk_count += 1;
                                
                                // Grow chunk size exponentially (double every N chunks)
                                if chunk_count % PCAP_CHUNKS_BEFORE_GROWTH == 0 && current_chunk_size < PCAP_MAX_CHUNK_SIZE {
                                    current_chunk_size = (current_chunk_size * 2).min(PCAP_MAX_CHUNK_SIZE);
                                    log::debug!("Increased chunk size to {} packets", current_chunk_size);
                                }
                                
                                let progress = if file_size > 0 {
                                    bytes_processed as f32 / file_size as f32
                                } else {
                                    0.0
                                };
                                progress_callback(
                                    progress,
                                    &format!("Parsing pcapng... ({} packets)", source.len()),
                                );
                                
                                // Log performance every 5 seconds
                                let now = std::time::Instant::now();
                                if now.duration_since(last_log_time).as_secs() >= 5 {
                                    let elapsed = now.duration_since(start_time).as_secs_f64();
                                    let rate = packets_since_log as f64 / now.duration_since(last_log_time).as_secs_f64();
                                    log::info!(
                                        "Parsed {} packets in {:.2}s ({:.0} pkt/s, {:.1} MB/s)",
                                        source.len(),
                                        elapsed,
                                        rate,
                                        (bytes_processed as f64 / elapsed) / 1_048_576.0
                                    );
                                    last_log_time = now;
                                    packets_since_log = 0;
                                }
                                
                                // Cleanup flow tracker periodically to prevent memory growth
                                if chunk_count % 10 == 0 {
                                    flow_tracker.cleanup(10000);
                                }
                            }
                        }
                    }
                    PcapBlockOwned::NG(pcap_parser::Block::SimplePacket(spb)) => {
                        // Simple packets don't have timestamps, use current time
                        let timestamp = Local::now();
                        if let Some(mut packet_info) = parse_packet_data(&spb.data, timestamp) {
                            // Analyze TCP packets for anomalies
                            flow_tracker.analyze_packet(&mut packet_info);
                            
                            let log_line = LogLine::Pcap(PcapLogLine::new(packet_info, line_number));
                            chunk_lines.push(log_line);
                            line_number += 1;
                        }
                    }
                    // Skip other block types (SectionHeader, etc.)
                    PcapBlockOwned::NG(_) | PcapBlockOwned::Legacy(_) | PcapBlockOwned::LegacyHeader(_) => {}
                }
                pcap_reader.consume(offset);
            }
            Err(PcapError::Eof) => break,
            Err(PcapError::Incomplete(_)) => {
                pcap_reader.refill().map_err(|e| format!("Read error: {e}"))?;
            }
            Err(e) => {
                log::warn!("Pcapng parse error: {e:?}");
                break;
            }
        }
    }

    // Send any remaining lines
    if !chunk_lines.is_empty() {
        source.append_lines(chunk_lines);
    }

    let elapsed = start_time.elapsed();
    let total_packets = source.len();
    log::info!(
        "Completed pcapng parsing: {} packets in {:.2}s ({:.0} pkt/s, {} MB total)",
        total_packets,
        elapsed.as_secs_f64(),
        total_packets as f64 / elapsed.as_secs_f64(),
        file_size / 1_048_576
    );

    Ok(source.len())
}

/// Detect pcap format by reading magic bytes
fn detect_pcap_format(path: &Path) -> Result<PcapFormat, String> {
    use std::io::Read;
    
    let mut file = File::open(path).map_err(|e| format!("Failed to open file: {e}"))?;
    let mut magic = [0u8; 4];
    file.read_exact(&mut magic).map_err(|e| format!("Failed to read magic: {e}"))?;

    // Check magic bytes
    match &magic {
        // Legacy pcap (little-endian)
        [0xd4, 0xc3, 0xb2, 0xa1] => Ok(PcapFormat::Legacy),
        // Legacy pcap (big-endian)
        [0xa1, 0xb2, 0xc3, 0xd4] => Ok(PcapFormat::Legacy),
        // Legacy pcap with nanosecond timestamps (little-endian)
        [0x4d, 0x3c, 0xb2, 0xa1] => Ok(PcapFormat::Legacy),
        // Legacy pcap with nanosecond timestamps (big-endian)
        [0xa1, 0xb2, 0x3c, 0x4d] => Ok(PcapFormat::Legacy),
        // pcapng (Section Header Block)
        [0x0a, 0x0d, 0x0d, 0x0a] => Ok(PcapFormat::PcapNG),
        _ => Err("Unknown pcap format".to_string()),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PcapFormat {
    Legacy,
    PcapNG,
}

/// Parse a pcap/pcapng file with incremental loading
///
/// Appends parsed lines directly to `source` in batches for progressive display.
/// Calls `progress_callback` periodically with progress updates.
///
/// Returns total number of packets parsed, or error.
pub fn parse_pcap_file_with_progress<P: AsRef<Path>>(
    path: P,
    source: &Arc<SourceData>,
    progress_callback: &ProgressCallback,
) -> Result<usize, String> {
    profiling::scope!("parse_pcap_file_with_progress");
    let path = path.as_ref();

    // Detect format and dispatch to appropriate parser
    let format = detect_pcap_format(path)?;
    
    let result = match format {
        PcapFormat::Legacy => parse_legacy_pcap(path, source, progress_callback),
        PcapFormat::PcapNG => parse_pcapng(path, source, progress_callback),
    };

    match &result {
        Ok(count) => log::info!("Parsed {count} packets from pcap file"),
        Err(e) => log::error!("Failed to parse pcap file: {e}"),
    }

    if source.is_empty() {
        Err("No valid packets found in pcap file".to_string())
    } else {
        result
    }
}
