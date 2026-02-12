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

//! BTSnoop (Bluetooth HCI log) file parser for Bluetooth traffic analysis.
//!
//! Supports Android btsnoop logs and btmon format.

use crate::core::log_file::ProgressCallback;
use crate::core::log_store::SourceData;

use super::line::{BtsnoopLogLine, LogLine};
use chrono::{DateTime, Local, TimeZone};
use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::sync::Arc;

/// Initial chunk size for incremental loading
const BTSNOOP_INITIAL_CHUNK_SIZE: usize = 1 << 12; // 4,096 packets
const BTSNOOP_MAX_CHUNK_SIZE: usize = 1 << 18; // 262,144 packets
const BTSNOOP_CHUNKS_BEFORE_GROWTH: usize = 3; // Double chunk size every 3 chunks

/// Represents a parsed HCI packet for display
#[derive(Debug, Clone)]
pub struct HciPacketInfo {
    /// Packet timestamp
    pub timestamp: DateTime<Local>,
    /// HCI packet type (Command, Event, ACL Data, SCO Data, etc.)
    pub packet_type: String,
    /// Direction (Sent/Received or Host→Controller/Controller→Host)
    pub direction: String,
    /// Packet length in bytes
    pub length: u32,
    /// Brief packet info (opcode, event code, handle, etc.)
    pub info: String,
}

impl HciPacketInfo {
    /// Format as a display message
    pub fn format_message(&self) -> String {
        if self.info.is_empty() {
            format!(
                "{} {} {} Len={}",
                self.packet_type, self.direction, self.info, self.length
            )
        } else {
            format!(
                "{} {} {} Len={}",
                self.packet_type, self.direction, self.info, self.length
            )
        }
    }

    /// Format as raw line (more detailed)
    pub fn format_raw(&self) -> String {
        format!(
            "[{}] {} {} {} Length={}",
            self.timestamp.format("%H:%M:%S%.6f"),
            self.packet_type,
            self.direction,
            self.info,
            self.length
        )
    }
}

/// Parse HCI packet data and extract packet info
fn parse_hci_packet(
    packet: &btsnoop::Packet,
    timestamp: DateTime<Local>,
) -> Option<HciPacketInfo> {
    profiling::scope!("parse_hci_packet");

    let data = &packet.packet_data;
    if data.is_empty() {
        return None;
    }

    // Determine direction from flags
    let direction = match packet.header.packet_flags.direction {
        btsnoop::DirectionFlag::Sent => "Sent",
        btsnoop::DirectionFlag::Received => "Rcvd",
    };

    // Parse HCI packet type (first byte for some formats, or from flags)
    let (packet_type, info) = parse_hci_type_and_info(data, &packet.header.packet_flags);

    Some(HciPacketInfo {
        timestamp,
        packet_type,
        direction: direction.to_string(),
        length: packet.header.original_length,
        info,
    })
}

