# Camera

Android apps in Deckard use the phone's cameras through Camera2, CameraX and
the old Camera API. The cameras stay the host's: libcamera on the host owns
the sensors, Marathon's own Camera app shares them, and Android never opens a
V4L2, media-controller or sub-device node.

## Shape

```
CameraX / Camera2 app
  └▶ cameraserver                                         (Android, unchanged)
       └▶ ICameraProvider "deckard/0"                      (AIDL, vendor, uid cameraserver)
            android.hardware.camera.provider-V1-deckard-service
            ├ static characteristics, requests, results, JPEG + EXIF  (AOSP external HAL, forked)
            └ frames ◀── /data/deckard/camera.sock ──▶ deckard-camera   (host, user session)
                         SOCK_SEQPACKET, dma-buf fds          └ GStreamer libcamerasrc ! appsink
                                                                 └ libcamera simple + SoftISP
                                                                    └ SENINF → CAMSV → V4L2
```

The host daemon runs the same libcamerasrc path as Marathon Camera, so both
get the same sensor modes, SoftISP and fixes. Each frame is a dma-buf that
SoftISP wrote; its descriptor crosses the socket and the HAL maps it. The
pixels are not copied until the HAL writes Android's own buffers, which are
allocated by their consumers (SurfaceFlinger, the video encoder, ImageReader)
and so always need one write.

## Options considered

