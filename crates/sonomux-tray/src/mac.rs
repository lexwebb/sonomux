//! macOS menu bar extra: an NSStatusItem with an SF Symbol and the speaker volume or headset
//! battery, and a native menu with the status, a speakers slider, Mute and Open Sonomux.
//!
//! AppKit runs on the main thread; the daemon loop runs on a Tokio thread and hands each new
//! view to the main queue.

use crate::{daemon_loop, open_gui, Cmd, View};
use dispatch2::DispatchQueue;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{define_class, msg_send, sel, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSControlStateValueOff, NSControlStateValueOn,
    NSImage, NSMenu, NSMenuItem, NSSlider, NSStatusBar, NSStatusItem, NSVariableStatusItemLength,
    NSView,
};
use objc2_foundation::{NSObject, NSObjectProtocol, NSPoint, NSRect, NSSize, NSString};
use sonomux_core::status::Indicator;
use std::cell::RefCell;
use std::sync::OnceLock;
use tokio::sync::mpsc::{unbounded_channel, UnboundedSender};

static CMDS: OnceLock<UnboundedSender<Cmd>> = OnceLock::new();

fn send(cmd: Cmd) {
    if let Some(tx) = CMDS.get() {
        let _ = tx.send(cmd);
    }
}

define_class!(
    // Receives menu actions.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "SonomuxTrayTarget"]
    struct Target;

    unsafe impl NSObjectProtocol for Target {}

    impl Target {
        #[unsafe(method(openSonomux:))]
        fn open_sonomux(&self, _sender: Option<&AnyObject>) {
            open_gui();
        }

        #[unsafe(method(toggleMute:))]
        fn toggle_mute(&self, _sender: Option<&AnyObject>) {
            send(Cmd::ToggleMute);
        }

        #[unsafe(method(volumeChanged:))]
        fn volume_changed(&self, sender: Option<&NSSlider>) {
            if let Some(slider) = sender {
                send(Cmd::SetVolume(slider.doubleValue()));
            }
        }

        #[unsafe(method(quit:))]
        fn quit(&self, _sender: Option<&AnyObject>) {
            let mtm = MainThreadMarker::from(self);
            NSApplication::sharedApplication(mtm).terminate(None);
        }
    }
);

impl Target {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(());
        unsafe { msg_send![super(this), init] }
    }
}

/// Everything the main thread updates. Lives in a thread-local on the main thread.
struct Ui {
    item: Retained<NSStatusItem>,
    lines: Vec<Retained<NSMenuItem>>,
    mute: Retained<NSMenuItem>,
    slider: Retained<NSSlider>,
    _target: Retained<Target>,
}

thread_local! {
    static UI: RefCell<Option<Ui>> = const { RefCell::new(None) };
}

fn symbol(indicator: Option<Indicator>) -> &'static str {
    match indicator {
        Some(Indicator::Headset) => "headphones",
        Some(Indicator::Muted) => "speaker.slash.fill",
        Some(Indicator::Speakers) => "speaker.wave.2.fill",
        Some(Indicator::Off) | None => "speaker.badge.exclamationmark",
    }
}

fn menu_item(
    mtm: MainThreadMarker,
    title: &str,
    action: Option<objc2::runtime::Sel>,
    target: &Target,
) -> Retained<NSMenuItem> {
    let item = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(mtm),
            &NSString::from_str(title),
            action,
            &NSString::from_str(""),
        )
    };
    if action.is_some() {
        unsafe { item.setTarget(Some(target)) };
    }
    item
}

