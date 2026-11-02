# Permission prompts

When an Android app asks for a runtime permission, the person sees
Marathon's own prompt, the one Marathon apps get, not Android's
`GrantPermissionsActivity` dialog. BlackBerry 10 did the same for its
Android apps. Android stays the authority on what is asked and on what an
answer means; Marathon only draws the question and collects the answer.

The wire protocol is in [../protocol/bridge.md](../protocol/bridge.md)
(*Permission prompts*). The shell side is `Permissions1.PromptAndroid` in
Marathon-Shell.

## Shape

```
app: requestPermissions([CAMERA])
  └▶ PermissionController GrantPermissionsActivity      (Android decides what to ask)
       mViewHandler = MarathonGrantView                  (patched in; draws nothing)
         └ call("ask") ▶ DeckardSystem PermissionPrompts  (system uid, provider)
              └ perm_ask ▶ deckard-bridge PermissionPrompts     (system.sock)
                   └ Permissions1.PromptAndroid ▶ shell PermissionDialog
                   ◀ {answer, precise}
              ◀ perm_answer
         ◀ ResultReceiver
       listener.onPermissionGrantResult(group, perms, ALLOWED_FOREGROUND | ONE_TIME | DENIED | ...)
  ◀ onRequestPermissionsResult (Android's own result)
```

## Choosing the hook

Android resolves a permission request with
`PackageManager.buildRequestPermissionsIntent`, which pins the intent to the
permission controller package. Checked against Android 13's code
(PermissionController TQ3A.230901.001 and `services.jar` taken from the
device and decompiled). Android 16's source keeps the chosen hook; the
rejected options were not rechecked there:

| Option | Verdict |
|---|---|
| Our own activity for `REQUEST_PERMISSIONS` | Impossible: the intent is `setPackage`d to PermissionController. Becoming the permission controller package means owning `MANAGE_PERMISSIONS`; PMS requires exactly one, and the same package also runs the role and permission services. |
| `ActivityInterceptorCallback` | system_server only (`ActivityTaskManagerInternal`); a system-uid app can't register one. |
| `IActivityManager.setActivityController` | Sees starts but strips extras (`cloneFilter`). Vetoing a start gives the app `RESULT_CANCELED` at once. It is also the Monkey hook and makes `isUserAMonkey()` true. |
| Intent firewall | A block is the same abort and `RESULT_CANCELED`. |
| Invisible dialog (an RRO) and DeckardSystem granting with `PackageManager` while it is up | No way to learn what is asked: the request's extras live only in PermissionController's process. GrantPermissionsActivity results come from `checkPermission` at finish, but its view model keeps a group pending after an outside denial, so the activity would also have to be driven by key injection. Every Android rule (one-time sessions, fine and coarse, split permissions, user-set and user-fixed, storage supergroups, auto-revoke exemptions) would have to be redone by hand. |
| **Swap GrantPermissionsActivity's view handler** | Chosen. |

GrantPermissionsActivity already separates deciding from drawing. Its view
model works out each question: the group, which buttons Android would offer
(allow, allow while in use, only this time, deny, deny and don't ask again,
keep while in use, a settings link) and the location accuracy choice. It
gives those to a `GrantPermissionsViewHandler` (`updateUi`). The handler
reports the button pressed (`onPermissionGrantResult(group, perms, result)`),
and the view model applies it exactly as for a tap. There are handlers for
phones, TV, Wear and cars. Deckard's PermissionController creates
`MarathonGrantView` instead of the phone handler. So every Android rule still
holds; only the drawing moved.

Deckard's image builds PermissionController from source (LineageOS 23.2,
Android 16; its APEXes are not flattened, so an APK swap cannot work). The
patch is one constructor call in `GrantPermissionsActivity.onCreate` plus
`MarathonGrantView`, written against Android 16's
`GrantPermissionsViewHandler` (the rationale message, `setResultListener`,
16 buttons). It lives in Marathon-Deckard's `image/lineage-23.2` branch as
`image/build/patches/packages_modules_Permission`, applied by
`image/build/scripts/apply-patches.sh` and shipped in the image's
`com.android.permission` APEX.

## Mapping a question to Marathon's prompt

`MarathonGrantView.updateUi` turns Android's question into a `perm_ask`:

