package org.xinutec.coach

import org.junit.Assert.assertTrue
import org.junit.Test
import java.net.URI

/**
 * The two constants that decide what the app is allowed to be.
 *
 * `ALLOWED_HOSTS` confines navigation to the WebView; anything else opens in the
 * real browser. If the app's own host ever fell out of that set the app would
 * launch itself straight into Chrome — and the bridge (`fromCoach`) tests the
 * URL against `BASE_URL`, so the reminder controls would go dead at the same
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

    // `fromCoach()` compares with startsWith, so a trailing slash here would make
    // every bridge call from the app itself fail the check.
    @Test
    fun `the base url is a prefix of the pages the bridge trusts`() {
        assertTrue(
            "BASE_URL must not end in a slash — fromCoach() compares with startsWith",
            !Config.BASE_URL.endsWith("/"),
        )
        assertTrue("${Config.BASE_URL}/settings".startsWith(Config.BASE_URL))
    }

    @Test
    fun `the login hop is allowed too, or signing in would leave the app`() {
        assertTrue(Config.ALLOWED_HOSTS.size >= 2)
    }
}
