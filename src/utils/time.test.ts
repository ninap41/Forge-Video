import { describe, expect, it } from "vitest";
import { basename, clamp, fmtMs } from "./time";

describe("fmtMs", () => {
  it("formats minutes, zero-padded seconds and tenths", () => {
    expect(fmtMs(0)).toBe("0:00.0");
    expect(fmtMs(1234)).toBe("0:01.2");
    expect(fmtMs(59_999)).toBe("0:59.9");
    expect(fmtMs(60_000)).toBe("1:00.0");
    expect(fmtMs(3_725_400)).toBe("62:05.4");
  });
  it("can hide tenths for ruler labels", () => {
    expect(fmtMs(1234, false)).toBe("0:01");
    expect(fmtMs(90_000, false)).toBe("1:30");
  });
  it("truncates rather than rounds, and never goes negative", () => {
    expect(fmtMs(1999)).toBe("0:01.9");
    expect(fmtMs(999.9)).toBe("0:00.9");
    expect(fmtMs(-500)).toBe("0:00.0");
    expect(fmtMs(NaN)).toBe("0:00.0");
  });
});

describe("basename", () => {
  it("returns the last path segment", () => {
    expect(basename("/Users/nina/Movies/take 1.mov")).toBe("take 1.mov");
    expect(basename("file.mp4")).toBe("file.mp4");
    expect(basename("")).toBe("");
    expect(basename("/trailing/")).toBe("");
  });
});

describe("clamp", () => {
  it("bounds a value inclusively", () => {
    expect(clamp(5, 0, 10)).toBe(5);
    expect(clamp(-1, 0, 10)).toBe(0);
    expect(clamp(11, 0, 10)).toBe(10);
    expect(clamp(0, 0, 0)).toBe(0);
  });
});
