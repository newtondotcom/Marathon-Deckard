//! One HAL connection: at most one camera, opened through libcamerasrc.

use std::collections::HashMap;
use std::io;
use std::mem::MaybeUninit;
use std::os::fd::{AsFd, BorrowedFd, OwnedFd};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::{self, JoinHandle};

use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_allocators::FdMemory;
use gstreamer_app::{AppSink, AppSinkCallbacks};
use gstreamer_video::{VideoInfo, VideoMeta};
use rustix::io::Errno;
use rustix::net::{
    RecvFlags, SendAncillaryBuffer, SendAncillaryMessage, SendFlags, recv, send, sendmsg,
};

use crate::probe::{Camera, FORMAT};
use crate::wire::{self, Message, Request};

/// Frames the HAL may hold at once; libcamera keeps the rest.
const FRAMES_OUT: usize = 2;
const MAX_FPS: u32 = 30;

#[derive(Default)]
struct Frames {
    next: u64,
    out: HashMap<u64, gst::Sample>,
    streaming: bool,
}

struct Stream {
    pipeline: gst::Pipeline,
    filter: gst::Element,
    frames: Arc<Mutex<Frames>>,
    bus: Option<JoinHandle<()>>,
}

fn reply(socket: &OwnedFd, message: &Message) -> io::Result<()> {
    send(socket, message.bytes(), SendFlags::NOSIGNAL)?;
    Ok(())
}

fn error(socket: &OwnedFd, code: u32) -> io::Result<()> {
    reply(socket, &Message::new(wire::ERROR).u32(code))
}

/// Serves `socket` until the HAL closes it.
///
/// # Errors
/// The socket failed or the HAL sent something that is not a request.
pub fn serve(socket: OwnedFd, cameras: &[Camera]) -> io::Result<()> {
    let socket = Arc::new(socket);
    let mut stream: Option<(usize, Stream)> = None;
    let result = (|| loop {
        let mut bytes = [0_u8; 32];
        let (_, len) = recv(&*socket, &mut bytes[..], RecvFlags::TRUNC)?;
        if len == 0 {
            return Ok(());
        }
        let request = bytes
            .get(..len)
            .and_then(Request::parse)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "not a camera request"))?;
        match request {
            Request::Hello { version } if version == wire::VERSION => {
                hello(&socket, cameras)?;
            }
            Request::Hello { .. } => error(&socket, wire::INVALID)?,
            Request::Open { camera } => {
                let index = usize::try_from(camera).ok().filter(|&i| i < cameras.len());
                match (index, &stream) {
                    (Some(index), None) => match open(&socket, &cameras[index]) {
                        Ok(opened) => {
                            stream = Some((index, opened));
                            reply(&socket, &Message::new(wire::OK))?;
                        }
                        Err(code) => error(&socket, code)?,
                    },
                    _ => error(&socket, wire::INVALID)?,
                }
            }
            Request::Start { width, height } => {
                let valid = stream
                    .as_ref()
                    .is_some_and(|(index, _)| cameras[*index].sizes.contains(&(width, height)));
                match &stream {
                    Some((_, opened)) if valid => match opened.start(width, height) {
                        Ok(()) => reply(&socket, &Message::new(wire::OK))?,
                        Err(()) => error(&socket, wire::BROKEN)?,
                    },
                    _ => error(&socket, wire::INVALID)?,
                }
            }
            Request::Stop => match &stream {
                Some((_, opened)) => {
                    opened.stop();
                    reply(&socket, &Message::new(wire::OK))?;
                }
                None => error(&socket, wire::INVALID)?,
            },
            Request::Release { frame } => {
                if let Some((_, opened)) = &stream {
                    lock(&opened.frames).out.remove(&frame);
                }
            }
        }
    })();
    if let Some((_, opened)) = stream {
        opened.close();
    }
    result
}

fn lock(frames: &Mutex<Frames>) -> std::sync::MutexGuard<'_, Frames> {
    frames.lock().unwrap_or_else(PoisonError::into_inner)
}

