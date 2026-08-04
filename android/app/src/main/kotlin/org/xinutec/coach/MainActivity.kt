package org.xinutec.coach

import android.Manifest
import android.annotation.SuppressLint
import android.content.pm.PackageManager
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.webkit.WebView
import android.widget.Toast
import androidx.core.content.ContextCompat
import androidx.webkit.JavaScriptReplyProxy
import androidx.webkit.WebMessageCompat
import androidx.webkit.WebViewCompat
import androidx.webkit.WebViewFeature
import com.google.android.gms.location.LocationServices
import com.google.android.gms.location.Priority
import com.google.android.gms.tasks.CancellationTokenSource
import org.json.JSONObject
import org.xinutec.shell.ShellConfig
import org.xinutec.shell.WebShellActivity
import org.xinutec.shell.sameOrigin

/**
 * coach (the Angular app at [Config.BASE_URL]) in the fleet's shared
 * [WebShellActivity] — session cookie kept, so the Nextcloud sign-in is one-time.
 *
 * The home-geofence reminders are configured from the web app's own Settings page
 * (there's no native chrome overlaying the web UI): the page calls the
 * [CoachBridge] `@JavascriptInterface`, which drives the native permission →
 * set-home → arm flow. The geofence itself + notifications are native (see
 * [Geofencing], [GeofenceBroadcastReceiver]); the home location is stored
 * on-device only ([Prefs]).
 */
class MainActivity : WebShellActivity() {
    override val shell =
        ShellConfig(
            url = Config.BASE_URL,
            allowedHosts = Config.ALLOWED_HOSTS,
            consoleTag = "coach-web",
        )

