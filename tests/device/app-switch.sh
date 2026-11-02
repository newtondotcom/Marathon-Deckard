#!/bin/sh
# App switching between two Android apps, on the phone, as root:
#   ssh root@PHONE 'sh -s -- [matrix|first-launch [N]|relaunch [N]|all]' < tests/device/app-switch.sh
# matrix: launch, home, switcher and closed-card switches in both
# directions, and a switch to an app killed in the background. Each step
# checks Marathon's front app (Navigation1.GetForegroundApp) and Android's
# focused task agree on the expected app, stay so for SETTLE seconds, and
# that DeckardSystem never followed another app to the front.
# first-launch: N fresh sessions, launching APP_A in each; reports the time
# from Android's START to the composer's window and any transition timeout.
# relaunch: N times, APP_A force-stopped while its permission prompt is up,
# then home and APP_A launched again within a second; it must start (a task
# left holding only the prompt used to swallow the launch).
# Restarts the Deckard session (never the container) and drives the screen
# with a uinput touchscreen, so nothing else may use the phone meanwhile.
#
# Launches go through the shell's Navigation1.LaunchApp, as the app grid's
# do, so the shell's re-show and Deckard's launch both run. The gesture
# coordinates default to the Zinwa Q25 (720x720) and are set with:
#   PILL="x y"        bottom pill (swipe up from it opens the switcher)
#   HOME_DOT="x y"    the switcher's app grid page indicator
#   CARD_FIRST="x y"  first switcher card (the app just left)
#   CARD_SECOND="x y" second card (the app before it)
#   CARD_CLOSE="x y"  the first card's close button
set -u

APP_A=${APP_A:-org.fdroid.fdroid}
APP_B=${APP_B:-com.aurora.store}
HOME_PKG=${HOME_PKG:-org.marathonos.deckard.home}
SESSION_USER=${SESSION_USER:-user}
SETTLE=${SETTLE:-3}
WAIT=${WAIT:-15}
BOOT_SETTLE=${BOOT_SETTLE:-25}
RESET_NOTIFICATIONS=${RESET_NOTIFICATIONS:-1}
PILL=${PILL:-360 712}
HOME_DOT=${HOME_DOT:-395 636}
CARD_FIRST=${CARD_FIRST:-180 180}
CARD_SECOND=${CARD_SECOND:-540 180}
CARD_CLOSE=${CARD_CLOSE:-344 304}

uid=$(id -u "$SESSION_USER")
work=$(mktemp -d /run/deckard-app-switch.XXXXXX)
trap 'rm -rf "$work"' EXIT INT TERM
failures=0

android() {
	cmd=$1
	shift
	lxc-attach -P /var/lib/deckard/lxc -n deckard --clear-env -- "/system/bin/$cmd" "$@"
}
as_user() { runuser -u "$SESSION_USER" -- env XDG_RUNTIME_DIR="/run/user/$uid" \
	DBUS_SESSION_BUS_ADDRESS="unix:path=/run/user/$uid/bus" "$@"; }
log() { printf '%s %s\n' "$(date +%T)" "$*"; }

# Marathon's front app id, "" for none.
front() {
	as_user busctl --user call org.marathonos.Shell /org/marathonos/Shell/Navigation \
		org.marathonos.Shell.Navigation1 GetForegroundApp 2>/dev/null | sed 's/^s "\(.*\)"$/\1/'
}

# The package whose task holds Android's top resumed activity.
android_front() {
	tid=$(android dumpsys activity activities 2>/dev/null |
		sed -n 's/.*topResumedActivity=ActivityRecord{.* t\([0-9]*\)}.*/\1/p' | head -n 1)
	[ -n "$tid" ] || return 0
	android am stack list 2>/dev/null |
		sed -n "s/^ *taskId=$tid: \([^/]*\)\/.*/\1/p" | head -n 1
}

launch() {
	as_user busctl --user call org.marathonos.Shell /org/marathonos/Shell/Navigation \
		org.marathonos.Shell.Navigation1 LaunchApp s "deckard.$1" >/dev/null
}

cat > "$work/touch.py" <<'EOF'
# touch.py W H GESTURE...: tap X Y | swipe X0 Y0 X1 Y1 MS, ";"-separated.
import fcntl, os, struct, sys, time
EV_SYN, EV_KEY, EV_ABS, BTN_TOUCH = 0, 1, 3, 0x14a
ABS_X, ABS_Y, SLOT, MAJOR, PX, PY, TID, PRESS = 0x00, 0x01, 0x2f, 0x30, 0x35, 0x36, 0x39, 0x3a
w, h = int(sys.argv[1]), int(sys.argv[2])
fd = os.open("/dev/uinput", os.O_WRONLY)
for ev in (EV_SYN, EV_KEY, EV_ABS):
    fcntl.ioctl(fd, 0x40045564, ev)