/// Parse HCI packet type and extract basic info
fn parse_hci_type_and_info(data: &[u8], _flags: &btsnoop::PacketFlags) -> (String, String) {
    if data.is_empty() {
        return ("Unknown".to_string(), String::new());
    }

    // Parse based on HCI packet type indicator (first byte in most formats)
    match data.first() {
        Some(0x01) => {
            // HCI Command packet
            if data.len() >= 3 {
                let opcode = u16::from_le_bytes([data[1], data[2]]);
                let param_len = data[3];

                let cmd_name = get_hci_command_name(opcode);
                (
                    "HCI_CMD".to_string(),
                    format!("{cmd_name} (0x{opcode:04x}) ParamLen={param_len}"),
                )
            } else {
                ("HCI_CMD".to_string(), "Truncated".to_string())
            }
        }
        Some(0x02) => {
            // ACL Data - parse L2CAP layer
            if data.len() >= 5 {
                let handle = u16::from_le_bytes([data[1], data[2]]) & 0x0FFF;
                let pb_flag = (data[2] >> 4) & 0x03;
                let bc_flag = (data[2] >> 6) & 0x03;
                let acl_len = u16::from_le_bytes([data[3], data[4]]);
                
                // Parse L2CAP header if present (need at least 4 more bytes)
                if data.len() >= 9 && pb_flag != 0x01 {
                    // pb_flag 0x01 = continuation fragment, no L2CAP header
                    let l2cap_len = u16::from_le_bytes([data[5], data[6]]);
                    let l2cap_cid = u16::from_le_bytes([data[7], data[8]]);
                    let channel_name = get_l2cap_channel_name(l2cap_cid);
                    
                    // Parse L2CAP signaling commands if applicable
                    let l2cap_info = if (l2cap_cid == 0x0001 || l2cap_cid == 0x0005) && data.len() >= 10 {
                        let sig_code = data[9];
                        format!("{channel_name} {}", get_l2cap_signaling_code(sig_code))
                    } else {
                        channel_name.to_string()
                    };
                    
                    (
                        "ACL_DATA".to_string(),
                        format!("Handle=0x{handle:04x} L2CAP(Len={l2cap_len} CID=0x{l2cap_cid:04x} {l2cap_info})"),
                    )
                } else {
                    (
                        "ACL_DATA".to_string(),
                        format!("Handle=0x{handle:04x} PB={pb_flag} BC={bc_flag} Len={acl_len}"),
                    )
                }
            } else {
                ("ACL_DATA".to_string(), "Truncated".to_string())
            }
        }
        Some(0x03) => {
            // SCO Data
            if data.len() >= 4 {
                let handle = u16::from_le_bytes([data[1], data[2]]) & 0x0FFF;
                let data_len = data[3];
                (
                    "SCO_DATA".to_string(),
                    format!("Handle=0x{handle:04x} Len={data_len}"),
                )
            } else {
                ("SCO_DATA".to_string(), "Truncated".to_string())
            }
        }
        Some(0x04) => {
            // HCI Event
            if data.len() >= 3 {
                let event_code = data[1];
                let param_len = data[2];
                let event_name = get_hci_event_name(event_code);
                (
                    "HCI_EVT".to_string(),
                    format!("{event_name} (0x{event_code:02x}) ParamLen={param_len}"),
                )
            } else {
                ("HCI_EVT".to_string(), "Truncated".to_string())
            }
        }
        Some(0x05) => {
            // ISO Data
            ("ISO_DATA".to_string(), String::new())
        }
        _ => {
            // Unknown packet type
            ("HCI".to_string(), format!("Type=0x{:02x}", data[0]))
        }
    }
}

