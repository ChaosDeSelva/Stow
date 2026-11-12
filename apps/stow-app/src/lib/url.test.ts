import { describe, expect, it } from "vitest";
import { countIgnoredLines, extractFirstUrl, extractUrls } from "./url";

describe("extractFirstUrl", () => {
  it("returns a bare https URL unchanged", () => {
    expect(extractFirstUrl("https://example.com/a/b?c=1")).toBe("https://example.com/a/b?c=1");
  });

  it("finds the first http(s) token inside free text and strips trailing punctuation", () => {
    expect(extractFirstUrl("Look at this: http://example.org/post, it's great.")).toBe(
      "http://example.org/post",
    );
    expect(extractFirstUrl("(see https://ex.com/x).")).toBe("https://ex.com/x");
  });

  it("trims surrounding whitespace", () => {
    expect(extractFirstUrl("   https://example.com  \n")).toBe("https://example.com");
  });

  it("rejects non-http schemes and plain text", () => {
    expect(extractFirstUrl("chrome://extensions")).toBeNull();
    expect(extractFirstUrl("javascript:alert(1)")).toBeNull();
    expect(extractFirstUrl("just some words")).toBeNull();
    expect(extractFirstUrl("")).toBeNull();
  });

  it("keeps balanced closing parentheses that are part of the URL", () => {
    expect(extractFirstUrl("https://en.wikipedia.org/wiki/Foo_(bar)")).toBe(
      "https://en.wikipedia.org/wiki/Foo_(bar)",
    );
  });
});

describe("extractUrls", () => {
  it("splits on commas, whitespace and newlines and drops junk", () => {
    expect(extractUrls("https://a.com, https://b.com\nhttps://c.com words\tftp://x.y chrome://new")).toEqual([
      "https://a.com",
      "https://b.com",
      "https://c.com",
    ]);
  });

  it("de-duplicates while keeping first-seen order and strips trailing punctuation", () => {
    expect(extractUrls("see http://ex.org/p, http://ex.org/p. and https://z.io/q;")).toEqual(["http://ex.org/p", "https://z.io/q"]);
  });

  it("returns an empty list for empty or url-free text", () => {
    expect(extractUrls("")).toEqual([]);
    expect(extractUrls("nothing here, really")).toEqual([]);
  });
});

describe("countIgnoredLines", () => {
  it("counts the non-empty lines that hold no http(s) web address", () => {
    expect(countIgnoredLines("https://a.com, https://b.com\nsome note I typed\nexample.com\nftp://x.y chrome://new")).toBe(3);
    expect(countIgnoredLines("see https://ok.example/ here\nnot a link")).toBe(1);
  });

  it("is zero for empty text, blank lines, or lines made only of links and separators", () => {
    expect(countIgnoredLines("")).toBe(0);
    expect(countIgnoredLines("  https://a.com ,\n\n https://b.com  \n")).toBe(0);
  });
});
