package org.xinutec.coach

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

/**
 * The reminders bridge's admission check.
 *
 * `addJavascriptInterface` — what this replaced — is documented by Android as
 * "available to every frame within the WebView, including iframes. It lacks
 * origin-based access control." That was not theoretical here: the library sheet
 * embeds a `youtube-nocookie.com` player so a demo can be watched mid-warm-up, so
 * the WebView deliberately runs somebody else's code, and the old main-frame URL
 * check passed because the main frame really was coach.
 *
 * `addWebMessageListener` moves the guarantee into the WebView, which will not
 * inject the port outside `Bridge.ALLOWED_ORIGINS` at all. These tests cover the
 * second layer — the one that still has to be right if the origin rule is ever
 * edited by someone who doesn't know it is load-bearing.
 */
class BridgeTest {
    private val coach = Config.BASE_URL

    // --- the vocabulary ---

    @Test
    fun `each word maps to its action`() {
        assertEquals(BridgeAction.STATUS, Bridge.actionFor("status", coach, true))
        assertEquals(BridgeAction.SETUP, Bridge.actionFor("setup", coach, true))
        assertEquals(BridgeAction.DISABLE, Bridge.actionFor("disable", coach, true))
    }

    @Test
    fun `a word we don't know earns nothing`() {
        assertNull(Bridge.actionFor("sethome", coach, true))
        assertNull(Bridge.actionFor("", coach, true))
        assertNull(Bridge.actionFor("STATUS", coach, true))
    }

    /** A non-text message has no string to read. It must fall through like any
     *  other unknown word rather than throwing out of the listener. */
    @Test
    fun `a message with no text earns nothing`() {
        assertNull(Bridge.actionFor(null, coach, true))
    }

    // --- who is allowed to say it ---

    /** The embedded player is the concrete reason this check exists. */
    @Test
    fun `a subframe is refused even saying the right word`() {
        assertNull(Bridge.actionFor("setup", coach, false))
        assertNull(Bridge.actionFor("status", coach, false))
        assertNull(Bridge.actionFor("disable", coach, false))
    }

    @Test
    fun `the embedded video host cannot arm a geofence`() {
        assertNull(Bridge.actionFor("setup", "https://www.youtube-nocookie.com", false))
        // Even if it somehow reached us as a main frame, the origin still fails.
        assertNull(Bridge.actionFor("setup", "https://www.youtube-nocookie.com", true))
    }

    /**
     * A prefix test would admit this. `sameOrigin` compares scheme + authority,
     * so a host that merely *starts with* ours is a different origin.
     */
    @Test
    fun `a lookalike host is a different origin`() {
        assertNull(Bridge.actionFor("setup", "https://coach.xinutec.org.evil.test", true))
        assertNull(Bridge.actionFor("setup", "https://evil.test/coach.xinutec.org", true))
        assertNull(Bridge.actionFor("setup", "https://xinutec.org", true))
    }

    /** The session cookie is the bridge's only credential. */
    @Test
    fun `the same host over plain http is a different origin`() {
        assertNull(Bridge.actionFor("setup", "http://coach.xinutec.org", true))
    }

    @Test
    fun `a port makes it a different origin`() {
        assertNull(Bridge.actionFor("setup", "https://coach.xinutec.org:8443", true))
    }

    @Test
    fun `a missing origin is refused`() {
        assertNull(Bridge.actionFor("setup", null, true))
        assertNull(Bridge.actionFor("setup", "", true))
    }

    /**
     * `sourceOrigin` arrives as a `Uri` that the activity stringifies, and an
     * origin has no path — but the page's own URL does, so the check must not
     * become path-sensitive if that ever changes.
     */
    @Test
    fun `our own origin is accepted however the caller spells the trailing slash`() {
        assertEquals(BridgeAction.STATUS, Bridge.actionFor("status", coach, true))
        assertEquals(BridgeAction.STATUS, Bridge.actionFor("status", "$coach/", true))
        assertEquals(BridgeAction.STATUS, Bridge.actionFor("status", "$coach/settings", true))
    }

    // --- the constants the WebView is handed ---

    @Test
    fun `the only allowed origin is coach itself`() {
        assertEquals(setOf(Config.BASE_URL), Bridge.ALLOWED_ORIGINS)
    }

    /** The page feature-detects on this name; renaming it silently removes the
     *  reminders card rather than breaking anything loudly. */
    @Test
    fun `the bridge keeps the name the page looks for`() {
        assertEquals("CoachAndroid", Bridge.NAME)
    }
}
