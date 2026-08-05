package org.xinutec.coach

import org.junit.Assert.assertTrue
import org.junit.Test
import java.net.URI

/**
 * The two constants that decide what the app is allowed to be.
 *
 * `ALLOWED_HOSTS` confines navigation to the WebView; anything else opens in the
 * real browser. If the app's own host ever fell out of that set the app would
 * launch itself straight into Chrome — and [Bridge] admits a message only from
 * `BASE_URL`'s own origin, so the reminder controls would go dead at the same
 * time.
 */
class ConfigTest {
    @Test
    fun `the app is allowed to load itself`() {
        val host = URI(Config.BASE_URL).host
        assertTrue(
            "$host is not in ALLOWED_HOSTS — the app would open itself in the browser",
            Config.ALLOWED_HOSTS.contains(host),
        )
    }

    // The session cookie is the reminder's only credential, and it is read back
    // out of the WebView by URL. Plain HTTP would put it on the wire.
    @Test
    fun `the app is served over https`() {
        assertTrue(Config.BASE_URL.startsWith("https://"))
    }

    /**
     * `BASE_URL` is handed to `addWebMessageListener` as the reminders bridge's
     * only allowed origin rule, and a rule is `scheme://host[:port]` — nothing
     * after the authority. A trailing slash or a path makes it malformed, and
     * malformed throws `IllegalArgumentException` when the WebView is created:
     * the app fails to start rather than the bridge quietly not working.
     */
    @Test
    fun `the base url is a well-formed origin rule`() {
        assertTrue("BASE_URL must not end in a slash", !Config.BASE_URL.endsWith("/"))
        assertTrue(
            "BASE_URL must carry no path",
            !Config.BASE_URL.substringAfter("://").contains("/"),
        )
        assertTrue("BASE_URL must carry no query", !Config.BASE_URL.contains("?"))
    }

    @Test
    fun `the login hop is allowed too, or signing in would leave the app`() {
        assertTrue(Config.ALLOWED_HOSTS.size >= 2)
    }
}