/// Get human-readable HCI command name from opcode
fn get_hci_command_name(opcode: u16) -> &'static str {
    match opcode {
        // Special
        0x0000 => "NOP",
        
        // Link Control Commands (OGF 0x01)
        0x0401 => "Inquiry",
        0x0402 => "Inquiry_Cancel",
        0x0403 => "Periodic_Inquiry_Mode",
        0x0404 => "Exit_Periodic_Inquiry_Mode",
        0x0405 => "Create_Connection",
        0x0406 => "Disconnect",
        0x0407 => "Add_SCO_Connection",
        0x0408 => "Accept_Connection_Request",
        0x0409 => "Reject_Connection_Request",
        0x040A => "Link_Key_Request_Reply",
        0x040B => "Authentication_Requested",
        0x040C => "Link_Key_Request_Negative_Reply",
        0x040D => "Set_Connection_Encryption",
        0x040F => "Change_Connection_Link_Key",
        0x0411 => "Remote_Name_Request",
        0x0419 => "Remote_Name_Request",
        0x041A => "Remote_Name_Request_Cancel",
        0x041B => "Read_Remote_Supported_Features",
        0x041C => "Read_Remote_Extended_Features",
        0x041D => "Read_Remote_Version_Information",
        0x041F => "Read_Remote_Extended_Features",
        0x0428 => "Setup_Synchronous_Connection",
        0x0429 => "Accept_Synchronous_Connection",
        
        // Link Policy Commands (OGF 0x02)
        0x0801 => "Hold_Mode",
        0x0802 => "Sniff_Mode",
        0x0803 => "Sniff_Mode",
        0x0804 => "Exit_Sniff_Mode",
        0x0805 => "Park_Mode",
        0x0806 => "Exit_Park_Mode",
        0x0807 => "QoS_Setup",
        0x0809 => "Role_Discovery",
        0x080B => "Switch_Role",
        0x080C => "Read_Link_Policy_Settings",
        0x080D => "Write_Link_Policy_Settings",
        0x080E => "Read_Default_Link_Policy_Settings",
        0x080F => "Write_Default_Link_Policy_Settings",
        0x0810 => "Flow_Specification",
        0x0811 => "Sniff_Subrating",
        
        // Controller & Baseband Commands (OGF 0x03)
        0x0C01 => "Set_Event_Mask",
        0x0C03 => "Reset",
        0x0C05 => "Set_Event_Filter",
        0x0C08 => "Flush",
        0x0C09 => "Read_PIN_Type",
        0x0C0A => "Write_PIN_Type",
        0x0C0B => "Create_New_Unit_Key",
        0x0C0D => "Read_Stored_Link_Key",
        0x0C11 => "Write_Stored_Link_Key",
        0x0C12 => "Delete_Stored_Link_Key",
        0x0C13 => "Change_Local_Name",
        0x0C14 => "Read_Local_Name",
        0x0C15 => "Read_Connection_Accept_Timeout",
        0x0C16 => "Write_Connection_Accept_Timeout",
        0x0C17 => "Read_Page_Timeout",
        0x0C18 => "Write_Page_Timeout",
        0x0C19 => "Read_Scan_Enable",
        0x0C1A => "Write_Scan_Enable",
        0x0C1B => "Read_Page_Scan_Activity",
        0x0C1C => "Write_Page_Scan_Activity",
        0x0C1D => "Read_Inquiry_Scan_Activity",
        0x0C1E => "Write_Inquiry_Scan_Activity",
        0x0C1F => "Read_Authentication_Enable",
        0x0C20 => "Write_Authentication_Enable",
        0x0C23 => "Read_Class_Of_Device",
        0x0C24 => "Write_Class_Of_Device",
        0x0C25 => "Read_Voice_Setting",
        0x0C26 => "Write_Voice_Setting",
        0x0C27 => "Read_Automatic_Flush_Timeout",
        0x0C28 => "Write_Automatic_Flush_Timeout",
        0x0C29 => "Read_Num_Broadcast_Retransmissions",
        0x0C2A => "Write_Num_Broadcast_Retransmissions",
        0x0C2B => "Read_Hold_Mode_Activity",
        0x0C2C => "Write_Hold_Mode_Activity",
        0x0C2D => "Read_Transmit_Power_Level",
        0x0C2E => "Read_SCO_Flow_Control_Enable",
        0x0C2F => "Write_SCO_Flow_Control_Enable",
        0x0C31 => "Set_Host_Controller_To_Host_Flow_Control",
        0x0C33 => "Host_Buffer_Size",
        0x0C35 => "Host_Number_Of_Completed_Packets",
        0x0C36 => "Read_Link_Supervision_Timeout",
        0x0C37 => "Write_Link_Supervision_Timeout",
        0x0C38 => "Read_Number_Of_Supported_IAC",
        0x0C39 => "Read_Current_IAC_LAP",
        0x0C3A => "Write_Current_IAC_LAP",
        0x0C3F => "Set_AFH_Host_Channel_Classification",
        0x0C42 => "Read_Inquiry_Scan_Type",
        0x0C43 => "Write_Inquiry_Scan_Type",
        0x0C44 => "Read_Inquiry_Mode",
        0x0C45 => "Write_Inquiry_Mode",
        0x0C46 => "Read_Page_Scan_Type",
        0x0C47 => "Write_Page_Scan_Type",
        0x0C48 => "Read_AFH_Channel_Assessment_Mode",
        0x0C49 => "Write_AFH_Channel_Assessment_Mode",
        0x0C51 => "Read_Extended_Inquiry_Response",
        0x0C52 => "Write_Extended_Inquiry_Response",
        0x0C53 => "Refresh_Encryption_Key",
        0x0C55 => "Read_Simple_Pairing_Mode",
        0x0C56 => "Write_Simple_Pairing_Mode",
        0x0C57 => "Read_Local_OOB_Data",
        0x0C58 => "Read_Inquiry_Response_Transmit_Power_Level",
        0x0C59 => "Write_Inquiry_Transmit_Power_Level",
        0x0C5A => "Read_Default_Erroneous_Data_Reporting",
        0x0C5B => "Write_Default_Erroneous_Data_Reporting",
        0x0C5F => "Enhanced_Flush",
        0x0C60 => "Send_Keypress_Notification",
        0x0C61 => "Read_Logical_Link_Accept_Timeout",
        0x0C62 => "Write_Logical_Link_Accept_Timeout",
        0x0C63 => "Set_Event_Mask_Page_2",
        0x0C64 => "Read_Location_Data",
        0x0C65 => "Write_Location_Data",
        0x0C66 => "Read_Flow_Control_Mode",
        0x0C67 => "Write_Flow_Control_Mode",
        0x0C68 => "Read_Enhance_Transmit_Power_Level",
        0x0C69 => "Read_Best_Effort_Flush_Timeout",
        0x0C6A => "Write_Best_Effort_Flush_Timeout",
        0x0C6B => "Short_Range_Mode",
        0x0C6C => "Read_LE_Host_Support",
        0x0C6D => "Write_LE_Host_Support",
        
        // Informational Parameters (OGF 0x04)
        0x1001 => "Read_Local_Version_Information",
        0x1002 => "Read_Local_Supported_Commands",
        0x1003 => "Read_Local_Supported_Features",
        0x1004 => "Read_Local_Extended_Features",
        0x1005 => "Read_Buffer_Size",
        0x1007 => "Read_Country_Code",
        0x1009 => "Read_BD_ADDR",
        0x100A => "Read_Data_Block_Size",
        0x100B => "Read_Local_Supported_Codecs",
        
        // Status Parameters (OGF 0x05)
        0x1401 => "Read_Failed_Contact_Counter",
        0x1402 => "Reset_Failed_Contact_Counter",
        0x1403 => "Read_Link_Quality",
        0x1405 => "Read_RSSI",
        0x1406 => "Read_AFH_Channel_Map",
        0x1407 => "Read_Clock",
        0x1408 => "Read_Encryption_Key_Size",
        
        // LE Controller Commands (OGF 0x08)
        0x2001 => "LE_Set_Event_Mask",
        0x2002 => "LE_Read_Buffer_Size",
        0x2003 => "LE_Read_Local_Supported_Features",
        0x2005 => "LE_Set_Random_Address",
        0x2006 => "LE_Set_Advertising_Parameters",
        0x2007 => "LE_Read_Advertising_Channel_Tx_Power",
        0x2008 => "LE_Set_Advertising_Data",
        0x2009 => "LE_Set_Scan_Response_Data",
        0x200A => "LE_Set_Advertising_Enable",
        0x200B => "LE_Set_Scan_Parameters",
        0x200C => "LE_Set_Scan_Enable",
        0x200D => "LE_Set_Scan_Enable",
        0x200E => "LE_Create_Connection",
        0x200F => "LE_Create_Connection_Cancel",
        0x2010 => "LE_Read_White_List_Size",
        0x2011 => "LE_Clear_White_List",
        0x2012 => "LE_Add_Device_To_White_List",
        0x2013 => "LE_Remove_Device_From_White_List",
        0x2014 => "LE_Connection_Update",
        0x2015 => "LE_Set_Host_Channel_Classification",
        0x2016 => "LE_Read_Channel_Map",
        0x2017 => "LE_Read_Remote_Features",
        0x2018 => "LE_Encrypt",
        0x2019 => "LE_Rand",
        0x201A => "LE_Start_Encryption",
        0x201B => "LE_Long_Term_Key_Request_Reply",
        0x201C => "LE_Long_Term_Key_Request_Negative_Reply",
        0x201D => "LE_Read_Supported_States",
        0x201E => "LE_Receiver_Test",
        0x201F => "LE_Transmitter_Test",
        0x2020 => "LE_Test_End",
        0x2021 => "LE_Remote_Connection_Parameter_Request_Reply",
        0x2022 => "LE_Remote_Connection_Parameter_Request_Negative_Reply",
        0x2023 => "LE_Set_Data_Length",
        0x2024 => "LE_Read_Suggested_Default_Data_Length",
        0x2025 => "LE_Write_Suggested_Default_Data_Length",
        0x2026 => "LE_Read_Local_P256_Public_Key",
        0x2027 => "LE_Generate_DHKey",
        0x2028 => "LE_Add_Device_To_Resolving_List",
        0x2029 => "LE_Remove_Device_From_Resolving_List",
        0x202A => "LE_Clear_Resolving_List",
        0x202B => "LE_Read_Resolving_List_Size",
        0x202C => "LE_Read_Peer_Resolvable_Address",
        0x202D => "LE_Read_Local_Resolvable_Address",
        0x202E => "LE_Set_Address_Resolution_Enable",
        0x202F => "LE_Set_Resolvable_Private_Address_Timeout",
        0x2030 => "LE_Read_Maximum_Data_Length",
        0x2031 => "LE_Read_PHY",
        0x2032 => "LE_Set_Default_PHY",
        0x2033 => "LE_Set_PHY",
        0x2034 => "LE_Enhanced_Receiver_Test",
        0x2035 => "LE_Enhanced_Transmitter_Test",
        0x2036 => "LE_Set_Advertising_Set_Random_Address",
        0x2037 => "LE_Set_Extended_Advertising_Parameters",
        0x2038 => "LE_Set_Extended_Advertising_Data",
        0x2039 => "LE_Set_Extended_Scan_Response_Data",
        0x203A => "LE_Set_Extended_Advertising_Enable",
        0x203B => "LE_Read_Maximum_Advertising_Data_Length",
        0x203C => "LE_Read_Number_Of_Supported_Advertising_Sets",
        0x203D => "LE_Remove_Advertising_Set",
        0x203E => "LE_Clear_Advertising_Sets",
        0x203F => "LE_Set_Periodic_Advertising_Parameters",
        0x2040 => "LE_Set_Periodic_Advertising_Data",
        0x2041 => "LE_Set_Periodic_Advertising_Enable",
        0x2042 => "LE_Set_Extended_Scan_Parameters",
        0x2043 => "LE_Set_Extended_Scan_Enable",
        0x2044 => "LE_Extended_Create_Connection",
        0x2045 => "LE_Periodic_Advertising_Create_Sync",
        0x2046 => "LE_Periodic_Advertising_Create_Sync_Cancel",
        0x2047 => "LE_Periodic_Advertising_Terminate_Sync",
        0x2048 => "LE_Add_Device_To_Periodic_Advertiser_List",
        0x2049 => "LE_Remove_Device_From_Periodic_Advertiser_List",
        0x204A => "LE_Clear_Periodic_Advertiser_List",
        0x204B => "LE_Read_Periodic_Advertiser_List_Size",
        0x204C => "LE_Read_Transmit_Power",
        0x204D => "LE_Read_RF_Path_Compensation",
        0x204E => "LE_Write_RF_Path_Compensation",
        0x204F => "LE_Set_Privacy_Mode",
        
        // Vendor Specific Commands (OGF 0x3F, opcodes 0xFC00-0xFFFF)
        0xFC00..=0xFFFF => "Vendor_Specific",
        
        _ => "Unknown_Command",
    }
}