fcntl.ioctl(fd, 0x40045565, BTN_TOUCH)
fcntl.ioctl(fd, 0x4004556e, 1)  # INPUT_PROP_DIRECT
absmax = [0] * 64
for code, top in ((ABS_X, w - 1), (ABS_Y, h - 1), (PX, w - 1), (PY, h - 1), (SLOT, 9), (MAJOR, 15), (TID, 65535), (PRESS, 255)):
    fcntl.ioctl(fd, 0x40045567, code)
    absmax[code] = top
os.write(fd, b"deckard-app-switch".ljust(80, b"\0") + struct.pack("HHHHI", 6, 0x1234, 0x5680, 1, 0)
         + struct.pack("64i", *absmax) + bytes(64 * 4 * 3))
fcntl.ioctl(fd, 0x5501)
time.sleep(1.5)  # the compositor opens the new device

def emit(t, c, v):
    n = time.time()
    os.write(fd, struct.pack("qqHHi", int(n), int(n % 1 * 1e6), t, c, v))

def stroke(tid, x0, y0, x1, y1, ms):
    steps = max(2, ms * 120 // 1000)
    for i in range(steps + 1):
        x, y = x0 + (x1 - x0) * i // steps, y0 + (y1 - y0) * i // steps
        if i == 0:
            emit(EV_ABS, SLOT, 0); emit(EV_ABS, TID, tid); emit(EV_KEY, BTN_TOUCH, 1)
            emit(EV_ABS, MAJOR, 6); emit(EV_ABS, PRESS, 60)
        for c, v in ((PX, x), (PY, y), (ABS_X, x), (ABS_Y, y)):
            emit(EV_ABS, c, v)
        emit(EV_SYN, 0, 0)
        time.sleep(ms / 1000 / steps)
    emit(EV_ABS, TID, -1); emit(EV_KEY, BTN_TOUCH, 0); emit(EV_SYN, 0, 0)

for n, part in enumerate(" ".join(sys.argv[3:]).split(";")):
    a = part.split()
    v = [int(x) for x in a[1:]]
    if a[0] == "tap":
        stroke(n + 1, v[0], v[1], v[0], v[1], 60)
    else:
        stroke(n + 1, *v)
    time.sleep(0.8)
fcntl.ioctl(fd, 0x5502)
EOF

size=$(head -n 1 /sys/class/drm/card*-*/modes 2>/dev/null | head -n 1)
width=${size%x*} height=${size#*x}
touch_() { python3 -I "$work/touch.py" "$width" "$height" "$@"; }
pill_up() { x=${PILL% *} y=${PILL#* }; echo "swipe $x $y $x $((y - 290)) 250"; }
switcher() { touch_ "$(pill_up)"; }
go_home() { touch_ "$(pill_up); tap $HOME_DOT"; }

# expect STEP PKG: Marathon and Android both show PKG ("" for home) within
# WAIT s and keep showing it for SETTLE s, and nothing was followed.
expect() {
	step=$1 pkg=$2
	want_m=${pkg:+deckard.$pkg} want_a=${pkg:-$HOME_PKG}
	i=0 m='' a=''
	while [ $i -lt $((WAIT * 2)) ]; do
		m=$(front) a=$(android_front)
		[ "$m" = "$want_m" ] && [ "$a" = "$want_a" ] && break
		sleep 0.5
		i=$((i + 1))
	done
	ok=1
	if [ "$m" != "$want_m" ] || [ "$a" != "$want_a" ]; then
		ok=0
	else
		sleep "$SETTLE"
		m=$(front) a=$(android_front)
		{ [ "$m" = "$want_m" ] && [ "$a" = "$want_a" ]; } || ok=0
	fi
	followed=$(android logcat -d -s DeckardWindowFit:I 2>/dev/null | grep -o 'following [^ ]*' | sort -u | tr '\n' ' ')
	timeouts=$(android logcat -d 2>/dev/null | grep -c 'readiness timeout')
	[ -z "$followed" ] && [ "$timeouts" = 0 ] || ok=0
	if [ $ok = 1 ]; then
		log "PASS $step"
	else
		failures=$((failures + 1))
		log "FAIL $step: want '$want_m'/'$want_a', Marathon '$m', Android '$a'${followed:+, $followed}, $timeouts transition timeouts"
	fi
	android logcat -c
}

kill_app() {
	pid=$(android pidof "$1")
	[ -n "$pid" ] && android kill "$pid"
}

matrix() {
	android logcat -c
	go_home;                    expect "home" ""
	launch "$APP_A";            expect "grid: $APP_A" "$APP_A"
	go_home;                    expect "home from $APP_A" ""
	launch "$APP_B";            expect "grid: $APP_A -> $APP_B" "$APP_B"
	go_home;                    expect "home from $APP_B" ""
	launch "$APP_A";            expect "grid: $APP_B -> $APP_A" "$APP_A"
	switcher; touch_ "tap $CARD_SECOND"; expect "switcher: $APP_A -> $APP_B" "$APP_B"
	switcher; touch_ "tap $CARD_SECOND"; expect "switcher: $APP_B -> $APP_A" "$APP_A"
	go_home; kill_app "$APP_B"; expect "home, $APP_B killed" ""
	launch "$APP_B";            expect "grid: $APP_B restarts after a kill" "$APP_B"
	go_home; kill_app "$APP_A"; expect "home, $APP_A killed" ""
	launch "$APP_A";            expect "grid: $APP_A restarts after a kill" "$APP_A"
	switcher; touch_ "tap $CARD_CLOSE; tap $CARD_FIRST"
	expect "switcher: close $APP_A's card, open $APP_B" "$APP_B"
	go_home;                    expect "home after closing $APP_A" ""
	launch "$APP_A";            expect "grid: $APP_A after its card closed" "$APP_A"
	go_home;                    expect "home at the end" ""
}

restart_session() {
	as_user systemctl --user stop deckard-session
	as_user systemctl --user start deckard-session
	i=0
	until [ "$(android getprop sys.boot_completed 2>/dev/null)" = 1 ] || [ $i -ge 60 ]; do
		sleep 2
		i=$((i + 1))
	done
}

# Epoch seconds of the first logcat line matching PATTERN.
logcat_time() { android logcat -d -v epoch 2>/dev/null | grep -m 1 -- "$1" | awk '{print $1}'; }

first_launch() {
	runs=${1:-5}
	n=1
	while [ $n -le "$runs" ]; do
		restart_session
		sleep "$BOOT_SETTLE"
		if [ "$RESET_NOTIFICATIONS" = 1 ]; then
			android pm revoke "$APP_A" android.permission.POST_NOTIFICATIONS 2>/dev/null
			android pm clear-permission-flags "$APP_A" android.permission.POST_NOTIFICATIONS user-set user-fixed 2>/dev/null
		fi
		go_home
		android logcat -c
		launch "$APP_A"
		i=0 shown=''
		while [ -z "$shown" ] && [ $i -lt $((WAIT * 2)) ]; do
			sleep 0.5
			shown=$(logcat_time "creating toplevel task=[0-9]* app=$APP_A")
			i=$((i + 1))
		done
		start=$(logcat_time "START u0 .*cmp=$APP_A/")
		timeouts=$(android logcat -d 2>/dev/null | grep -c 'readiness timeout')
		if [ -n "$shown" ] && [ -n "$start" ]; then
			ms=$(awk -v a="$start" -v b="$shown" 'BEGIN { printf "%d", (b - a) * 1000 }')
		else
			ms=none
		fi
		if [ "$ms" != none ] && [ "$timeouts" = 0 ] && [ "$ms" -lt 2000 ]; then
			log "PASS first launch $n of $APP_A: window ${ms} ms after START"
		else
			failures=$((failures + 1))
			log "FAIL first launch $n of $APP_A: window ${ms} ms after START, $timeouts transition timeouts"
		fi
		n=$((n + 1))
	done
	go_home
}

relaunch() {
	runs=${1:-10}
	n=1
	while [ $n -le "$runs" ]; do
		android pm revoke "$APP_A" android.permission.POST_NOTIFICATIONS 2>/dev/null
		android pm clear-permission-flags "$APP_A" android.permission.POST_NOTIFICATIONS user-set user-fixed 2>/dev/null
		launch "$APP_A"
		android logcat -c
		expect "relaunch $n: $APP_A with its prompt" "$APP_A"
		android am force-stop "$APP_A"
		go_home
		launch "$APP_A"
		expect "relaunch $n: $APP_A after a force-stop" "$APP_A"
		n=$((n + 1))
	done
	go_home
}

case ${1:-all} in
	matrix) matrix ;;
	first-launch) first_launch "${2:-5}" ;;
	relaunch) relaunch "${2:-10}" ;;
	all) matrix; first_launch "${2:-5}"; relaunch 10 ;;
	*) echo "usage: app-switch.sh [matrix|first-launch [N]|relaunch [N]|all [N]]" >&2; exit 2 ;;
esac
log "$failures failed"
[ "$failures" = 0 ]
