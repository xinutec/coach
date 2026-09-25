package org.xinutec.coach

import org.junit.Assert.assertEquals
import org.junit.Test

/**
 * The ordering of the permission → set-home → arm flow.
 *
 * Worth its own test because the flow re-enters itself from three places — two
 * permission callbacks and an async location fix — so "which step is next" is
 * answered many times per run, from states that are awkward to reach by hand,
 * and a step that can never be reached looks exactly like a step that succeeded.
 */
class SetupFlowTest {
    /**
     * How a run stands with respect to one permission.
     *
     * [ALREADY] and [GRANTS] end in the same state by different routes; a
     * boolean could not tell them apart, and so could not express a fresh
     * install.
     */
    private enum class Answer {
        /** Held before setup began; never prompted for. */
        ALREADY,

        /** Not held; granted when asked. */
        GRANTS,

        /** Not held; refused when asked. */
        DENIES,
    }

    /**
     * One run of the flow, as the sequence of steps it actually takes.
     *
     * Note what is absent: whether a home is already stored. It is not an input
     * to [SetupFlow.next], so the harness cannot describe it either.
     */
    private fun run(
        fine: Answer = Answer.ALREADY,
        background: Answer = Answer.ALREADY,
        notifications: Answer = Answer.ALREADY,
        captureSucceeds: Boolean = true,
    ): List<SetupStep> {
        var hasFine = fine == Answer.ALREADY
        var hasBackground = background == Answer.ALREADY
        var hasNotifications = notifications == Answer.ALREADY
        var homeCaptured = false
        var notificationsAsked = false
        val taken = mutableListOf<SetupStep>()

        // The real flow is callback-driven and unbounded; 20 is far past any
        // legitimate route, and turns a non-terminating one into a failure
        // rather than a hung test.
        repeat(20) {
            val step =
                SetupFlow.next(
                    hasFine = hasFine,
                    homeCaptured = homeCaptured,
                    hasBackground = hasBackground,
                    hasNotifications = hasNotifications,
                    notificationsAsked = notificationsAsked,
                )
            taken += step
            when (step) {
                SetupStep.ASK_FINE -> {
                    if (fine == Answer.DENIES) return taken
                    hasFine = true
                }

                SetupStep.CAPTURE_HOME -> {
                    if (!captureSucceeds) return taken
                    homeCaptured = true
                }

                SetupStep.ASK_BACKGROUND -> {
                    if (background == Answer.DENIES) return taken
                    hasBackground = true
                }

                // Asked at most once and armed either way, so a denial here is
                // not the end of the run.
                SetupStep.ASK_NOTIFICATIONS -> {
                    notificationsAsked = true
                    hasNotifications = notifications != Answer.DENIES
                }

                SetupStep.ARM -> {
                    return taken
                }
            }
        }
        throw AssertionError("the flow did not terminate: $taken")
    }

    @Test
    fun `a fresh install walks every prerequisite once, then arms`() {
        assertEquals(
            listOf(
                SetupStep.ASK_FINE,
                SetupStep.CAPTURE_HOME,
                SetupStep.ASK_BACKGROUND,
                SetupStep.ASK_NOTIFICATIONS,
                SetupStep.ARM,
            ),
            run(fine = Answer.GRANTS, background = Answer.GRANTS, notifications = Answer.GRANTS),
        )
    }

    /**
     * ⚠ Everything already granted is a phone that has run setup before, which
     * is when the button reads "Update home & turn on". A capture guarded on a
     * stored home would make this run `[ARM]`: the home never moves, and the
     * toast still says "Reminders on".
     */
    @Test
    fun `a returning user still re-captures home before arming`() {
        assertEquals(listOf(SetupStep.CAPTURE_HOME, SetupStep.ARM), run())
    }

    /** The latch has to hold across the re-entries, or the capture would run
     *  again on every callback. */
    @Test
    fun `home is captured exactly once per run, never in a loop`() {
        val taken = run(fine = Answer.GRANTS, background = Answer.GRANTS)
        assertEquals(1, taken.count { it == SetupStep.CAPTURE_HOME })
    }

    /** Notifications are a nice-to-have: denying them must not cost the geofence,
     *  and must not re-ask forever — the nudge simply won't show until they are
     *  enabled in system settings. */
    @Test
    fun `a denied notification permission still arms, and is asked only once`() {
        val taken = run(notifications = Answer.DENIES)
        assertEquals(1, taken.count { it == SetupStep.ASK_NOTIFICATIONS })
        assertEquals(SetupStep.ARM, taken.last())
    }

    /** Location is not optional — without it there is nothing to arm against, so
     *  each refusal stops the run where it stands. */
    @Test
    fun `denying location ends the flow without arming`() {
        assertEquals(listOf(SetupStep.ASK_FINE), run(fine = Answer.DENIES))
        assertEquals(
            listOf(SetupStep.CAPTURE_HOME, SetupStep.ASK_BACKGROUND),
            run(background = Answer.DENIES),
        )
    }

    /** A capture that yields nothing stops the flow here. Whether the *caller*
     *  then carries on with a previously stored home is [MainActivity]'s
     *  decision, not this table's. */
    @Test
    fun `a failed capture does not fall through to arming`() {
        assertEquals(listOf(SetupStep.CAPTURE_HOME), run(captureSucceeds = false))
    }
}