    // Drives the multi-step permission → set-home → arm flow across the async
    // location fetch and the permission-result callbacks.
    private var setupInProgress = false
    private var notifAsked = false

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        // Re-register the geofence if reminders were armed before (e.g. after an
        // app update). No-op if not armed / permissions missing.
        Geofencing.arm(this)
    }

    /**
     * Expose the reminders bridge to the coach app's own pages, and to nothing
     * else in the WebView.
     *
     * This was `addJavascriptInterface`, which Android documents as "available to
     * every frame within the WebView, including iframes. It lacks origin-based
     * access control." The library sheet embeds a `youtube-nocookie.com` player
     * so a demo can be watched mid-warm-up without leaving the app — so the
     * WebView deliberately runs somebody else's code, which is the exact
     * condition the API's own warning names. That frame could call
     * `setupReminders()`, and the old main-frame URL check passed, because the
     * main frame really was coach.
     *
     * `addWebMessageListener` is the origin-scoped replacement: the WebView
     * itself guarantees the object is only injected into frames matching
     * [ALLOWED_ORIGINS]. The `sourceOrigin` and `isMainFrame` checks below are
     * belt and braces on top of that, which is what Android's own guidance
     * recommends rather than trusting the rules alone.
     *
     * With no [WebViewFeature.WEB_MESSAGE_LISTENER] the bridge is simply absent —
     * the Settings page then shows no reminders controls, exactly as it does in a
     * desktop browser. Degrading to `addJavascriptInterface` would be re-opening
     * the hole on the devices least able to afford it.
     */
    override fun onWebViewCreated(web: WebView) {
        if (!WebViewFeature.isFeatureSupported(WebViewFeature.WEB_MESSAGE_LISTENER)) return
        WebViewCompat.addWebMessageListener(
            web,
            BRIDGE_NAME,
            ALLOWED_ORIGINS,
            ::onBridgeMessage,
        )
    }

    // ---- bridge for the web Settings page ----

    /** The page's channel back to us, kept so the flow can report when it settles
     *  rather than making the page guess with a timer. Null until it first speaks,
     *  and stale after a reload — a push that lands nowhere is not an error, the
     *  page asks again on load. */
    private var reply: JavaScriptReplyProxy? = null

    /**
     * One message in, one action. The vocabulary is three words; anything else is
     * ignored rather than answered, so a frame that got this far learns nothing
     * from probing it.
     */
    private fun onBridgeMessage(
        @Suppress("UNUSED_PARAMETER") view: WebView,
        message: WebMessageCompat,
        sourceOrigin: Uri,
        isMainFrame: Boolean,
        proxy: JavaScriptReplyProxy,
    ) {
        // The origin rules already did this. Checked again because a bridge that
        // depends on one line being right elsewhere is a bridge that breaks when
        // that line is edited by someone who doesn't know it is load-bearing.
        if (!isMainFrame) return
        if (!sameOrigin(Config.BASE_URL, sourceOrigin.toString())) return
        reply = proxy
        when (message.data) {
            MSG_STATUS -> {
                postStatus()
            }

            MSG_SETUP -> {
                beginSetup()
            }

            MSG_DISABLE -> {
                Prefs(this).armed = false
                Geofencing.disarm(this)
                toast("Reminders off.")
                postStatus()
            }
        }
    }

    /** Tell the page what the on-device state is: whether a home has been set and
     *  whether reminders are armed. Two booleans — never the coordinates, which do
     *  not leave the phone. */
    private fun postStatus() {
        val p = Prefs(this)
        reply?.postMessage(
            JSONObject().put("hasHome", p.hasHome).put("armed", p.armed).toString(),
        )
    }

    // ---- geofence setup flow ----

    private fun beginSetup() {
        setupInProgress = true
        notifAsked = false
        continueSetup()
    }

    // Walk the prerequisites in order; each missing one is requested and the flow
    // resumes from onRequestPermissionsResult (or captureHome's callback).
    private fun continueSetup() {
        if (!setupInProgress) return
        if (!hasPerm(Manifest.permission.ACCESS_FINE_LOCATION)) {
            requestPermissions(arrayOf(Manifest.permission.ACCESS_FINE_LOCATION), REQ_FINE)
            return
        }
        // Always re-capture home on setup (the user may be setting it fresh).
        if (!Prefs(this).hasHome) {
            captureHome()
            return
        }
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q &&
            !hasPerm(Manifest.permission.ACCESS_BACKGROUND_LOCATION)
        ) {
            requestPermissions(arrayOf(Manifest.permission.ACCESS_BACKGROUND_LOCATION), REQ_BG)
            return
        }
        // Notifications: nice-to-have — if denied we still arm (the nudge just
        // won't show until enabled in settings), so ask at most once.
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU &&
            !hasPerm(Manifest.permission.POST_NOTIFICATIONS) &&
            !notifAsked
        ) {
            notifAsked = true
            requestPermissions(arrayOf(Manifest.permission.POST_NOTIFICATIONS), REQ_NOTIF)
            return
        }

        val prefs = Prefs(this)
        prefs.armed = true
        val ok = Geofencing.arm(this)
        settle(
            if (ok) {
                "Reminders on — I'll nudge you when you're home."
            } else {
                "Couldn't arm the geofence."
            },
        )
    }

    /**
     * The flow is over, whichever way it went: say so, and tell the page.
     *
     * The page used to re-read the state on a `setTimeout` — 1500 ms after asking
     * to set up, which is a guess about how long someone takes to answer two
     * permission dialogs, and wrong in both directions. It settles when it
     * settles, and now the phone is the one that says so.
     */
    private fun settle(message: String) {
        setupInProgress = false
        toast(message)
        postStatus()
    }

    @SuppressLint("MissingPermission") // FINE is checked in continueSetup before we get here
    private fun captureHome() {
        toast("Getting your location…")
        LocationServices
            .getFusedLocationProviderClient(this)
            .getCurrentLocation(Priority.PRIORITY_HIGH_ACCURACY, CancellationTokenSource().token)
            .addOnSuccessListener { loc ->
                if (loc != null) {
                    val prefs = Prefs(this)
                    prefs.homeLat = loc.latitude
                    prefs.homeLng = loc.longitude
                    toast("Home set to here.")
                    continueSetup()
                } else {
                    settle("Couldn't get a location fix — try again near a window.")
                }
            }.addOnFailureListener {
                settle("Location unavailable.")
            }
    }

    // Still the request-code API rather than the Activity Result one: the flow is
    // resumed from several places and re-enters itself, so the request code is the
    // state machine's own signal, not a launcher's callback.
    @Deprecated("Deprecated in Java")
    @Suppress("DEPRECATION")
    override fun onRequestPermissionsResult(
        requestCode: Int,
        permissions: Array<String>,
        grantResults: IntArray,
    ) {
        super.onRequestPermissionsResult(requestCode, permissions, grantResults)
        val granted =
            grantResults.isNotEmpty() && grantResults[0] == PackageManager.PERMISSION_GRANTED
        when (requestCode) {
            REQ_FINE -> {
                if (granted) {
                    continueSetup()
                } else {
                    settle("Location is needed to know when you're home.")
                }
            }

            REQ_BG -> {
                if (granted) {
                    continueSetup()
                } else {
                    settle("Set location to \"Allow all the time\" for home reminders to work.")
                }
            }

            // Notifications: proceed to arm whether or not it was granted.
            REQ_NOTIF -> {
                continueSetup()
            }
        }
    }

    private fun hasPerm(p: String) =
        ContextCompat.checkSelfPermission(this, p) == PackageManager.PERMISSION_GRANTED

    private fun toast(m: String) = Toast.makeText(this, m, Toast.LENGTH_SHORT).show()

    private companion object {
        const val REQ_FINE = 101
        const val REQ_BG = 102
        const val REQ_NOTIF = 103

        /** `window.CoachAndroid` on the page. Its presence is still how the
         *  Settings page knows it is running inside the native app. */
        const val BRIDGE_NAME = "CoachAndroid"

        /** The only origin the bridge is injected into. An origin rule is
         *  `scheme://host[:port]` with no trailing slash — which [Config.BASE_URL]
         *  already is, and a test in ConfigTest keeps it that way. */
        val ALLOWED_ORIGINS = setOf(Config.BASE_URL)

        const val MSG_STATUS = "status"
        const val MSG_SETUP = "setup"
        const val MSG_DISABLE = "disable"
    }
}