/// Get human-readable HCI event name from event code
fn get_hci_event_name(event_code: u8) -> &'static str {
    match event_code {
        0x01 => "Inquiry_Complete",
        0x02 => "Inquiry_Result",
        0x03 => "Connection_Complete",
        0x04 => "Connection_Request",
        0x05 => "Disconnection_Complete",
        0x06 => "Authentication_Complete",
        0x07 => "Remote_Name_Request_Complete",
        0x08 => "Encryption_Change",
        0x09 => "Change_Connection_Link_Key_Complete",
        0x0A => "Master_Link_Key_Complete",
        0x0B => "Read_Remote_Supported_Features_Complete",
        0x0C => "Read_Remote_Version_Information_Complete",
        0x0D => "QoS_Setup_Complete",
        0x0E => "Command_Complete",
        0x0F => "Command_Status",
        0x10 => "Hardware_Error",
        0x11 => "Flush_Occurred",
        0x12 => "Role_Change",
        0x13 => "Number_Of_Completed_Packets",
        0x14 => "Mode_Change",
        0x15 => "Return_Link_Keys",
        0x16 => "PIN_Code_Request",
        0x17 => "Link_Key_Request",
        0x18 => "Link_Key_Notification",
        0x19 => "Loopback_Command",
        0x1A => "Data_Buffer_Overflow",
        0x1B => "Max_Slots_Change",
        0x1C => "Read_Clock_Offset_Complete",
        0x1D => "Connection_Packet_Type_Changed",
        0x1E => "QoS_Violation",
        0x20 => "Page_Scan_Repetition_Mode_Change",
        0x21 => "Flow_Specification_Complete",
        0x22 => "Inquiry_Result_With_RSSI",
        0x23 => "Read_Remote_Extended_Features_Complete",
        0x2C => "Synchronous_Connection_Complete",
        0x2D => "Synchronous_Connection_Changed",
        0x2E => "Sniff_Subrating",
        0x2F => "Extended_Inquiry_Result",
        0x30 => "Encryption_Key_Refresh_Complete",
        0x31 => "IO_Capability_Request",
        0x32 => "IO_Capability_Response",
        0x33 => "User_Confirmation_Request",
        0x34 => "User_Passkey_Request",
        0x35 => "Remote_OOB_Data_Request",
        0x36 => "Simple_Pairing_Complete",
        0x38 => "Link_Supervision_Timeout_Changed",
        0x39 => "Enhanced_Flush_Complete",
        0x3B => "User_Passkey_Notification",
        0x3C => "Keypress_Notification",
        0x3D => "Remote_Host_Supported_Features_Notification",
        0x3E => "LE_Meta_Event",
        0x40 => "Physical_Link_Complete",
        0x41 => "Channel_Selected",
        0x42 => "Disconnection_Physical_Link_Complete",
        0x43 => "Physical_Link_Loss_Early_Warning",
        0x44 => "Physical_Link_Recovery",
        0x45 => "Logical_Link_Complete",
        0x46 => "Disconnection_Logical_Link_Complete",
        0x47 => "Flow_Spec_Modify_Complete",
        0x48 => "Number_Of_Completed_Data_Blocks",
        0x49 => "AMP_Start_Test",
        0x4A => "AMP_Test_End",
        0x4B => "AMP_Receiver_Report",
        0x4C => "Short_Range_Mode_Change_Complete",
        0x4D => "AMP_Status_Change",
        0x4E => "Triggered_Clock_Capture",
        0x4F => "Synchronization_Train_Complete",
        0x50 => "Synchronization_Train_Received",
        0x51 => "Connectionless_Slave_Broadcast_Receive",
        0x52 => "Connectionless_Slave_Broadcast_Timeout",
        0x53 => "Truncated_Page_Complete",
        0x54 => "Slave_Page_Response_Timeout",
        0x55 => "Connectionless_Slave_Broadcast_Channel_Map_Change",
        0x56 => "Inquiry_Response_Notification",
        0x57 => "Authenticated_Payload_Timeout_Expired",
        0x58 => "SAM_Status_Change",
        0xFE => "Bluetooth_Logo_Testing",
        0xFF => "Vendor_Specific",
        _ => "Unknown_Event",
    }
}