| Android | `perm_ask` | Marathon's prompt |
|---|---|---|
| The group's platform permissions the app asks for (`Utils.getPlatformPermissionNamesOfGroup` ∩ the manifest) | `perms` | deckard-bridge names them with `contract::marathon_permission` (the table Settings uses); the shell takes the label and description from Marathon's vocabulary. Android permissions with no Marathon name are left out: Android 16's groups also hold `READ_MEDIA_VISUAL_USER_SELECTED` (implicit for every app asking for photos), `ACCESS_MEDIA_LOCATION`, `RANGING`, `ACCESS_LOCAL_NETWORK`, `READ_CELL_BROADCASTS` and health permissions. A request with no named permission, two names, or a non-platform permission goes back to Android's dialog. |
| `ALLOW_ONE_TIME` offered (camera, microphone, location) | `once` | An "Only this time" switch |
| Location accuracy radio group (fine requested with coarse) | `accuracy: choice`, `preciseDefault` | A "Precise location" switch, on by Android's default |
| Coarse only | `accuracy: approximate` | "…your approximate location" |
| Fine only, coarse granted (upgrade) | `kind: precise` | "…wants your precise location" |
| "All the time" (see below) | `kind: background` | "…wants to use your location all the time, even when you aren't using it" |
| `ALLOW_SELECTED` offered: photos and videos with Android 14's picker (apps targeting API 34+, or declaring `READ_MEDIA_VISUAL_USER_SELECTED`) | `limited` | An "Only photos I pick" switch, off by default. Older apps get the plain question and no switch. |

Marathon's design system allows two actions per dialog, so Android's extra
buttons become switches: Deny and Allow stay the prompt's only actions.

The answer maps back to the button Android offered:

| Answer | Result |
|---|---|
| Allow | `GRANTED_ALWAYS` if Android offered Allow, else `GRANTED_FOREGROUND_ONLY` ("while using the app"). The location permissions passed are the ones the precise switch chose, as the dialog's radio buttons would. |
| Allow, only photos I pick | `GRANTED_USER_SELECTED`: GrantPermissionsActivity opens Android's photo picker, and the app gets the photos and videos chosen there (`READ_MEDIA_VISUAL_USER_SELECTED`) |
| Allow, only this time | `GRANTED_ONE_TIME`: Android's one-time session; the grant goes when the app has been in the background for a while |
| Deny | `DENIED_DO_NOT_ASK_AGAIN` when Android offered "don't allow" in its second-time form (the group was denied before, `USER_SET`), else `DENIED`. Android 11+ semantics: the second denial is final (`USER_FIXED`) and the app's `shouldShowRequestPermissionRationale` follows. After a limited grant, Android asks whether to pick more; Deny is then `DENIED_MORE` and keeps the photos already chosen. |
| Dismissed (back, Home, another app in front, the shell gone) | `CANCELED`, as Android's back: no flags change and the app may ask again |
| deckard-bridge can't ask (no shell method, refused, unknown permission, too many waiting) | Android's own dialog is shown. DeckardPermissionControllerRes restyles Android 13's Material 3 layout; Android 16 draws its expressive layout, which the overlay doesn't reach yet. |

Nothing is granted without the person: every result comes from their
answer, and every failure falls back to Android asking them.

### "All the time"

Android 11+ asks for background location (and `BODY_SENSORS_BACKGROUND`)
only through its per-app permission page: GrantPermissionsActivity sends the
app to `MANAGE_APP_PERMISSION` (`sendToSettingsImmediately`, or the dialog's
settings link for older apps). DeckardSystem's `BackgroundAccess` answers
that action above PermissionController's page (priority 2 over 1; it
requires `GRANT_RUNTIME_PERMISSIONS`, so only PermissionController and the
system can start it).

- **From GrantPermissionsActivity** (`CALLER_NAME`, and the caller is
  PermissionController): it finds the background permissions of the group's
  granted foreground permissions (`PermissionInfo.backgroundPermission`)
  that the app asks for, and shows Marathon's background prompt.
  - Allow grants them, `USER_SET`.
  - Deny leaves them revoked, `USER_SET`, and `USER_FIXED` if they were
    already user-set, as Android's page does.
  - It returns `RESULT_PERMISSION_INTERACTED` so GrantPermissionsActivity
    moves on.
  - Dismissed, it returns nothing, and GrantPermissionsActivity applies
    Android's own rule for leaving the page.
  - Without deckard-bridge it opens PermissionController's page and passes the
    result back.
- **From anywhere else**, it opens the app's page in Marathon Settings.

For apps that target Android 10 or lower, "all the time" is only behind
the dialog's settings link. MarathonGrantView follows the link once (the
same Marathon prompt as above). If Android asks again, it answers "keep
while using".

## The shell

`org.marathonos.Shell.Permissions1` gains two methods. Only a declared
session service holding `android-permissions` may call them; deckard-bridge's
declaration (`deckard-bridge.service.json`) now lists it.

- `PromptAndroid(s appId, s permission, a{sv} options) → a{sv}`: the reply
  is delayed until the person answers: `{answer: allow|once|deny|cancel,
  precise: b, limited: b}`.
  - `appId` must be `deckard.<package>`.
  - `permission` must be a Marathon vocabulary name that isn't restricted.
  - `options` hold `request` (16 hex digits), `kind`, `once`, `accuracy`,
    `preciseDefault` and `limited`.
  - At most 16 prompts wait.
  - A caller that leaves the bus has its prompts cancelled.