fn build(mtm: MainThreadMarker) -> Ui {
    let target = Target::new(mtm);
    let item = NSStatusBar::systemStatusBar().statusItemWithLength(NSVariableStatusItemLength);
    let menu = NSMenu::new(mtm);
    menu.setAutoenablesItems(false);

    let title = menu_item(mtm, "Sonomux", None, &target);
    title.setEnabled(false);
    menu.addItem(&title);
    let lines: Vec<_> = (0..4)
        .map(|_| {
            let l = menu_item(mtm, "", None, &target);
            l.setEnabled(false);
            menu.addItem(&l);
            l
        })
        .collect();
    menu.addItem(&NSMenuItem::separatorItem(mtm));

    // Speakers slider: a plain NSSlider hosted in a menu item.
    let label = menu_item(mtm, "Speakers", None, &target);
    label.setEnabled(false);
    menu.addItem(&label);
    let host = NSView::initWithFrame(
        NSView::alloc(mtm),
        NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(240.0, 28.0)),
    );
    let slider = NSSlider::initWithFrame(
        NSSlider::alloc(mtm),
        NSRect::new(NSPoint::new(18.0, 4.0), NSSize::new(204.0, 20.0)),
    );
    slider.setMinValue(0.0);
    slider.setMaxValue(100.0);
    slider.setContinuous(true);
    unsafe {
        slider.setTarget(Some(&target));
        slider.setAction(Some(sel!(volumeChanged:)));
    }
    host.addSubview(&slider);
    let slider_item = menu_item(mtm, "", None, &target);
    slider_item.setView(Some(&host));
    menu.addItem(&slider_item);

    let mute = menu_item(mtm, "Mute speakers", Some(sel!(toggleMute:)), &target);
    menu.addItem(&mute);
    menu.addItem(&NSMenuItem::separatorItem(mtm));
    menu.addItem(&menu_item(
        mtm,
        "Open Sonomux…",
        Some(sel!(openSonomux:)),
        &target,
    ));
    menu.addItem(&menu_item(
        mtm,
        "Quit Sonomux Tray",
        Some(sel!(quit:)),
        &target,
    ));
    item.setMenu(Some(&menu));

    Ui {
        item,
        lines,
        mute,
        slider,
        _target: target,
    }
}

fn show(ui: &Ui, view: &View, mtm: MainThreadMarker) {
    let indicator = view.summary.as_ref().map(|s| s.indicator);
    if let Some(button) = ui.item.button(mtm) {
        let desc = NSString::from_str("Sonomux");
        if let Some(img) = NSImage::imageWithSystemSymbolName_accessibilityDescription(
            &NSString::from_str(symbol(indicator)),
            Some(&desc),
        ) {
            img.setTemplate(true);
            button.setImage(Some(&img));
        }
        let label = view
            .summary
            .as_ref()
            .map_or("Offline".to_string(), |s| s.label.clone());
        button.setTitle(&NSString::from_str(&format!(" {}", label)));
    }
    match &view.summary {
        Some(s) => {
            for (item, line) in ui.lines.iter().zip(&s.lines) {
                item.setTitle(&NSString::from_str(line));
                item.setHidden(false);
            }
        }
        None => {
            for (i, item) in ui.lines.iter().enumerate() {
                item.setTitle(&NSString::from_str("Daemon not running"));
                item.setHidden(i > 0);
            }
        }
    }
    let muted = indicator == Some(Indicator::Muted);
    ui.mute.setState(if muted {
        NSControlStateValueOn
    } else {
        NSControlStateValueOff
    });
    ui.mute.setEnabled(view.summary.is_some());
    ui.slider.setEnabled(view.summary.is_some());
    ui.slider.setDoubleValue(view.speaker_volume);
}

pub fn run() -> anyhow::Result<()> {
    let mtm = MainThreadMarker::new().expect("must run on the main thread");
    let app = NSApplication::sharedApplication(mtm);
    // No Dock icon or app menu: just the menu bar extra.
    app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);

    let ui = build(mtm);
    show(&ui, &View::default(), mtm);
    UI.with(|u| *u.borrow_mut() = Some(ui));

    let (tx, rx) = unbounded_channel();
    let _ = CMDS.set(tx);
    std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
        rt.block_on(daemon_loop(
            |view| {
                DispatchQueue::main().exec_async(move || {
                    let mtm = MainThreadMarker::new().expect("main queue is the main thread");
                    UI.with(|u| {
                        if let Some(ui) = u.borrow().as_ref() {
                            show(ui, &view, mtm);
                        }
                    });
                });
            },
            rx,
        ));
    });

    app.run();
    Ok(())
}
