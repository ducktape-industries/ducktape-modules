//! A view that opens the microphone and the camera through the device
//! doors, and shows what each opened in and how much has arrived. The
//! smallest working use of `media.devices`, `audio.capture` and
//! `video.capture`, and the shape a real view copies: one subscription per
//! device, its FIRST item the mode, every later item the samples or the
//! frame; dropping the task closes the device.
//!
//! Served in place of a program's view during development:
//! `cargo build --target wasm32-unknown-unknown --example media_probe`, then
//! copy it to `$DUCKTAPE_VIEWS_DIR/<program>_view.wasm`.

use futures::StreamExt as _;
use serde::{Deserialize, Serialize};
use view_guest::doors::{AudioCapture, AudioItem, Device, MediaDevices, VideoCapture, VideoItem};
use view_guest::{
    div, ClickEvent, Context, InteractiveElement, IntoElement, ParentElement, Render,
    StatefulInteractiveElement, Styled, Task, View, Window,
};

#[derive(Serialize, Deserialize, Default)]
pub struct MediaProbe {
    devices: Vec<String>,
    microphone: Capture,
    camera: Capture,
    notice: String,
}

/// One device's state; the task is the open device, and never snapshots
/// (a restored view starts with both closed).
#[derive(Serialize, Deserialize, Default)]
struct Capture {
    opened: String,
    items: u64,
    bytes: u64,
    #[serde(skip)]
    task: Option<Task<()>>,
}

impl View for MediaProbe {
    fn new(_: &mut Window, cx: &mut Context<Self>) -> Self {
        cx.spawn(async move |this, cx| {
            let listed = cx.host().ask::<MediaDevices>(()).await;
            let _ = this.update(cx, |view, cx| {
                match listed {
                    Ok(devices) => {
                        view.devices = devices
                            .iter()
                            .map(|Device { kind, name, .. }| format!("{kind}: {name}"))
                            .collect()
                    }
                    Err(refusal) => view.notice = refusal.sentence,
                }
                cx.notify();
            });
        })
        .detach();
        Self::default()
    }
}

impl MediaProbe {
    fn listen(&mut self, cx: &mut Context<Self>) {
        let mut items = cx.host().subscribe::<AudioCapture>(Default::default());
        self.microphone = Capture::default();
        self.microphone.task = Some(cx.spawn(async move |this, cx| {
            while let Some(item) = items.next().await {
                let stop = this
                    .update(cx, |view, cx| {
                        match item {
                            Ok(AudioItem::Opened(mode)) => {
                                view.microphone.opened =
                                    format!("{} Hz × {}", mode.rate, mode.channels)
                            }
                            Ok(AudioItem::Samples(pcm)) => {
                                view.microphone.items += 1;
                                view.microphone.bytes += pcm.len() as u64;
                            }
                            Err(refusal) => view.notice = refusal.sentence,
                        }
                        cx.notify();
                    })
                    .is_err();
                if stop {
                    break;
                }
            }
        }));
    }

    fn watch(&mut self, cx: &mut Context<Self>) {
        let mut items = cx.host().subscribe::<VideoCapture>(Default::default());
        self.camera = Capture::default();
        self.camera.task = Some(cx.spawn(async move |this, cx| {
            while let Some(item) = items.next().await {
                let stop = this
                    .update(cx, |view, cx| {
                        match item {
                            Ok(VideoItem::Opened(framing)) => {
                                view.camera.opened = format!(
                                    "{}×{} @{} {}",
                                    framing.width, framing.height, framing.fps, framing.format
                                )
                            }
                            Ok(VideoItem::Frame(frame)) => {
                                view.camera.items += 1;
                                view.camera.bytes += frame.len() as u64;
                            }
                            Err(refusal) => view.notice = refusal.sentence,
                        }
                        cx.notify();
                    })
                    .is_err();
                if stop {
                    break;
                }
            }
        }));
    }
}

fn row(
    id: &'static str,
    label: &str,
    capture: &Capture,
    toggle: impl Fn(&ClickEvent, &mut Window, &mut view_guest::App) + 'static,
) -> impl IntoElement {
    let open = capture.task.is_some();
    div()
        .flex()
        .gap_2()
        .child(div().id(id).on_click(toggle).child(if open {
            format!("Stop {label}")
        } else {
            format!("Start {label}")
        }))
        .child(if open {
            format!(
                "{} · {} items · {} bytes",
                capture.opened, capture.items, capture.bytes
            )
        } else {
            "closed".to_owned()
        })
}

impl Render for MediaProbe {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let microphone = cx.listener(|view, _: &ClickEvent, _, cx| {
            match view.microphone.task.take() {
                Some(_) => view.microphone = Capture::default(),
                None => view.listen(cx),
            }
            cx.notify();
        });
        let camera = cx.listener(|view, _: &ClickEvent, _, cx| {
            match view.camera.task.take() {
                Some(_) => view.camera = Capture::default(),
                None => view.watch(cx),
            }
            cx.notify();
        });
        div()
            .flex()
            .flex_col()
            .gap_2()
            .p_4()
            .children(self.devices.iter().cloned())
            .child(row(
                "microphone",
                "microphone",
                &self.microphone,
                microphone,
            ))
            .child(row("camera", "camera", &self.camera, camera))
            .child(self.notice.clone())
    }
}

view_guest::export_view!(
    MediaProbe,
    "Media probe",
    "opens the microphone and the camera through the device doors",
    ["media", "audio", "video"]
);
