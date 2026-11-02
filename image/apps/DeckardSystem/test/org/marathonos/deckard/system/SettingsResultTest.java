package org.marathonos.deckard.system;

import android.app.Activity;

// Runs on the build host's JVM: SettingsResult only uses Activity's result constants.
public final class SettingsResultTest {
    private static int failures;

    private static void check(String name, Object got, Object want) {
        if (got == null ? want != null : !got.equals(want)) {
            failures++;
            System.err.println("FAIL " + name + ": " + got + ", want " + want);
        }
    }

    public static void main(String[] args) {
        check("unknown sources", SettingsResult.reported("android.settings.MANAGE_UNKNOWN_APP_SOURCES"), "install_apps");
        check("exact alarms", SettingsResult.reported("android.settings.REQUEST_SCHEDULE_EXACT_ALARM"), "alarms");
        check("battery request", SettingsResult.reported("android.settings.REQUEST_IGNORE_BATTERY_OPTIMIZATIONS"), null);
        check("open by default", SettingsResult.reported("android.settings.APP_OPEN_BY_DEFAULT_SETTINGS"), null);
        check("turned on", SettingsResult.code(false, true), Activity.RESULT_OK);
        check("left off", SettingsResult.code(false, false), Activity.RESULT_CANCELED);
        check("already on", SettingsResult.code(true, true), Activity.RESULT_CANCELED);
        check("turned off", SettingsResult.code(true, false), Activity.RESULT_CANCELED);
        if (failures > 0) {
            System.err.println(failures + " failed");
            System.exit(1);
        }
    }
}
