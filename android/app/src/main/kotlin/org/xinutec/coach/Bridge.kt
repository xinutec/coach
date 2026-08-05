package org.xinutec.coach

import org.xinutec.shell.sameOrigin

/**
 * What the Settings page asked the phone to do.
 *
 * A closed set rather than a string carried further inward: past this point the
 * activity dispatches on an enum the compiler can check it has handled, so a
 * fourth message cannot be added to the page and silently do nothing here.
 */
internal enum class BridgeAction {
    /** Report `hasHome` + `armed`. The page sends this on load. */
    STATUS,

    /** Run the permission → set-home → arm flow. */
    SETUP,

    /** Disarm the geofence and forget nothing else — home stays set. */
    DISABLE,
}

/**
 * Who may speak to the reminders bridge, and what they may say.
 *
 * Both halves of that question are answered here, as one pure function, because
 * they are one decision: a message is acted on only if it comes from coach's own
 * main frame *and* says something in the vocabulary. Keeping it out of
 * [MainActivity] is what makes it testable — the activity's copy needed a
 * WebView, a Play-services client and a live geofence to reach, so the origin
 * check was the one piece of the security fix that nothing could exercise.
 *
 * The frame and origin checks are belt and braces: `addWebMessageListener` has
 * already refused to inject the port into any frame outside
 * [ALLOWED_ORIGINS]. They are repeated because Android's own guidance says to,
 * and because a check that lives only in an argument list is one careless edit
 * from being gone with nothing failing.
 */
internal object Bridge {
    /** `window.CoachAndroid` on the page. Its presence is still how the Settings
     *  page knows it is running inside the native app. */
    const val NAME = "CoachAndroid"

    /** The only origin the bridge is injected into. An origin rule is
     *  `scheme://host[:port]` with no trailing slash — which [Config.BASE_URL]
     *  already is, and a test in ConfigTest keeps it that way. */
    val ALLOWED_ORIGINS = setOf(Config.BASE_URL)

    private const val MSG_STATUS = "status"
    private const val MSG_SETUP = "setup"
    private const val MSG_DISABLE = "disable"

    /**
     * The action a message earns, or `null` for every message that earns none.
     *
     * Null covers three different refusals on purpose — wrong frame, wrong
     * origin, unknown word — because the caller's response to all three is the
     * same silence. Answering them differently would tell a frame that got this
     * far which of its guesses was closest.
     *
     * [data] is nullable because a non-text `WebMessageCompat` has no string to
     * read; it falls through the `when` like any other word we don't know,
     * rather than being special-cased on a message type whose exact null
     * behaviour varies by androidx.webkit version.
     */
    fun actionFor(data: String?, sourceOrigin: String?, isMainFrame: Boolean): BridgeAction? {
        if (!isMainFrame) return null
        if (!sameOrigin(Config.BASE_URL, sourceOrigin)) return null
        return when (data) {
            MSG_STATUS -> BridgeAction.STATUS
            MSG_SETUP -> BridgeAction.SETUP
            MSG_DISABLE -> BridgeAction.DISABLE
            else -> null
        }
    }
}