/// Get human-readable L2CAP channel name from CID
fn get_l2cap_channel_name(cid: u16) -> &'static str {
    match cid {
        0x0001 => "L2CAP_Signaling",
        0x0002 => "Connectionless",
        0x0003 => "AMP_Manager",
        0x0004 => "ATT",
        0x0005 => "LE_Signaling",
        0x0006 => "SMP",
        0x0007 => "SMP_BR/EDR",
        0x003F => "AMP_Test",
        0x0040..=0x007F => "Dynamically_Allocated",
        _ => "Unknown_Channel",
    }
}

/// Get human-readable L2CAP signaling command code
fn get_l2cap_signaling_code(code: u8) -> &'static str {
    match code {
        0x01 => "Command_Reject",
        0x02 => "Connection_Request",
        0x03 => "Connection_Response",
        0x04 => "Configuration_Request",
        0x05 => "Configuration_Response",
        0x06 => "Disconnection_Request",
        0x07 => "Disconnection_Response",
        0x08 => "Echo_Request",
        0x09 => "Echo_Response",
        0x0A => "Information_Request",
        0x0B => "Information_Response",
        0x0C => "Create_Channel_Request",
        0x0D => "Create_Channel_Response",
        0x0E => "Move_Channel_Request",
        0x0F => "Move_Channel_Response",
        0x10 => "Move_Channel_Confirmation_Request",
        0x11 => "Move_Channel_Confirmation_Response",
        0x12 => "Connection_Parameter_Update_Request",
        0x13 => "Connection_Parameter_Update_Response",
        0x14 => "LE_Credit_Based_Connection_Request",
        0x15 => "LE_Credit_Based_Connection_Response",
        0x16 => "LE_Flow_Control_Credit",
        0x17 => "Credit_Based_Connection_Request",
        0x18 => "Credit_Based_Connection_Response",
        0x19 => "Credit_Based_Reconfigure_Request",
        0x1A => "Credit_Based_Reconfigure_Response",
        _ => "Unknown_Signaling",
    }
}

