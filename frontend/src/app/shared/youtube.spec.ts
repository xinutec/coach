import { describe, expect, it } from "vitest";

import { embedUrl, parseYoutube } from "./youtube";

describe("parseYoutube", () => {
	it("reads the short form the catalog mostly uses", () => {
		expect(parseYoutube("https://youtu.be/3S5rnnI7VSs")).toEqual({
			id: "3S5rnnI7VSs",
			startS: 0,
		});
	});

	it("keeps the start offset — the link points at the rep, not the intro", () => {
		expect(parseYoutube("https://youtu.be/3S5rnnI7VSs?t=11")?.startS).toBe(11);
		expect(parseYoutube("https://www.youtube.com/watch?v=_pykNV65JEQ&t=1m30s")?.startS).toBe(90);
		expect(parseYoutube("https://youtu.be/3S5rnnI7VSs?t=1h2m3s")?.startS).toBe(3723);
	});

	it("reads the long form", () => {
		expect(parseYoutube("https://www.youtube.com/watch?v=_pykNV65JEQ")?.id).toBe("_pykNV65JEQ");
	});

	// A link we can't parse gets linked out, not framed — an embed built from a
	// guessed id renders YouTube's error page where the demo should be.
	it("declines anything that isn't a YouTube video", () => {
		expect(parseYoutube("https://youtube.be/3GFZpOYu0pQ")).toBeNull();
		expect(parseYoutube("https://vimeo.com/12345")).toBeNull();
		expect(parseYoutube("https://www.youtube.com/watch?v=short")).toBeNull();
		expect(parseYoutube("not a url")).toBeNull();
		expect(parseYoutube("javascript:alert(1)")).toBeNull();
	});
});

describe("embedUrl", () => {
	it("embeds via the no-cookie host, starting where the link pointed", () => {
		const url = embedUrl({ id: "3S5rnnI7VSs", startS: 11 });
		expect(url).toContain("https://www.youtube-nocookie.com/embed/3S5rnnI7VSs?");
		expect(url).toContain("start=11");
	});

	// Autoplay without mute is refused in a cross-origin frame, and the athlete is
	// left tapping play a second time. The two belong together.
	it("plays on its own — muted, which is the only autoplay browsers allow here", () => {
		const url = embedUrl({ id: "3S5rnnI7VSs", startS: 0 });
		expect(url).toContain("autoplay=1");
		expect(url).toContain("mute=1");
	});

	it("shows the movement, not a video player", () => {
		const url = embedUrl({ id: "3S5rnnI7VSs", startS: 0 });
		expect(url).toContain("controls=0");
		expect(url).toContain("iv_load_policy=3");
	});
});
