//! How Sonomux sums up its state in one glance: the Omarchy/Waybar widget, `sonomuxctl` and
//! the tray all use this, so they never disagree.

/// What the indicator shows, most important first.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Indicator {
    /// Headset on: shows its battery.
    Headset,
    /// GoXLR not reachable.
    Off,
    /// Desk speakers at 0.
    Muted,
    /// Desk speakers on: shows their volume.
    Speakers,
}

impl Indicator {
    /// Waybar CSS class.
    pub fn class(self) -> &'static str {
        match self {
            Indicator::Headset => "headset",
            Indicator::Off => "disconnected",
            Indicator::Muted => "muted",
            Indicator::Speakers => "speakers",
        }
    }

    /// Nerd Font glyph used by the Omarchy bar.
    pub fn glyph(self) -> &'static str {
        match self {
            Indicator::Headset => "󰋋",
            Indicator::Off => "󰓄",
            Indicator::Muted => "󰝟",
            Indicator::Speakers => "󰓃",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Summary {
    pub indicator: Indicator,
    /// Short text next to the icon: "74%", "Muted", "Off", headset battery or "On".
    pub label: String,
    /// 0-100 for bars that draw a level.
    pub percentage: u64,
    /// Detail lines for a tooltip or menu.
    pub lines: Vec<String>,
}

/// Build the summary from the daemon's `get_status` tuple.
pub fn summarize(
    goxlr_up: bool,
    headset_status: &str,
    headset_active: bool,
    battery: u8,
    speaker_volume: f64,
    loopback_active: bool,
) -> Summary {
    let (indicator, label, percentage) = if headset_active {
        let label = if battery > 0 {
            format!("{}%", battery)
        } else {
            "On".into()
        };
        (Indicator::Headset, label, battery as f64)
    } else if !goxlr_up {
        (Indicator::Off, "Off".into(), 0.0)
    } else if speaker_volume <= 0.0 {
        (Indicator::Muted, "Muted".into(), 0.0)
    } else {
        (
            Indicator::Speakers,
            format!("{:.0}%", speaker_volume),
            speaker_volume,
        )
    };
    let battery_text = if headset_active && battery > 0 {
        format!("{}%", battery)
    } else {
        "Unavailable".into()
    };
    Summary {
        indicator,
        label,
        percentage: percentage.round() as u64,
        lines: vec![
            format!(
                "GoXLR: {}",
                if goxlr_up {
                    "Connected"
                } else {
                    "Disconnected"
                }
            ),
            format!("Headset: {} (Battery: {})", headset_status, battery_text),
            format!("Desk Speakers: {:.0}%", speaker_volume),
            format!(
                "Feed: {}",
                if loopback_active { "Streaming" } else { "Idle" }
            ),
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_the_most_important_state() {
        assert_eq!(
            summarize(true, "Active", true, 82, 0.0, true).indicator,
            Indicator::Headset
        );
        assert_eq!(summarize(true, "Active", true, 82, 0.0, true).label, "82%");
        assert_eq!(summarize(true, "Active", true, 0, 50.0, false).label, "On");
        assert_eq!(
            summarize(false, "Standby", false, 0, 74.0, false).indicator,
            Indicator::Off
        );
        assert_eq!(
            summarize(true, "Standby", false, 0, 0.0, false).indicator,
            Indicator::Muted
        );
        let s = summarize(true, "Standby", false, 0, 74.4, false);
        assert_eq!(
            (s.indicator, s.label.as_str(), s.percentage),
            (Indicator::Speakers, "74%", 74)
        );
        assert_eq!(s.lines[0], "GoXLR: Connected");
        assert_eq!(s.lines[1], "Headset: Standby (Battery: Unavailable)");
    }
}
