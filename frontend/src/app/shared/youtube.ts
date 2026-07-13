/**
 * Demo links in the catalog are YouTube URLs, hand-picked and usually deep-linked
 * at the second the movement starts (`?t=`). To play one *in* the app rather than
 * throwing the athlete out to another tab mid-set, we need the video id and that
 * offset — so we parse the link rather than embedding it verbatim.
 */

/** An embeddable YouTube video: its id, and where in the clip the movement starts. */
export interface YoutubeRef {
	id: string;
	/** Seconds into the video to start at (0 = from the top). */
	startS: number;
}

const VIDEO_ID = /^[\w-]{11}$/;
/** `90`, or YouTube's `1h2m3s` spelling. */
const CLOCK = /^(?:(\d+)h)?(?:(\d+)m)?(?:(\d+)s?)?$/;

/**
 * Parse a demo URL into something we can embed, or `null` if it isn't a YouTube
 * link we recognise. Null means "link out to it" — we don't guess at an id and
 * serve a frame that renders an error where a video should be.
 */
export function parseYoutube(url: string): YoutubeRef | null {
	let u: URL;
	try {
		u = new URL(url);
	} catch {
		return null;
	}
	if (u.protocol !== "https:" && u.protocol !== "http:") return null;

	const host = u.hostname.replace(/^(www|m)\./, "");
	let id: string | null = null;
	if (host === "youtu.be") {
		id = u.pathname.slice(1);
	} else if (host === "youtube.com" || host === "youtube-nocookie.com") {
		if (u.pathname === "/watch") id = u.searchParams.get("v");
		else if (u.pathname.startsWith("/embed/")) id = u.pathname.slice("/embed/".length);
		else if (u.pathname.startsWith("/shorts/")) id = u.pathname.slice("/shorts/".length);
	}
	if (id === null || !VIDEO_ID.test(id)) return null;

	return { id, startS: parseStart(u.searchParams.get("t")) };
}

/** The privacy-preserving embed host — no tracking cookies, so no consent banner
 *  inside our own sheet. `rel=0` keeps YouTube from suggesting unrelated videos
 *  at the end, and `playsinline` stops iOS hijacking the whole screen. */
export function embedUrl(ref: YoutubeRef): string {
	const p = new URLSearchParams({
		start: String(ref.startS),
		autoplay: "1",
		rel: "0",
		playsinline: "1",
	});
	return `https://www.youtube-nocookie.com/embed/${ref.id}?${p}`;
}

function parseStart(t: string | null): number {
	if (t === null) return 0;
	const m = CLOCK.exec(t);
	if (!m) return 0;
	const [, h, min, s] = m;
	return Number(h ?? 0) * 3600 + Number(min ?? 0) * 60 + Number(s ?? 0);
}
