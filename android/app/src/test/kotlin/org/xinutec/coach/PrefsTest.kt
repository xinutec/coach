package org.xinutec.coach

import androidx.test.core.app.ApplicationProvider
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner

/**
 * The on-device home store.
 *
 * This is the one place the athlete's home coordinates exist — never in source,
 * never on the server. SharedPreferences has no Double, so they are stored as
 * the raw bits of one, and that indirection is worth pinning: a coordinate that
 * round-trips wrong doesn't fail, it moves the geofence somewhere else, and the
 * only symptom is reminders that fire in the wrong place or never.
 */
@RunWith(RobolectricTestRunner::class)
class PrefsTest {
    private lateinit var prefs: Prefs

    @Before
    fun setUp() {
        prefs = Prefs(ApplicationProvider.getApplicationContext())
    }

    @Test
    fun `a coordinate comes back the number it went in as`() {
        prefs.homeLat = 51.5072
        prefs.homeLng = -0.1276
        assertEquals(51.5072, prefs.homeLat!!, 0.0)
        assertEquals(-0.1276, prefs.homeLng!!, 0.0)
    }

    // Longitude is negative for half the planet and the sign lives in the top bit
    // of the raw representation — exactly the bit a wrong conversion loses.
    @Test
    fun `a negative coordinate keeps its sign`() {
        prefs.homeLng = -122.4194
        assertTrue(prefs.homeLng!! < 0)
    }

    @Test
    fun `full precision survives, not a rounded-off version of it`() {
        val exact = 51.50735931234567
        prefs.homeLat = exact
        assertEquals(exact, prefs.homeLat!!, 0.0)
    }

    // Distinguishable from a home at zero: 0,0 is in the Atlantic, but it is a
    // coordinate, and "unset" must not be stored as one.
    @Test
    fun `no home set reads as absent, not as the origin`() {
        assertNull(prefs.homeLat)
        assertNull(prefs.homeLng)
        assertFalse(prefs.hasHome)
        prefs.homeLat = 0.0
        prefs.homeLng = 0.0
        assertEquals(0.0, prefs.homeLat!!, 0.0)
        assertTrue(prefs.hasHome)
    }

    @Test
    fun `clearing a coordinate takes it away again`() {
        prefs.homeLat = 51.5
        prefs.homeLng = -0.12
        prefs.homeLat = null
        assertNull(prefs.homeLat)
        assertFalse("half a home is not a home", prefs.hasHome)
    }

    // Both halves are needed to describe a point; one of them is not a location.
    @Test
    fun `one coordinate is not a home`() {
        prefs.homeLat = 51.5
        assertFalse(prefs.hasHome)
    }

    @Test
    fun `reminders are off until they are turned on`() {
        assertFalse(prefs.armed)
        prefs.armed = true
        assertTrue(Prefs(ApplicationProvider.getApplicationContext()).armed)
    }

    // 150 m covers a house and garden without firing from the street.
    @Test
    fun `the radius has a sane default and can be changed`() {
        assertEquals(150f, prefs.radiusM, 0f)
        prefs.radiusM = 200f
        assertEquals(200f, Prefs(ApplicationProvider.getApplicationContext()).radiusM, 0f)
    }

    @Test
    fun `what one instance stores another instance reads`() {
        prefs.homeLat = 51.5072
        prefs.homeLng = -0.1276
        val other = Prefs(ApplicationProvider.getApplicationContext())
        assertTrue(other.hasHome)
        assertEquals(51.5072, other.homeLat!!, 0.0)
    }
}