fn hello(socket: &OwnedFd, cameras: &[Camera]) -> io::Result<()> {
    for (index, camera) in (0_u32..).zip(cameras) {
        let mut message = Message::new(wire::CAMERA)
            .u32(index)
            .u32(camera.facing)
            .u32(camera.rotation)
            .u32(u32::try_from(camera.sizes.len()).unwrap_or(0));
        for &(width, height) in &camera.sizes {
            message = message.u32(width).u32(height).u32(MAX_FPS);
        }
        reply(socket, &message)?;
    }
    let count = u32::try_from(cameras.len()).unwrap_or(0);
    reply(socket, &Message::new(wire::CAMERAS_END).u32(count))
}

/// The `ERROR` code for why the pipeline did not reach READY.
fn open_error(pipeline: &gst::Pipeline) -> u32 {
    let Some(bus) = pipeline.bus() else {
        return wire::BROKEN;
    };
    while let Some(message) = bus.pop_filtered(&[gst::MessageType::Error]) {
        if let gst::MessageView::Error(failure) = message.view() {
            eprintln!("deckard-camera: open: {}", failure.error());
            if failure.error().matches(gst::ResourceError::Busy) {
                return wire::BUSY;
            }
            if failure.error().matches(gst::ResourceError::NotFound) {
                return wire::GONE;
            }
        }
    }
    wire::BROKEN
}

/// Acquires `camera` (libcamerasrc at READY).
fn open(socket: &Arc<OwnedFd>, camera: &Camera) -> Result<Stream, u32> {
    let build = || -> Result<(gst::Pipeline, gst::Element, AppSink), gst::glib::BoolError> {
        let pipeline = gst::Pipeline::new();
        let source = gst::ElementFactory::make("libcamerasrc")
            .property("camera-name", &camera.id)
            .build()?;
        let filter = gst::ElementFactory::make("capsfilter").build()?;
        let sink = AppSink::builder()
            .sync(false)
            .max_buffers(u32::try_from(FRAMES_OUT).unwrap_or(2))
            .drop(true)
            .enable_last_sample(false)
            .build();
        pipeline.add_many([&source, &filter, sink.upcast_ref()])?;
        gst::Element::link_many([&source, &filter, sink.upcast_ref()])?;
        Ok((pipeline, filter, sink))
    };
    let (pipeline, filter, sink) = build().map_err(|failure| {
        eprintln!("deckard-camera: pipeline: {failure}");
        wire::BROKEN
    })?;
    let frames = Arc::new(Mutex::new(Frames::default()));
    let to_hal = Arc::clone(socket);
    let shared = Arc::clone(&frames);
    sink.set_callbacks(
        AppSinkCallbacks::builder()
            // Strided frames (1276 px rows) stay in libcamera's dma-bufs only when
            // the sink takes video meta; otherwise libcamerasrc copies them.
            .propose_allocation(|_, query| {
                query.add_allocation_meta::<VideoMeta>(None);
                true
            })
            .new_sample(move |sink| {
                let sample = sink.pull_sample().map_err(|_| gst::FlowError::Flushing)?;
                let base = sink.base_time().unwrap_or(gst::ClockTime::ZERO);
                deliver(&to_hal, &shared, sample, base);
                Ok(gst::FlowSuccess::Ok)
            })
            .build(),
    );
    if pipeline.set_state(gst::State::Ready).is_err() {
        let code = open_error(&pipeline);
        let _ = pipeline.set_state(gst::State::Null);
        return Err(code);
    }
    let bus = pipeline.bus().map(|bus| {
        let to_hal = Arc::clone(socket);
        thread::spawn(move || watch(&bus, &to_hal))
    });
    Ok(Stream {
        pipeline,
        filter,
        frames,
        bus,
    })
}

/// Tells the HAL when the running pipeline fails, until `close` posts the
/// application message.
fn watch(bus: &gst::Bus, socket: &OwnedFd) {
    let kinds = [gst::MessageType::Error, gst::MessageType::Application];
    while let Some(message) = bus.timed_pop_filtered(gst::ClockTime::NONE, &kinds) {
        match message.view() {
            gst::MessageView::Error(failure) => {
                eprintln!("deckard-camera: stream: {}", failure.error());
                let _ = reply(socket, &Message::new(wire::FAILED));
            }
            _ => return,
        }
    }
}

