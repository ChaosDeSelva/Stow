import { describe, expect, it } from "vitest";
import { parseTaskLines, renderSnapshot, renderUserMarkdown, wordCount } from "./markdown";

describe("renderUserMarkdown (notes & AI output)", () => {
  it("strips images so AI output can't fetch remote URLs", () => {
    const html = renderUserMarkdown("Hi ![x](https://evil.example/pixel.png) <img src=https://evil.example/a.png onerror=alert(1)>");
    expect(html).not.toContain("<img");
    expect(html).not.toContain("evil.example/pixel");
    expect(html).not.toContain("onerror");
  });

  it("strips forms, iframes, svg, style tags and style attributes", () => {
    const html = renderUserMarkdown(
      '<form><input><button>x</button></form><iframe src="https://x"></iframe><svg><circle/></svg><style>body{}</style><p style="color:red">ok</p>',
    );
    for (const bad of ["<form", "<input", "<button", "<iframe", "<svg", "<style", "style="]) expect(html).not.toContain(bad);
    expect(html).toContain("ok");
  });

  it("keeps ordinary Markdown", () => {
    expect(renderUserMarkdown("**bold** [link](https://a.example)")).toContain('<a href="https://a.example">link</a>');
  });
});

describe("renderSnapshot (readable copies)", () => {
  it("turns a remote image into an [image] link by default", () => {
    const html = renderSnapshot("![chart](https://cdn.example/c.png)");
    expect(html).not.toContain("<img");
    expect(html).toContain('href="https://cdn.example/c.png"');
    expect(html).toContain("[image: chart]");
  });

  it("keeps remote images when Load images is on", () => {
    const html = renderSnapshot("![](https://cdn.example/c.png)", { loadImages: true });
    expect(html).toContain("<img");
    expect(html).toContain('referrerpolicy="no-referrer"');
  });

  it("an image inside a link becomes text (no nested links)", () => {
    const html = renderSnapshot("[![](https://cdn.example/c.png)](https://site.example)");
    expect(html).not.toContain("<img");
    expect(html.match(/<a /g)?.length).toBe(1);
    expect(html).toContain("[image]");
  });

  it("still strips active content", () => {
    expect(renderSnapshot("<iframe src=x></iframe><script>1</script>ok")).not.toMatch(/iframe|script/);
  });
});

describe("helpers", () => {
  it("parseTaskLines reads list items", () => {
    expect(parseTaskLines("- [ ] One\n* **Two**\n3. Three\nnot a task")).toEqual(["One", "Two", "Three"]);
  });
  it("wordCount counts words", () => {
    expect(wordCount("# Title\n\nSome *words* here")).toBe(4);
    expect(wordCount("")).toBe(0);
  });
});