- `CancelAndroidPrompt(s request)`.

`MarathonPermissionManager::askForeign` queues these beside Marathon apps'
prompts. They are shown one at a time with the same `PermissionDialog`: the
app's icon and name from its `waydroid.<pkg>` desktop entry, and the
permission's label and description from the vocabulary. Nothing is written
to Marathon's `app-permissions.json`: Android keeps the grant, and Settings
reads it from Android. Dismissing a prompt (`dismissForApp`, `dismissAll`,
the foreground app changing) answers `cancel`.

Marathon has no global camera, microphone or location switches today: the
Quick Settings location tile starts and stops the shell's GeoClue client and
isn't a privacy switch. So there is nothing for the prompt to defer to yet.

## Verified

- **Shell** (Marathon-Shell, built in a private build directory):
  - `test_core_permissionmanager`, 32 tests, 5 new: foreign prompts show at
    once and store nothing; "only this time" only when offered; precise is
    the switch only when there is a choice; dismissal and cancellation;
    foreign and Marathon prompts take turns.
  - `test_core_permissionauth`, 2 new: app ids and option cleaning.
  - `test_core_ipcgates` with `PermissionsObject` added: every Permissions1
    method checks its caller.
  - `tst_permissiondialog.qml`, 4 new: the switches appear only for Android
    prompts, and the answers carry them; the wording per kind.
  - qmllint clean.
  - Rendered offscreen at 540 × 1140 and looked at: camera with "Only this
    time"; location with both switches; background location.
- **deckard-bridge**: `test_permissionprompts`, a fake shell on a private bus.
  Requests become prompts with the right app id, name and options. Answers
  go back. Unsupported requests go back to Android without reaching the
  shell. With no shell, Android asks. Android's cancels and a lost channel
  reach the shell. The waiting list is bounded and a repeated id is
  ignored. All 17 deckard-bridge tests pass.
- **deckard-bridge**, again for Android 16: a photos request carrying
  `READ_MEDIA_VISUAL_USER_SELECTED` and `ACCESS_MEDIA_LOCATION` becomes the
  storage prompt; a request with only unnamed members, or with a
  non-platform permission, goes back to Android (`tests/permissions.rs`).
- **On the Zinwa Q25**, LineageOS 23.2 image `system_datetime 1793176546`
  (PermissionController BP4A.251205.006, from the source patch), with a test
  app targeting API 33 that requests on demand. The results come from
  `dumpsys package` and `appops get`:
  - Camera, "Only this time": `granted`, `USER_SET|ONE_TIME`.
  - Camera, Deny: `USER_SET`, and the rationale is shown. A second Deny
    gives `USER_FIXED` with no rationale. A third request returns denied at
    once, with no prompt.
  - Location with "Precise location" off, then Allow: only
    `ACCESS_COARSE_LOCATION`, appop `COARSE_LOCATION: foreground`. Asking
    for fine again gives "wants your precise location"; Allow gives
    `FINE_LOCATION: foreground`.
  - Background location: Marathon's "all the time" prompt (BackgroundAccess).
    Deny leaves it revoked and `USER_SET`.
  - Microphone, Allow: `RECORD_AUDIO: foreground` (while in use).
  - Notifications, Allow: `POST_NOTIFICATIONS` granted, `USER_SET`.
  - Contacts dismissed with Back: the app gets denied and no flags change.
  - Photos and videos: before the deckard-bridge fix above, Android's dialog
    ("left to Android: … 4 permissions"). After it, Marathon's "Your files"
    prompt; Allow grants `READ_MEDIA_IMAGES` and `READ_MEDIA_VIDEO`
    (`USER_SET`), and Android grants the implicit
    `READ_MEDIA_VISUAL_USER_SELECTED` with them.
  - The first screenshots after each request show the app, then Marathon's
    prompt; Android's dialog never appears.
  - Photos with Android 14's picker, test app targeting API 34: Marathon's
    prompt with "Only photos I pick", off by default. Allow gives
    `READ_MEDIA_IMAGES` and `READ_MEDIA_VIDEO` (`USER_SET`); Deny leaves
    them denied (`USER_SET`, rationale shown). With the switch on, Allow
    opens Android's photo picker; after one of three photos was picked,
    `READ_MEDIA_VISUAL_USER_SELECTED` was granted (`USER_SET`) and the app's
    MediaStore query returned only that photo. The picker needs Android's
    emulated storage (`/dev/fuse` in the container).
- **Still Android's dialog**: anything Marathon can't name.
