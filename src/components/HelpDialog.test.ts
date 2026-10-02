import { describe, expect, it } from "vitest";
import { mount } from "@vue/test-utils";
import HelpDialog, { GUIDE, MOUSE, SHORTCUTS, SHORTCUTS_SECTION } from "./HelpDialog.vue";

const nav = (w: ReturnType<typeof mount>) => w.findAll("[data-testid=help-nav]");
const heading = (w: ReturnType<typeof mount>) => w.find("[data-testid=help-body] h3").text();

describe("HelpDialog", () => {
  it("renders nothing while closed", () => {
    expect(mount(HelpDialog, { props: { open: false } }).find("[role=dialog]").exists()).toBe(false);
  });

  it("covers every part of the app: one nav entry per section, each with an intro and detailed items", async () => {
    const w = mount(HelpDialog, { props: { open: true } });
    expect(nav(w).map((b) => b.text())).toEqual(GUIDE.map((g) => g.title));
    for (const t of ["Overview", "Project", "Media pool", "V1 video track", "Overlay rows", "Text rows (titles)", "Audio tracks", "Range selection & pinned loops", "Preview & framing", "AI mode", "Export", "Keyboard & mouse"]) {
      expect(GUIDE.map((g) => g.title)).toContain(t);
    }
    for (const g of GUIDE) {
      await nav(w).find((b) => b.text() === g.title)!.trigger("click");
      expect(heading(w)).toBe(g.title);
      expect(w.find("[data-testid=help-body]").text()).toContain(g.intro);
      if (g.shortcuts) continue;
      expect(g.items.length).toBeGreaterThanOrEqual(4);
      for (const i of g.items) { expect(w.text()).toContain(i.label); expect(w.text()).toContain(i.text); }
    }
  });

  it("the shortcuts section lists every key and mouse gesture", async () => {
    const w = mount(HelpDialog, { props: { open: true, section: SHORTCUTS_SECTION } });
    expect(heading(w)).toBe("Keyboard & mouse");
    expect(nav(w).find((b) => b.text() === "Keyboard & mouse")!.attributes("aria-current")).toBe("true");
    for (const s of [...SHORTCUTS, ...MOUSE]) { expect(w.text()).toContain(s.keys); expect(w.text()).toContain(s.action); }
  });

  it("re-opening jumps to the requested section, an unknown one falls back to the first", async () => {
    const w = mount(HelpDialog, { props: { open: false, section: "export" } });
    await w.setProps({ open: true });
    expect(heading(w)).toBe("Export");
    await nav(w)[0]!.trigger("click");
    expect(heading(w)).toBe("Overview");
    await w.setProps({ open: false });
    await w.setProps({ open: true, section: "nope" });
    expect(heading(w)).toBe("Overview");
  });

  it("closes from ✕, Close and the backdrop", async () => {
    const w = mount(HelpDialog, { props: { open: true } });
    await w.find("button[aria-label=Close]").trigger("click");
    await w.findAll("button").find((b) => b.text() === "Close")!.trigger("click");
    await w.find(".fixed").trigger("pointerdown");
    expect(w.emitted("close")).toHaveLength(3);
  });
});
