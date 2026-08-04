import { Component, computed, inject, signal } from "@angular/core";
import { FormsModule } from "@angular/forms";
import { MatButtonModule } from "@angular/material/button";
import { MatFormFieldModule } from "@angular/material/form-field";
import { MatIconModule } from "@angular/material/icon";
import { MatInputModule } from "@angular/material/input";
import { MatSelectModule } from "@angular/material/select";
import { RouterLink } from "@angular/router";

import { BUILD_INFO } from "../../build-info";
import { CoachApi } from "../../coach-api";
import type { Settings } from "../../models";
import { isRecord } from "../../shared/narrow";
import { SwUpdates } from "../../sw-updates";

/** The native bridge the Android wrapper injects as `window.CoachAndroid`. Its
 *  presence is how we know we're running inside the app and can offer the
 *  on-device home-geofence reminders (the geofence + notifications are native;
 *  the home location never leaves the phone). Absent in a plain browser.
 *
 *  A message port rather than the three plain methods it used to be. The wrapper
 *  injects it with `WebViewCompat.addWebMessageListener`, whose origin rules keep
 *  it out of every frame that isn't this app — the library sheet embeds a YouTube
 *  player, and the API this replaced was injected into that frame too. The shape
 *  is the platform's `MessagePort`, so it is a `postMessage` out and a `message`
 *  event back, and `remindersStatus()` could no longer be a return value. */
interface CoachAndroidBridge {
	postMessage(message: string): void;
	addEventListener(
		type: "message",
		listener: (event: { data: string }) => void,
	): void;
}

/** What we can ask the phone for. Three words, matching MainActivity. */
type BridgeRequest = "status" | "setup" | "disable";

/** The shape an app older than this page injects: three plain methods, from the
 *  `addJavascriptInterface` era. The app is sideloaded, so the page always
 *  updates first and can be running against either — and calling
 *  `addEventListener` on this one throws, which would take the whole Settings
 *  page down rather than just the reminders card. */
interface LegacyCoachAndroidBridge {
	remindersStatus(): string;
	setupReminders(): void;
	disableReminders(): void;
}
// Declared on Window rather than asserted at the read. An ambient declaration is
// what a foreign API contract is *for*: it says the shape once, in one place, so
// the reads are ordinary typed property accesses instead of a cast each site has
// to get right.
declare global {
	interface Window {
		CoachAndroid?: CoachAndroidBridge | LegacyCoachAndroidBridge;
	}
}
/** The bridge, if the installed app speaks this page's version of it.
 *
 *  An older app reads as no bridge at all: the reminders card is then absent,
 *  exactly as in a desktop browser, and installing the current APK brings it
 *  back. Narrowed with `in` rather than asserted — which of the two is there is a
 *  fact about the phone, not something this page gets to decide. */
function coachAndroid(): CoachAndroidBridge | null {
	const bridge = window.CoachAndroid;
	if (bridge === undefined) return null;
	return "postMessage" in bridge ? bridge : null;
}

@Component({
	selector: "app-settings",
	templateUrl: "./settings.html",
	styleUrl: "./settings.scss",
	imports: [
		FormsModule,
		MatButtonModule,
		MatFormFieldModule,
		MatIconModule,
		MatInputModule,
		MatSelectModule,
		RouterLink,
	],
})
export class SettingsPage {
	private api = inject(CoachApi);
	private swUpdates = inject(SwUpdates);

	readonly modes = ["balanced", "strength", "skills", "conditioning"] as const;
	readonly regions = [
		"chest",
		"back",
		"shoulders",
		"arms",
		"forearms",
		"core",
		"legs",
	] as const;
	label(s: string): string {
		return s.charAt(0).toUpperCase() + s.slice(1);
	}

	// Signal so a zoneless view refreshes when the async load/save resolves. The
	// form fields two-way-bind to the held object's properties (mutating them in
	// place is fine — only the object reference is swapped via .set()).
	readonly form = signal<Settings | null>(null);
	readonly saving = signal(false);
	readonly saved = signal(false);
	readonly updateMsg = signal("");

	// Home-reminders state, only meaningful inside the Android app.
	readonly isAndroid = signal(false);
	readonly remindersArmed = signal(false);
	readonly remindersHasHome = signal(false);

	constructor() {
		this.api.settings().subscribe((s) => this.form.set(s));
		this.refreshReminders();
	}

	/** Listen for what the phone says about the reminders, then ask once.
	 *
	 *  Every answer arrives the same way, whether we asked for it or the native
	 *  flow finished and volunteered it. That replaces a pair of `setTimeout`s
	 *  that re-read the state 1500 ms after asking to set up — a guess at how long
	 *  someone takes to answer two permission dialogs, and wrong in both
	 *  directions: too short and the page showed the old state, too long and it
	 *  sat there having already succeeded. */
	private refreshReminders(): void {
		const bridge = coachAndroid();
		this.isAndroid.set(bridge !== null);
		if (bridge === null) return;
		bridge.addEventListener("message", (event) => this.onBridgeMessage(event.data));
		this.ask("status");
	}

	private ask(request: BridgeRequest): void {
		coachAndroid()?.postMessage(request);
	}

	private onBridgeMessage(data: string): void {
		try {
			const status: unknown = JSON.parse(data);
			if (!isRecord(status)) return;
			this.remindersHasHome.set(status["hasHome"] === true);
			this.remindersArmed.set(status["armed"] === true);
		} catch {
			// Bridge said something unexpected — leave the defaults.
		}
	}

	/** Kick off the native set-home + arm flow (permission dialogs are native).
	 *  The phone reports the outcome when the flow settles. */
	enableReminders(): void {
		this.ask("setup");
	}

	disableReminders(): void {
		this.ask("disable");
	}

	save(): void {
		const f = this.form();
		if (!f) return;
		this.saving.set(true);
		this.api.patchSettings({ ...f }).subscribe({
			next: (s) => {
				this.form.set(s);
				this.saving.set(false);
				this.saved.set(true);
				setTimeout(() => this.saved.set(false), 2000);
			},
			error: () => this.saving.set(false),
		});
	}

	/** Which build this tab is running — the commit and when it was built. Uses the
	 *  stamp compiled into *this* bundle, so a stale cached tab reports its own old
	 *  commit instead of the server's; that's what makes "Up to date." checkable. */
	readonly buildStamp = computed(() => {
		const at = new Date(BUILD_INFO.builtAt);
		const when = Number.isNaN(at.getTime()) ? BUILD_INFO.builtAt : at.toLocaleString();
		return `Build ${BUILD_INFO.sha} · ${when}`;
	});

	async checkUpdates(): Promise<void> {
		const r = await this.swUpdates.checkNow();
		this.updateMsg.set(
			r === "current"
				? "Up to date."
				: r === "updating"
					? "Updating…"
					: "No service worker (dev build).",
		);
	}
}
