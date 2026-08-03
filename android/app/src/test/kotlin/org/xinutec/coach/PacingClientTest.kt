package org.xinutec.coach

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner

/**
 * What the phone makes of the coach's answer.
 *
 * The reminder reads two fields out of a verdict that carries a couple of dozen,
 * and the verdict is the app's most-changed type — `Ask` was retagged, `Readiness`
 * gained a constructor, `distance_m` arrived, and none of that is the reminder's
 * business. A parse that broke on any of it would show up as a reminder that
 * quietly stopped firing, on a phone, with no log anyone reads.
 */
@RunWith(RobolectricTestRunner::class)
class PacingClientTest {
    @Test
    fun `a verdict says whether to nudge and what to say`() {
        val v = PacingClient.parse("""{"nudge":true,"reason":"Chest is untrained this week"}""")
        assertTrue(v!!.nudge)
        assertEquals("Chest is untrained this week", v.reason)
    }

    @Test
    fun `a verdict that says no is not a nudge`() {
        assertFalse(PacingClient.parse("""{"nudge":false,"reason":"Rest up"}""")!!.nudge)
    }

    // The fields the reminder doesn't read are the ones that keep changing. A
    // response carrying the whole plan, or a response that has lost a field
    // entirely, must both still yield the two this cares about.
    @Test
    fun `the rest of the verdict is none of its business`() {
        val body =
            """
            {"state":"active","deload":false,"readiness":null,"nudge":true,
             "reason":"Time to train","window":"within","spacingOk":true,
             "minutesSinceLastSet":null,"dayTargetSets":10,"dayDoneSets":0,
             "groups":[],"suggestion":null,"plan":[],"notices":[]}
            """.trimIndent()
        assertTrue(PacingClient.parse(body)!!.nudge)
    }

    @Test
    fun `a verdict missing the fields it reads still yields something to show`() {
        val v = PacingClient.parse("{}")
        assertFalse(v!!.nudge)
        assertEquals("Time to train", v.reason)
    }

    // A body that isn't a verdict reaches the same outcome as a verdict that says
    // "not now" — silence. The phone never guesses.
    @Test
    fun `nothing that fails to parse becomes a reminder`() {
        assertNull(PacingClient.parse(""))
        assertNull(PacingClient.parse("not json at all"))
        assertNull(PacingClient.parse("[]"))
        assertNull(PacingClient.parse("<html>login</html>"))
    }
}
