package org.marathonos.deckard.system;

import android.app.Activity;

/**
 * The result Android's Settings gives an app that asked for a settings page:
 * RESULT_OK only when the person turned on the access the app asked for
 * (ExternalSourcesDetails, AlarmsAndRemindersDetails); everything else,
 * the battery request included, answers RESULT_CANCELED.
 */
final class SettingsResult {
    private SettingsResult() {}

    /** The special-access kind whose change the action reports, or null. */
    static String reported(String action) {
        if ("android.settings.MANAGE_UNKNOWN_APP_SOURCES".equals(action))
            return "install_apps";
        if ("android.settings.REQUEST_SCHEDULE_EXACT_ALARM".equals(action))
            return "alarms";
        return null;
    }

    static int code(boolean before, boolean after) {
        return !before && after ? Activity.RESULT_OK : Activity.RESULT_CANCELED;
    }
}
