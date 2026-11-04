//! Linux tray icon over StatusNotifierItem (Waybar, KDE, ...). Waybar's tray shows icons only,
//! so the Omarchy bar keeps its text module; this adds the menu.
//!
//! Click: open Sonomux. Middle-click: mute speakers. Scroll: speakers volume.

use crate::{daemon_loop, open_gui, Cmd, View};
use ksni::menu::{CheckmarkItem, StandardItem};
use ksni::{MenuItem, ToolTip, TrayMethods};
use sonomux_core::status::Indicator;
use tokio::sync::mpsc::{unbounded_channel, UnboundedSender};

struct SonomuxTray {
    view: View,
    cmds: UnboundedSender<Cmd>,
}

impl SonomuxTray {
    fn send(&self, cmd: Cmd) {
        let _ = self.cmds.send(cmd);
    }
}

impl ksni::Tray for SonomuxTray {
    fn id(&self) -> String {
        "sonomux".into()
    }

    fn title(&self) -> String {
        "Sonomux".into()
    }

    fn icon_name(&self) -> String {
        match self.view.summary.as_ref().map(|s| s.indicator) {
            Some(Indicator::Headset) => "audio-headphones",
            Some(Indicator::Muted) => "audio-volume-muted",
            Some(Indicator::Speakers) => "audio-volume-high",
            Some(Indicator::Off) | None => "audio-volume-muted-blocking",
        }
        .into()
    }

    fn tool_tip(&self) -> ToolTip {
        let (title, description) = match &self.view.summary {
            Some(s) => (format!("Sonomux · {}", s.label), s.lines.join("\n")),
            None => ("Sonomux".into(), "Daemon not running".into()),
        };
        ToolTip {
            title,
            description,
            ..Default::default()
        }
    }

    fn activate(&mut self, _x: i32, _y: i32) {
        open_gui();
    }

    fn secondary_activate(&mut self, _x: i32, _y: i32) {
        self.send(Cmd::ToggleMute);
    }

    fn scroll(&mut self, delta: i32, _orientation: ksni::Orientation) {
        // Hosts send roughly ±120 per notch; 5% a notch.
        let step = if delta > 0 { -5.0 } else { 5.0 };
        let v = (self.view.speaker_volume + step).clamp(0.0, 100.0);
        self.view.speaker_volume = v;
        self.send(Cmd::SetVolume(v));
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        let online = self.view.summary.is_some();
        let lines = match &self.view.summary {
            Some(s) => s.lines.clone(),
            None => vec!["Daemon not running".into()],
        };
        let mut items: Vec<MenuItem<Self>> = lines
            .into_iter()
            .map(|label| {
                StandardItem {
                    label,
                    enabled: false,
                    ..Default::default()
                }
                .into()
            })
            .collect();
        items.push(MenuItem::Separator);
        items.push(
            CheckmarkItem {
                label: "Mute speakers".into(),
                enabled: online,
                checked: self.view.summary.as_ref().map(|s| s.indicator) == Some(Indicator::Muted),
                activate: Box::new(|t: &mut Self| t.send(Cmd::ToggleMute)),
                ..Default::default()
            }
            .into(),
        );
        items.push(MenuItem::Separator);
        items.push(
            StandardItem {
                label: "Open Sonomux…".into(),
                activate: Box::new(|_| open_gui()),
                ..Default::default()
            }
            .into(),
        );
        items.push(
            StandardItem {
                label: "Quit Sonomux Tray".into(),
                activate: Box::new(|_| std::process::exit(0)),
                ..Default::default()
            }
            .into(),
        );
        items
    }
}

pub fn run() -> anyhow::Result<()> {
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async {
        let (tx, rx) = unbounded_channel();
        let handle = SonomuxTray {
            view: View::default(),
            cmds: tx,
        }
        .spawn()
        .await?;
        daemon_loop(
            move |view| {
                let h = handle.clone();
                tokio::spawn(async move {
                    h.update(|t| t.view = view).await;
                });
            },
            rx,
        )
        .await;
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ksni::Tray;
    use sonomux_core::status::summarize;

    fn tray(view: View) -> (SonomuxTray, tokio::sync::mpsc::UnboundedReceiver<Cmd>) {
        let (tx, rx) = unbounded_channel();
        (SonomuxTray { view, cmds: tx }, rx)
    }

    #[test]
    fn shows_state_like_the_omarchy_widget() {
        let (t, _) = tray(View::default());
        assert_eq!(t.icon_name(), "audio-volume-muted-blocking");
        assert_eq!(t.tool_tip().description, "Daemon not running");
        assert_eq!(t.menu().len(), 6); // status, sep, mute, sep, open, quit

        let (t, _) = tray(View {
            summary: Some(summarize(true, "Standby", false, 0, 74.0, false)),
            speaker_volume: 74.0,
        });
        assert_eq!(t.icon_name(), "audio-volume-high");
        assert_eq!(t.tool_tip().title, "Sonomux · 74%");
        assert_eq!(t.menu().len(), 9); // 4 status lines, sep, mute, sep, open, quit

        let (t, _) = tray(View {
            summary: Some(summarize(true, "Active", true, 82, 0.0, false)),
            speaker_volume: 0.0,
        });
        assert_eq!(t.icon_name(), "audio-headphones");
    }

    #[test]
    fn scroll_and_middle_click_drive_the_speakers() {
        let (mut t, mut rx) = tray(View {
            summary: Some(summarize(true, "Standby", false, 0, 98.0, false)),
            speaker_volume: 98.0,
        });
        t.scroll(-120, ksni::Orientation::Vertical);
        assert!(matches!(rx.try_recv(), Ok(Cmd::SetVolume(v)) if v == 100.0));
        t.scroll(120, ksni::Orientation::Vertical);
        assert!(matches!(rx.try_recv(), Ok(Cmd::SetVolume(v)) if v == 95.0));
        t.secondary_activate(0, 0);
        assert!(matches!(rx.try_recv(), Ok(Cmd::ToggleMute)));
    }
}
