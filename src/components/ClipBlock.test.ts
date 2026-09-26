import { describe, expect, it } from "vitest";
import { mount } from "@vue/test-utils";
import ClipBlock from "./ClipBlock.vue";
import { clip, project } from "../test/fixtures";
import { ctx2d } from "../test/setup";

const mountBlock = (over = {}, props = {}) => {
  const c = clip({ source: "/videos/take 1.mov", ...over });
  project([clip(), c]);
  return { c, w: mount(ClipBlock, { props: { clip: c, pxPerMs: 0.1, selected: false, index: 1, ...props } }) };
};

describe("ClipBlock", () => {
  it("positions itself from timeline_start and duration", () => {
    const { w } = mountBlock({ source_start: 1000, source_end: 3000 });
    const style = (w.element as HTMLElement).style;
    expect(style.left).toBe("500px");
    expect(style.width).toBe("200px");
  });

  it("labels with 1-based index and file name", () => {
    const { w } = mountBlock();
    expect(w.text()).toContain("2 · take 1.mov");
  });

  it("shows selection styling", () => {
    const { w } = mountBlock({}, { selected: true });
    expect(w.classes()).toContain("border-accent");
    const { w: w2 } = mountBlock();
    expect(w2.classes()).toContain("border-line");
  });

  it("emits select+dragStart on body, select+trim on the handles", async () => {
    const { w } = mountBlock();
    await w.trigger("pointerdown");
    expect(w.emitted("select")).toHaveLength(1);
    expect(w.emitted("dragStart")).toHaveLength(1);
    const handles = w.findAll(".cursor-ew-resize");
    expect(handles).toHaveLength(2);
    await handles[0].trigger("pointerdown");
    await handles[1].trigger("pointerdown");
    expect(w.emitted("trimStart")).toHaveLength(1);
    expect(w.emitted("trimEnd")).toHaveLength(1);
    expect(w.emitted("select")).toHaveLength(3);
    expect(w.emitted("dragStart")).toHaveLength(1);
  });

  it("draws fade ramps and a transition badge sized in pixels", () => {
    const { w } = mountBlock({ fade_in: 500, fade_out: 250, transition_out: { type: "CrossDissolve", ms: 1000 } });
    const grads = w.findAll("[class*='bg-gradient']");
    expect(grads).toHaveLength(2);
    expect((grads[0].element as HTMLElement).style.width).toBe("50px");
    expect((grads[1].element as HTMLElement).style.width).toBe("25px");
    const badge = w.find(".border-accent-2");
    expect((badge.element as HTMLElement).style.width).toBe("100px");
    expect(badge.text()).toBe("⨯");
    const { w: dip } = mountBlock({ transition_out: { type: "DipToBlack", ms: 300 } });
    expect(dip.find(".border-accent-2").text()).toBe("■");
    const { w: cut } = mountBlock();
    expect(cut.find(".border-accent-2").exists()).toBe(false);
    expect(cut.findAll("[class*='bg-gradient']")).toHaveLength(0);
  });

  it("shows only the thumbnails that fall inside the trimmed range", () => {
    const thumbs = { intervalMs: 1000, urls: ["u0", "u1", "u2", "u3", "u4"] };
    const { w } = mountBlock({ source_start: 1500, source_end: 3500 }, { thumbs });
    const imgs = w.findAll("img");
    expect(imgs.map((i) => i.attributes("src"))).toEqual(["u1", "u2", "u3"]);
    expect((imgs[0].element as HTMLElement).style.left).toBe("-50px");
    expect((imgs[0].element as HTMLElement).style.width).toBe("100px");
    const { w: none } = mountBlock({}, { thumbs: { intervalMs: 200, urls: [] } });
    expect(none.findAll("img")).toHaveLength(0);
  });

  it("paints waveform peaks onto the canvas, but not when muted", async () => {
    ctx2d.fillRect.mockClear();
    const peaks = new Array(500).fill(200);
    mountBlock({}, { peaks });
    await Promise.resolve();
    expect(ctx2d.fillRect).toHaveBeenCalled();
    ctx2d.fillRect.mockClear();
    mountBlock({ muted: true }, { peaks });
    await Promise.resolve();
    expect(ctx2d.fillRect).not.toHaveBeenCalled();
  });
});