| Option | Verdict |
|---|---|
| Host ICameraProvider over `/dev/host_binder` (Waydroid's host-AIDL passthrough, `hostaidls.conf`) | Rejected. It exists for Halium hosts, where Android HALs already run on the host with their own servicemanager. Deckard's hosts have none, so the host side would have to rebuild a camera HAL outside Android: camera metadata encoding, request and result state, flush, JPEG and EXIF, and above all the gralloc handle layout of the image's minigbm, which is private to Android and changes with it. The host binder bridge also routes every `ICameraProvider` lookup and registration to the host, so it stops an in-container provider from being found; `ICameraProvider` is dropped from `hostaidls.conf`. |
| In-container HAL fed by the host over a socket, dma-buf frames | **Chosen.** Android's buffer handling (IMapper, fences, stream configuration) stays Android's; the HAL is the AOSP AIDL external camera HAL, whose request machinery, JPEG encoder and EXIF are CTS-tested at the EXTERNAL level, with V4L2 swapped for the socket. |
| A PipeWire camera node read by the HAL | Not now. PipeWire on the phone has no libcamera SPA plugin, the HAL would need a bionic libpipewire, and Marathon Camera talks to libcamera directly, so PipeWire could not arbitrate between the two anyway. If Marathon's camera moves to PipeWire, `deckard-camera` swaps `libcamerasrc` for `pipewiresrc`; the socket and the HAL stay. |
| AIDL virtual camera (`VirtualCameraService`) | Rejected. Virtual cameras belong to a virtual device; apps on the default device do not see them, and frames would be drawn by an app into a Surface (two copies). |
| AOSP external (UVC) HAL on a v4l2loopback node | Rejected. A kernel module and a V4L2 node inside Android; MJPEG only; every camera reports `LENS_FACING_EXTERNAL`, which CameraX's front and back selectors skip. |

## Sharing

Both cameras sit behind one SENINF/CAMSV media device, and libcamera's simple
pipeline locks the media device when a camera is acquired. So one camera can
stream at a time, on the whole phone, whoever asks.

Policy: **the first holder keeps the camera; the other gets a clean error.**

- Android opens a camera while Marathon Camera has one: `ICameraDevice::open`
  fails with `CAMERA_IN_USE`, and Camera2 apps get `CameraAccessException`
  (`CAMERA_IN_USE`); CameraX reports the camera unavailable.
- Marathon Camera opens while an Android app has the camera: libcamerasrc
  cannot acquire it and the app shows "Camera unavailable".
- Android holds the camera from `ICameraDevice::open` to the session's
  `close`. Apps close their camera when they go to the background, so
  switching to Marathon Camera normally finds it free. The HAL acquires on
  `open`, not at the first frame, so the error comes where apps expect it.
- Inside Android, cameraserver arbitrates as usual: each camera costs 100 and
  conflicts with the other, so a second Android client evicts or is refused
  by Android's own priority rules before the host is asked.

Last-opened-wins was considered and rejected: Marathon Camera holds the
camera inside its own process through libcamera, and nothing outside it can
take the camera away cleanly.

## Host: `deckard-camera`

A user service (`deckard-camera.service`, `host/deckard-camera`). It binds
`<Android data>/deckard/camera.sock` (Android's `/data/deckard/camera.sock`,
mode 0666) and accepts only peers whose UID is Android's `cameraserver`
(1047; the container shares the host's UIDs). Everything a peer sends is
checked before use.

At start it probes cameras once with libcamera's GStreamer device provider:
the libcamera id, `Location` (front, back, external) and `Rotation`, and the
`VideoRecording` sizes in `RGBA` (libcamera `ABGR8888`). For a size range it
takes the range's largest size, or its largest 16:9 and 4:3 sizes when that
is over the cap. Sizes are capped at 1920×1080; larger viewfinder sizes
stall SoftISP today.

The stream runs at the sensor mode's own rate. Asking libcamerasrc for a
rate clamps it to the camera's FrameDurationLimits, which on the Q25 gave
10 fps, so `START` carries no rate. The appsink offers video meta, so frames
whose rows are padded (1276 px wide) stay in libcamera's dma-bufs instead of
being copied by libcamerasrc.

Each connection drives one camera:

| HAL → host | Host → HAL |
|---|---|
| `HELLO version` | `CAMERAS [id, facing, rotation, sizes]` |
| `OPEN camera` | `OK` or `ERROR busy|gone|failed` (pipeline to READY: libcamera acquire) |
| `START width height` | `OK` or `ERROR`, then `FRAME` messages (pipeline to PLAYING) |
| `RELEASE frame` | |
| `STOP` | `OK` (pipeline to READY; the camera stays acquired) |
| close | pipeline to NULL: libcamera release |

`FRAME` carries the frame number, the capture time on CLOCK_MONOTONIC (the
buffer's PTS plus the pipeline's base time), size, stride and offset, plus
the dma-buf in `SCM_RIGHTS`. At most two frames are out at a
time; a sample that arrives while two are out is dropped, so libcamera never
runs out of buffers because the HAL is slow.

The provider keeps one more connection open with only `HELLO`; when it
closes (the daemon stopped) the cameras become `NOT_PRESENT` and the
provider reconnects.

## Android: the Deckard camera provider

`hardware/deckard/camera` in the image tree: a fork of
`hardware/interfaces/camera/{provider,device}/default` (Android 16), built as
`android.hardware.camera.provider-V1-deckard-service`, instance `deckard/0`,
running as `cameraserver`. The fork keeps the AOSP code and changes:

- **Enumeration.** Cameras come from `CAMERAS`, not `/dev/video*` and
  inotify. Device names are `device@1.1/deckard/<n>` (cameraserver wants the
  provider instance's type), back camera first, so the old Camera API's ids
  0 and 1 are back and front.
- **Characteristics.** `LENS_FACING` from `Location`; `SENSOR_ORIENTATION`
  from `Rotation` (libcamera's own Android layer maps them the same way);
  hardware level EXTERNAL; `BACKWARD_COMPATIBLE`; outputs YUV_420_888,
  IMPLEMENTATION_DEFINED (PRIVATE) and BLOB (JPEG) at every host size (the
  HAL crops and scales its working frame to each stream); AE target FPS
  ranges from the host sizes' rates.
- **Frames.** `RGBA` from the socket is converted once to the HAL's
  full-range I420 working frame with libyuv (`ABGRToJ420`), replacing the
  MJPEG decode, and the dma-buf goes back to the host right after; the rest
  (crop, scale, YUV and JPEG outputs, EXIF) is AOSP's. At most two requests
  hold host frames; each stream gets four buffers.
- **Buffers.** cameraserver hands each capture request its output buffers
  and fences; the provider declares no buffer management version. The AOSP
  HAL buffer manager path (`requestStreamBuffers` with a 132 ms wait) failed
  on the Q25 whenever the preview's consumer was slow to return a buffer.
- **Timestamps.** The host's CLOCK_MONOTONIC capture time, reported as
  `SENSOR_INFO_TIMESTAMP_SOURCE_UNKNOWN`, as the external HAL does.

The image drops the HIDL 2.7 external provider and its config, declares the
AIDL provider in the vendor manifest, and declares `android.hardware.camera`
(back) and `android.hardware.camera.front` instead of the full, raw and
autofocus features it never had.

## Host devices

Deckard used to bind every `/dev/video*` into the container and make it
world-writable, for SDM845's codecs. On the Q25 that is the CAMSV capture
node. Only memory-to-memory nodes (codecs: `V4L2_CAP_VIDEO_M2M*`) are bound
and opened now; capture nodes stay the host's.

## Permissions

Android's camera permission goes through Marathon's prompt as for every
runtime permission ([permissions.md](permissions.md)). cameraserver checks it
before a client reaches the HAL. Marathon has no camera-in-use indicator yet;
when it gets one, `deckard-camera` knows when the camera is streaming and for
which open.

## Showing the preview in Marathon

Camera preview buffers reach Marathon through Deckard's composer HAL like
any Android layer. Two composer faults hid them on the Q25, both fixed in
image patch `hardware_deckard/0009` (shipped in the overlay until the image
is rebuilt):

- The preview layer carries the sensor's rotation as a layer transform.
  Marathon's compositor (Qt Wayland 6.11) does not apply
  `wl_surface.set_buffer_transform` (it keeps it as a content orientation)
  and its viewporter rejects a source rectangle in transformed coordinates,
  ending the composer's connection every frame. The composer now draws a
  transformed layer into a copy, turned, and attaches that with no
  transform.
- The composer's copy for YUV frames was allocated tiled by minigbm on
  Panfrost and then refused as not linear; it is allocated linear.

## Findings for the camera bring-up

Measured on the Q25 (libcamera 0.8.0 with the Q25 patches, 2026-11-02).
Nothing here was changed in libcamera or the kernel.

- **A requested frame rate can drop the front camera to 10 fps.** libcamerasrc
  clamps a `framerate` in its caps to the camera's `FrameDurationLimits`
  before configuring (`gst_libcamera_clamp_and_set_frameduration`). Before
  configuration the cameras report, from `cam --list-controls`:
  - front IMX111: `FrameDurationLimits [96449..98004]` µs, about 10.2 fps
    at most;
  - rear S5KJN1: `[33323..509300]` µs, 2–30 fps.

  So `framerate=30/1` on the front camera negotiates `10/1`, and caps that
  insist on 30/1 fail to negotiate (`not-negotiated`, no frames, no bus
  error). The limits look like the sensor's default mode rather than the mode
  the requested size selects. Any libcamerasrc client that sets a frame rate
  is affected, Marathon Camera included if its caps carry one.
  `deckard-camera` leaves the rate out and gets the mode's own rate.
- **Padded rows are copied unless the sink takes video meta.** For widths whose
  RGBA rows libcamera pads (1276 px front, 1440 px rear), libcamerasrc copies
  every frame into system memory when downstream does not accept
  `GstVideoMeta` ("Downstream doesn't support video meta, need to copy
  frame"). `deckard-camera`'s appsink offers it.
- **Rear 1920×1080 through `deckard-camera`:** 29.2 fps, first frame 367 ms
  after `START`, capture-to-client latency median 36 ms, p95 96 ms (frame
  PTS plus base time against CLOCK_MONOTONIC at receipt).

## Verified on the Q25 (2026-11-03, kernel f2)

F-Droid's Open Camera (`net.sourceforge.opencamera`, version code 96) through
Deckard, Marathon Camera alongside:

| Check | Result |
|---|---|
| Cameras | 2: back (facing back, orientation 90), front (front, 270), EXTERNAL level |
| Back preview in Marathon | shown; 22.9 fps in Android (host limit about 24 at 12.5 MP) |
| Front preview in Marathon | shown; 30.2 fps |
| Stills | JPEG from both (1920x1080 back, 1276x720 front) |
| Marathon Camera brought in front of Open Camera | Open Camera closes on pause; Marathon Camera previews at once |
| Open Camera brought in front of Marathon Camera | opens after Marathon Camera lets go (the open waits up to 1.5 s) |
| Host to client latency (rear 1920x1080, `deckard-camera` alone) | median 36 ms, p95 96 ms |

Open: JPEG EXIF orientation (1 on both test photos, taken lying flat;
Open Camera derives it from the accelerometer), a rotated video through the
composer patch (the image has no video player), video recording.
