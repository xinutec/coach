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
 * Separate from [MainActivity] so the ordering can be tested without a location
 * client or permission dialogs.
 *
 * ⚠ **[next] deliberately cannot see whether a home is already stored.** Every
 * run of setup ("Update home & turn on") must re-read where you are; guarding
 * the capture on a stored home would make the first home permanent while the
 * UI reported an update. The capture is latched per run instead, like
 * [SetupStep.ASK_NOTIFICATIONS], so the loop still terminates.
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
