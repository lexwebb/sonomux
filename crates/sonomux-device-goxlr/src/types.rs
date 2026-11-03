use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Channel {
    Mic,
    LineIn,
    Console,
    System,
    Game,
    Chat,
    Sample,
    Music,
    Headphones,
    MicMonitor,
    LineOut,
}

impl Channel {
    pub fn as_str(&self) -> &'static str {
        match self {
            Channel::Mic => "Mic",
            Channel::LineIn => "LineIn",
            Channel::Console => "Console",
            Channel::System => "System",
            Channel::Game => "Game",
            Channel::Chat => "Chat",
            Channel::Sample => "Sample",
            Channel::Music => "Music",
            Channel::Headphones => "Headphones",
            Channel::MicMonitor => "MicMonitor",
            Channel::LineOut => "LineOut",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum InputDevice {
    Microphone,
    Chat,
    Music,
    Game,
    Console,
    LineIn,
    System,
    Samples,
}

impl InputDevice {
    pub fn as_str(&self) -> &'static str {
        match self {
            InputDevice::Microphone => "Microphone",
            InputDevice::Chat => "Chat",
            InputDevice::Music => "Music",
            InputDevice::Game => "Game",
            InputDevice::Console => "Console",
            InputDevice::LineIn => "LineIn",
            InputDevice::System => "System",
            InputDevice::Samples => "Samples",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum OutputDevice {
    Headphones,
    BroadcastMix,
    ChatMic,
    Sampler,
    LineOut,
    StreamMix2,
}

impl OutputDevice {
    pub fn as_str(&self) -> &'static str {
        match self {
            OutputDevice::Headphones => "Headphones",
            OutputDevice::BroadcastMix => "BroadcastMix",
            OutputDevice::ChatMic => "ChatMic",
            OutputDevice::Sampler => "Sampler",
            OutputDevice::LineOut => "LineOut",
            OutputDevice::StreamMix2 => "StreamMix2",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MixerLevels {
    pub volumes: HashMap<String, u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MixerData {
    pub levels: MixerLevels,
    pub router: HashMap<String, HashMap<String, bool>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaemonStatus {
    pub mixers: HashMap<String, MixerData>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DaemonResponse {
    Status(DaemonStatus),
    Ok,
    Error(String),
}
