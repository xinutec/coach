package org.xinutec.coach

import com.google.android.gms.location.Geofence
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * The phone's whole job is to decide when "you're home" has happened. The
 * backend applies every other gate — window, night cutoff, spacing — so this one
 * decision is the entire native contribution, and getting it wrong is silent in
 * both directions: too narrow and the reminder never fires, too wide and it
 * fires as you walk past your own front door.
 */
class GeofencingTest {
    @Test
    fun `arriving and settling both count`() {
        assertTrue(Geofencing.settledAtHome(Geofence.GEOFENCE_TRANSITION_ENTER))
        assertTrue(Geofencing.settledAtHome(Geofence.GEOFENCE_TRANSITION_DWELL))
    }

    @Test
    fun `leaving is not arriving`() {
        assertFalse(Geofencing.settledAtHome(Geofence.GEOFENCE_TRANSITION_EXIT))
    }

    @Test
    fun `a transition nobody registered is not an arrival`() {
        assertFalse(Geofencing.settledAtHome(0))
    }

    /**
     * The registration and the filter are the two ends of one mechanism, and they
     * are written in different files. Registering a crossing the receiver drops
     * is a reminder that never fires with no error anywhere to notice it, so the
     * agreement is asserted rather than assumed.
     */
    @Test
    fun `every registered crossing is one the receiver accepts`() {
        assertEquals(
            Geofence.GEOFENCE_TRANSITION_ENTER or Geofence.GEOFENCE_TRANSITION_DWELL,
            Geofencing.TRANSITIONS,
        )
        val registered =
            listOf(
                Geofence.GEOFENCE_TRANSITION_ENTER,
                Geofence.GEOFENCE_TRANSITION_DWELL,
            )
        for (bit in registered) {
            if ((Geofencing.TRANSITIONS and bit) != 0) {
                assertTrue(
                    "registered transition $bit is dropped by the receiver",
                    Geofencing.settledAtHome(bit),
                )
            }
        }
    }
}