/// Sends `sample` to the HAL unless it already holds `FRAMES_OUT` frames.
fn deliver(socket: &OwnedFd, frames: &Mutex<Frames>, sample: gst::Sample, base: gst::ClockTime) {
    let mut frames = lock(frames);
    if !frames.streaming || frames.out.len() >= FRAMES_OUT {
        return;
    }
    let (Some(buffer), Some(caps)) = (sample.buffer(), sample.caps()) else {
        return;
    };
    let Ok(info) = VideoInfo::from_caps(caps) else {
        return;
    };
    if buffer.n_memory() != 1 {
        return;
    }
    let memory = buffer.peek_memory(0);
    let Some(fd) = memory.downcast_memory_ref::<FdMemory>().map(|fd| fd.fd()) else {
        eprintln!("deckard-camera: libcamerasrc gave a frame without a dma-buf");
        return;
    };
    let stride = buffer
        .meta::<VideoMeta>()
        .and_then(|meta| meta.stride().first().copied())
        .unwrap_or_else(|| info.stride()[0]);
    let time = buffer.pts().map_or(0, |pts| (pts + base).nseconds());
    let frame = frames.next;
    let message = Message::new(wire::FRAME)
        .u64(frame)
        .u64(time)
        .u32(info.width())
        .u32(info.height())
        .u32(u32::try_from(stride).unwrap_or(0))
        .u32(u32::try_from(memory.offset()).unwrap_or(u32::MAX))
        .u32(u32::try_from(memory.size()).unwrap_or(0));
    #[allow(unsafe_code)]
    // SAFETY: `fd` belongs to `memory`, which `sample` keeps alive until after
    // `sendmsg` returns.
    let fd = unsafe { BorrowedFd::borrow_raw(fd) };
    let mut space = [MaybeUninit::uninit(); rustix::cmsg_space!(ScmRights(1))];
    let mut control = SendAncillaryBuffer::new(&mut space);
    let fds = [fd.as_fd()];
    if !control.push(SendAncillaryMessage::ScmRights(&fds)) {
        return;
    }
    let flags = SendFlags::DONTWAIT | SendFlags::NOSIGNAL;
    match sendmsg(
        socket,
        &[io::IoSlice::new(message.bytes())],
        &mut control,
        flags,
    ) {
        Ok(_) => {
            frames.next += 1;
            frames.out.insert(frame, sample);
        }
        Err(Errno::AGAIN) => {}
        Err(failure) => eprintln!("deckard-camera: frame: {failure}"),
    }
}

impl Stream {
    /// Streams at the sensor mode's own rate: libcamerasrc clamps a requested
    /// rate to the camera's FrameDurationLimits, which on the Q25 gave 10 fps.
    fn start(&self, width: u32, height: u32) -> Result<(), ()> {
        let dimension = |value: u32| i32::try_from(value).map_err(|_| ());
        let caps = gst::Caps::builder("video/x-raw")
            .field("format", FORMAT)
            .field("width", dimension(width)?)
            .field("height", dimension(height)?)
            .build();
        self.stop();
        self.filter.set_property("caps", &caps);
        lock(&self.frames).streaming = true;
        if self.pipeline.set_state(gst::State::Playing).is_err() {
            self.stop();
            return Err(());
        }
        eprintln!("deckard-camera: streaming {width}x{height}");
        Ok(())
    }

    /// Back to READY: libcamera stops, the camera stays acquired.
    fn stop(&self) {
        {
            let mut frames = lock(&self.frames);
            frames.streaming = false;
            frames.out.clear();
        }
        let _ = self.pipeline.set_state(gst::State::Ready);
        lock(&self.frames).out.clear();
    }

    /// Releases the camera.
    fn close(mut self) {
        self.stop();
        if let Some(bus) = self.pipeline.bus() {
            let _ = bus.post(gst::message::Application::new(gst::Structure::new_empty(
                "deckard-camera-close",
            )));
        }
        if let Some(watcher) = self.bus.take() {
            let _ = watcher.join();
        }
        let _ = self.pipeline.set_state(gst::State::Null);
        eprintln!("deckard-camera: closed");
    }
}
