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

/**
 * coach (the Angular app at [Config.BASE_URL]) in the fleet's shared
 * [WebShellActivity] — session cookie kept, so the Nextcloud sign-in is one-time.
 *
 * The home-geofence reminders are configured from the web app's own Settings page
 * (there's no native chrome overlaying the web UI): the page posts to the
 * [Bridge] message port, which drives the native permission →
 * set-home → arm flow. The geofence itself + notifications are native (see
 * [Geofencing], [GeofenceBroadcastReceiver]); the home location is stored
 * on-device only ([Prefs]).
 *
 * What the page is allowed to say, and from where, is [Bridge].
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
    private var homeCaptured = false

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
     * The library sheet embeds a `youtube-nocookie.com` player, so the WebView
     * runs somebody else's code. `addJavascriptInterface` would hand that frame
     * the bridge too: Android documents it as "available to every frame within
     * the WebView, including iframes. It lacks origin-based access control."
     * `addWebMessageListener` injects only into frames matching
     * [Bridge.ALLOWED_ORIGINS], and [Bridge.actionFor] re-checks origin and
     * frame on every message, as Android's guidance recommends.
     *
     * With no [WebViewFeature.WEB_MESSAGE_LISTENER] the bridge is absent and the
     * Settings page shows no reminders controls, as in a desktop browser.
     * Falling back to `addJavascriptInterface` would reopen the hole.
     */
    override fun onWebViewCreated(web: WebView) {
        if (!WebViewFeature.isFeatureSupported(WebViewFeature.WEB_MESSAGE_LISTENER)) return
        WebViewCompat.addWebMessageListener(
            web,
            Bridge.NAME,
            Bridge.ALLOWED_ORIGINS,
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
     * One message in, one action. Who may speak and what the words mean is
     * [Bridge.actionFor]'s decision — everything that reaches the `when` has
     * already been established to be coach's own main frame saying something we
     * answer. A message that earns no action does not become the reply target
     * either: `reply` moves only for a caller we are actually talking to.
     */
    private fun onBridgeMessage(
        @Suppress("UNUSED_PARAMETER") view: WebView, // the WebView callback's signature
        message: WebMessageCompat,
        sourceOrigin: Uri,
        isMainFrame: Boolean,
        proxy: JavaScriptReplyProxy,
    ) {
        val action = Bridge.actionFor(message.data, sourceOrigin.toString(), isMainFrame) ?: return
        reply = proxy
        when (action) {
            BridgeAction.STATUS -> {
                postStatus()
            }

            BridgeAction.SETUP -> {
                beginSetup()
            }

            BridgeAction.DISABLE -> {
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
        homeCaptured = false
        continueSetup()
    }

    // Walk the prerequisites in order; each missing one is requested and the flow
    // resumes from onRequestPermissionsResult (or captureHome's callback). Which
    // one is next is [SetupFlow]'s decision, so that the ordering is testable
    // without a location client; this half is only the doing.
    private fun continueSetup() {
        if (!setupInProgress) return
        val step =
            SetupFlow.next(
                hasFine = hasPerm(Manifest.permission.ACCESS_FINE_LOCATION),
                homeCaptured = homeCaptured,
                hasBackground =
                    Build.VERSION.SDK_INT < Build.VERSION_CODES.Q ||
                        hasPerm(Manifest.permission.ACCESS_BACKGROUND_LOCATION),
                hasNotifications =
                    Build.VERSION.SDK_INT < Build.VERSION_CODES.TIRAMISU ||
                        hasPerm(Manifest.permission.POST_NOTIFICATIONS),
                notificationsAsked = notifAsked,
            )
        // Exhaustive on the enum: a step added to the flow cannot silently do
        // nothing here.
        when (step) {
            SetupStep.ASK_FINE -> {
                requestPermissions(arrayOf(Manifest.permission.ACCESS_FINE_LOCATION), REQ_FINE)
            }

            SetupStep.CAPTURE_HOME -> {
                homeCaptured = true
                captureHome()
            }

            SetupStep.ASK_BACKGROUND -> {
                requestPermissions(
                    arrayOf(Manifest.permission.ACCESS_BACKGROUND_LOCATION),
                    REQ_BG,
                )
            }

            SetupStep.ASK_NOTIFICATIONS -> {
                notifAsked = true
                requestPermissions(arrayOf(Manifest.permission.POST_NOTIFICATIONS), REQ_NOTIF)
            }

            SetupStep.ARM -> {
                Prefs(this).armed = true
                val ok = Geofencing.arm(this)
                settle(
                    if (ok) {
                        "Reminders on — I'll nudge you when you're home."
                    } else {
                        "Couldn't arm the geofence."
                    },
                )
            }
        }
    }

    /**
     * The flow is over, whichever way it went: say so, and tell the page. The
     * phone reports the end because only it knows when the permission dialogs
     * have been answered; a page-side timer would be a guess.
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
                    noFix("Couldn't get a location fix — try again near a window.")
                }
            }.addOnFailureListener {
                noFix("Location unavailable.")
            }
    }

    /**
     * A capture that produced nothing, which is not the same as a flow that has
     * to stop.
     *
     * With no stored home the flow is over. With one, only the *move* failed:
     * arm on the stored home rather than make re-enabling reminders depend on
     * getting a fix indoors, and say so, so a stale home is never kept silently.
     */
    private fun noFix(why: String) {
        if (Prefs(this).hasHome) {
            toast("$why Keeping your previous home.")
            continueSetup()
        } else {
            settle(why)
        }
    }

    // Still the request-code API rather than the Activity Result one: the flow is
    // resumed from several places and re-enters itself, so the request code is the
    // state machine's own signal, not a launcher's callback.
    @Deprecated("Deprecated in Java")
    @Suppress("DEPRECATION") // the request code is the state machine's own signal (above)
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
    }
}
