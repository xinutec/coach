package org.xinutec.coach

/** The one thing the setup flow does next. */
internal enum class SetupStep {
    ASK_FINE,
    CAPTURE_HOME,
    ASK_BACKGROUND,
    ASK_NOTIFICATIONS,
    ARM,
}

/**
 * Which step of the permission → set-home → arm flow still has to happen.
 *
 * Lifted out of [MainActivity] because the activity's copy needed a live
 * location client and two permission dialogs to reach, so the ordering — the
 * part that can be wrong — was the part nothing could exercise.
 *
 * ⚠ **[next] deliberately cannot see whether a home is already stored.** It used
 * to: the capture was guarded on `!prefs.hasHome`, which meant that once a home
 * existed it could never be replaced, while the button went on offering "Update
 * home & turn on" and the toast went on saying it had worked. Home on the Pixel 9
 * sat unchanged from 2026-07-04 to 2026-09-12 because of it. The capture is now
 * latched per run instead, like [SetupStep.ASK_NOTIFICATIONS] beside it, so
 * re-running setup always re-reads where you are and the loop still terminates.
 *
 * Keeping `hasHome` out of the signature is the fix: the old condition is not
 * merely corrected, it is no longer expressible here.
 */
internal object SetupFlow {
    /**
     * [hasBackground] and [hasNotifications] are true on OS versions that do not
     * have the permission at all — "nothing left to ask for" rather than
     * "granted", which is what the caller needs and keeps the SDK check at the
     * edge where the `Build.VERSION` constants live.
     *
     * Notifications are the one prerequisite that is only nice to have: hence
     * [notificationsAsked], which lets the flow arm after a refusal instead of
     * asking again forever. The nudge then simply does not show until they are
     * enabled in system settings.
     */
    fun next(
        hasFine: Boolean,
        homeCaptured: Boolean,
        hasBackground: Boolean,
        hasNotifications: Boolean,
        notificationsAsked: Boolean,
    ): SetupStep =
        when {
            !hasFine -> SetupStep.ASK_FINE
            !homeCaptured -> SetupStep.CAPTURE_HOME
            !hasBackground -> SetupStep.ASK_BACKGROUND
            !hasNotifications && !notificationsAsked -> SetupStep.ASK_NOTIFICATIONS
            else -> SetupStep.ARM
        }
}