/// Parse a btsnoop file with incremental loading
///
/// Appends parsed lines directly to `source` in batches for progressive display.
/// Calls `progress_callback` periodically with progress updates.
///
/// Returns total number of packets parsed, or error.
pub fn parse_btsnoop_file_with_progress<P: AsRef<Path>>(
    path: P,
    source: &Arc<SourceData>,
    progress_callback: &ProgressCallback,
) -> Result<usize, String> {
    profiling::scope!("parse_btsnoop_file_with_progress");
    let path = path.as_ref();
    log::info!("Starting btsnoop parsing: {}", path.display());

    let start_time = std::time::Instant::now();

    // Read entire file (btsnoop crate requires byte slice)
    let mut file = File::open(path).map_err(|e| format!("Failed to open btsnoop file: {e}"))?;
    let file_size = file
        .metadata()
        .map(|m| m.len())
        .unwrap_or(0);

    let mut buffer = Vec::new();
    file.read_to_end(&mut buffer)
        .map_err(|e| format!("Failed to read btsnoop file: {e}"))?;

    log::info!("Read {} bytes into memory", buffer.len());

    // Parse the btsnoop file
    let btsnoop_file = btsnoop::parse_btsnoop_file(&buffer)
        .map_err(|e| format!("Failed to parse btsnoop file: {e:?}"))?;

    log::info!(
        "Parsed btsnoop header: datalink={:?}, {} packets",
        btsnoop_file.header.datalink_type,
        btsnoop_file.packets.len()
    );

    // Convert packets to log lines incrementally
    let mut chunk_lines = Vec::new();
    let mut line_number = 1;
    let mut last_log_time = std::time::Instant::now();
    let mut packets_since_log = 0;
    let mut chunk_count = 0;
    let mut current_chunk_size = BTSNOOP_INITIAL_CHUNK_SIZE;

    for packet in &btsnoop_file.packets {
        // Convert timestamp from microseconds since epoch
        let timestamp_micros = packet.header.timestamp_microseconds;
        let timestamp_secs = i64::try_from(timestamp_micros / 1_000_000)
            .unwrap_or_else(|_| Local::now().timestamp());
        let timestamp_nanos = ((timestamp_micros % 1_000_000) * 1_000) as u32;

        let timestamp = Local
            .timestamp_opt(timestamp_secs, timestamp_nanos)
            .single()
            .unwrap_or_else(Local::now);

        if let Some(hci_info) = parse_hci_packet(packet, timestamp) {
            let log_line = LogLine::Btsnoop(BtsnoopLogLine::new(hci_info, line_number));
            chunk_lines.push(log_line);
            line_number += 1;
            packets_since_log += 1;

            if chunk_lines.len() >= current_chunk_size {
                source.append_lines(std::mem::take(&mut chunk_lines));
                chunk_count += 1;

                // Grow chunk size exponentially
                if chunk_count % BTSNOOP_CHUNKS_BEFORE_GROWTH == 0
                    && current_chunk_size < BTSNOOP_MAX_CHUNK_SIZE
                {
                    current_chunk_size = (current_chunk_size * 2).min(BTSNOOP_MAX_CHUNK_SIZE);
                    log::debug!("Increased chunk size to {} packets", current_chunk_size);
                }

                let progress = source.len() as f32 / btsnoop_file.packets.len() as f32;
                progress_callback(
                    progress,
                    &format!("Parsing btsnoop... ({} packets)", source.len()),
                );

                // Log performance every 5 seconds
                let now = std::time::Instant::now();
                if now.duration_since(last_log_time).as_secs() >= 5 {
                    let elapsed = now.duration_since(start_time).as_secs_f64();
                    let rate = packets_since_log as f64
                        / now.duration_since(last_log_time).as_secs_f64();
                    log::info!(
                        "Parsed {} packets in {:.2}s ({:.0} pkt/s)",
                        source.len(),
                        elapsed,
                        rate
                    );
                    last_log_time = now;
                    packets_since_log = 0;
                }
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
        "Completed btsnoop parsing: {} packets in {:.2}s ({:.0} pkt/s, {} MB total)",
        total_packets,
        elapsed.as_secs_f64(),
        total_packets as f64 / elapsed.as_secs_f64(),
        file_size / 1_048_576
    );

    if source.is_empty() {
        Err("No valid HCI packets found in btsnoop file".to_string())
    } else {
        Ok(source.len())
    }
}
